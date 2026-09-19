//! Plan source-backed tbl cells and replay tbl's bounded lexical execution.
use crate::mandoc::{
    LoweringContext, TableTextBlock,
    inline::{
        FilledBoundary, FragmentEnhancement, InlineBuilder, enhance_source_fragment, plain_text,
    },
};
use libmandoc_rs::{Node, NodeKind};
use mant_ir::Inline;

pub(in crate::mandoc::blocks) struct TableEmbedding {
    pub(super) blocks: Vec<TableTextBlock>,
}

fn table_embeddings(nodes: &[Node], context: &LoweringContext<'_>) -> TableEmbeddingPlan {
    let mut embeddings = (0..nodes.len()).map(|_| None).collect::<Vec<_>>();
    for (index, node) in nodes.iter().enumerate() {
        if node.kind != NodeKind::Table {
            continue;
        }
        let blocks = context.table_text_blocks(
            node.line,
            node.table_cells
                .iter()
                .filter(|cell| cell.text_block)
                .count(),
            node.table_escape,
        );
        if blocks.is_empty() {
            continue;
        }
        embeddings[index] = Some(TableEmbedding { blocks });
    }
    TableEmbeddingPlan { embeddings }
}

/// One sibling stream owns source coordinates for embedded table text.
///
/// Native macro expansion can reuse the source line of a table cell while
/// emitting ordinary AST siblings after the table. Those siblings remain in
/// the normal block stream unless a native execution witness proves that a
/// source recovery transaction consumed them; line-number overlap alone is
/// never ownership evidence.
pub(in crate::mandoc::blocks) struct TableEmbeddingPlan {
    embeddings: Vec<Option<TableEmbedding>>,
}
impl TableEmbeddingPlan {
    pub(in crate::mandoc::blocks) fn new(nodes: &[Node], context: &LoweringContext<'_>) -> Self {
        table_embeddings(nodes, context)
    }
    pub(in crate::mandoc::blocks) fn embedding(&self, index: usize) -> Option<&TableEmbedding> {
        self.embeddings[index].as_ref()
    }
}
/// A cell's position is needed to interpret row-local tbl layout controls.
#[derive(Clone, Copy)]
pub(super) struct CellPosition<'a> {
    pub(super) index: usize,
    pub(super) row: &'a [libmandoc_rs::TableCell],
}

#[must_use]
struct CellCandidate {
    inlines: Vec<Inline>,
    formatter: crate::mandoc::formatter::FormatterState,
    diagnostics: Vec<mant_ir::Diagnostic>,
}

struct NativeCellEvidence<'a> {
    cell: &'a libmandoc_rs::TableCell,
    position: CellPosition<'a>,
    source_operands: &'a str,
}

impl NativeCellEvidence<'_> {
    /// An empty normalized cell can shift source-block association. Accept
    /// styles only when the candidate agrees with this cell or is not proven
    /// to belong to another one; a control-only empty cell still commits state.
    fn authorizes(&self, candidate: &CellCandidate) -> bool {
        // A complete synthetic parse has no authority on its own.  The
        // parser records this row as safe only when tbl received the original
        // source rather than a user-macro expansion or renamed request.
        if !self.cell.source_recovery_safe {
            return false;
        }
        let native = self.cell.text.as_deref().filter(|text| !text.is_empty());
        if candidate.inlines.is_empty() {
            return native.is_none();
        }
        let text = plain_text(&candidate.inlines);
        // CVS mandoc invokes roff_expand() before tbl_read(). Strings,
        // registers, and macro arguments therefore need the original
        // session's tbl payload. Source recovery may fill an empty native
        // cell, but never replaces non-empty native content unless the
        // normalized visible text is exactly the same.
        if let Some(native) = native {
            // `roff_parsetext()` gives tbl direct high-level macro operands,
            // and the owned cell retains the executed operand stream. A
            // source fragment can deliberately change its presentation
            // (`.Fl Fl help` -> `--help`, `.MR printf 3` -> a typed
            // reference), so its display text is not evidence. The original
            // direct operand stream must match native text exactly.
            return table_text_agrees(self.source_operands, native);
        }
        !self
            .position
            .row
            .iter()
            .enumerate()
            .any(|(index, competing_cell)| {
                index != self.position.index
                    && competing_cell
                        .text
                        .as_deref()
                        .is_some_and(|native| table_text_agrees(&text, native))
            })
    }
}

impl CellCandidate {
    fn commit(
        self,
        context: &LoweringContext<'_>,
        formatter: &mut crate::mandoc::formatter::FormatterState,
    ) -> Vec<Inline> {
        *formatter = self.formatter;
        context.diagnostics.borrow_mut().extend(self.diagnostics);
        self.inlines
    }
}

enum TableEnhancement {
    Accepted(CellCandidate),
    Rejected,
    Exhausted,
}

struct TableEnhancementRequest<'a> {
    source: &'a str,
    escape: Option<u8>,
    dialect: libmandoc_rs::MacroSet,
    default_name: Option<&'a str>,
    synopsis: bool,
    formatter: crate::mandoc::formatter::FormatterState,
    evidence: NativeCellEvidence<'a>,
}

fn plan_table_enhancement(request: &TableEnhancementRequest<'_>) -> TableEnhancement {
    if contains_native_table_request(request.source) {
        return TableEnhancement::Rejected;
    }
    match enhance_source_fragment(
        request.source,
        request.escape,
        request.dialect,
        request.default_name,
        request.synopsis,
        request.formatter,
    ) {
        FragmentEnhancement::Accepted(recovered) => {
            let candidate = CellCandidate {
                inlines: recovered.inlines,
                formatter: recovered.formatter,
                diagnostics: Vec::new(),
            };
            if request.evidence.authorizes(&candidate) {
                TableEnhancement::Accepted(candidate)
            } else {
                TableEnhancement::Rejected
            }
        }
        FragmentEnhancement::Rejected => TableEnhancement::Rejected,
        FragmentEnhancement::Exhausted => TableEnhancement::Exhausted,
    }
}

/// `Some`, including an empty vector, is decoded native/recovered content.
/// `None` means no usable payload was available and source fallback may apply.
pub(super) fn lower_table_cell(
    cell: &libmandoc_rs::TableCell,
    position: CellPosition<'_>,
    node: &Node,
    context: &LoweringContext<'_>,
    text_block: Option<&TableTextBlock>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Option<Vec<Inline>> {
    if let Some(text_block) = text_block {
        let initial_state = *formatter;
        let diagnostic_start = context.diagnostics.borrow().len();
        let source = LoweringContext::table_execution_source(&text_block.source, text_block.escape);
        let source_operands = table_source_operands(&source);
        // CVS mandoc passes high-level macro operands into tbl, while GNU
        // tbl expands the same inline macro language. A `T{}` source block
        // may enrich its already-associated native cell, but an isolated
        // parse is never execution evidence on its own: redefined macros,
        // control characters, and conditionals belong to the original roff
        // session. Commit the candidate only after it agrees with this exact
        // native payload. This keeps unrelated document-local macros from
        // disabling recovery without allowing a synthetic parser to replace
        // what the native execution actually produced.
        let enhancement = plan_table_enhancement(&TableEnhancementRequest {
            source: &source,
            escape: text_block.escape,
            dialect: context.macro_set,
            default_name: context.default_name,
            synopsis: node.flags.synopsis_pretty,
            formatter: initial_state,
            evidence: NativeCellEvidence {
                cell,
                position,
                source_operands: &source_operands,
            },
        });
        let allow_raw_fallback = match enhancement {
            TableEnhancement::Accepted(mut candidate) => {
                candidate.diagnostics =
                    context.diagnostics.borrow_mut().split_off(diagnostic_start);
                return Some(candidate.commit(context, formatter));
            }
            TableEnhancement::Rejected => true,
            TableEnhancement::Exhausted => false,
        };

        if allow_raw_fallback {
            // The raw fallback is deliberately weaker than semantic recovery:
            // it never replays requests or macro meaning, only the complete
            // operand stream that native tbl would have received. It is not
            // attempted after enhancement exhaustion: a failed bounded
            // transaction cannot silently enter a less constrained path.
            if cell.text.as_deref().is_none_or(str::is_empty) {
                let mut candidate_state = initial_state;
                let recovered = lower_raw_table_text_block(&source, context, &mut candidate_state);
                let candidate_diagnostics =
                    context.diagnostics.borrow_mut().split_off(diagnostic_start);
                // Recovery remains transactional: it can replace native text
                // only when a raw candidate agrees with this exact cell.
                let candidate = CellCandidate {
                    inlines: recovered,
                    formatter: candidate_state,
                    diagnostics: candidate_diagnostics,
                };
                let evidence = NativeCellEvidence {
                    cell,
                    position,
                    source_operands: &source_operands,
                };
                if evidence.authorizes(&candidate) {
                    return Some(candidate.commit(context, formatter));
                }
            }
        }
        context.diagnostics.borrow_mut().truncate(diagnostic_start);
        *formatter = initial_state;
    }
    if cell.text.as_deref().is_some_and(|text| !text.is_empty()) {
        return Some(lower_table_cell_text(
            cell.text.as_deref().unwrap_or_default(),
            node.line,
            context,
            formatter,
        ));
    }
    None
}

fn table_text_agrees(reconstructed: &str, parsed: &str) -> bool {
    fn normalize(value: &str) -> String {
        // tbl's native macro branch may retain structural padding from a
        // no-operand wrapper (`.Oo`/`.Oc`) even though the corresponding
        // source operand stream has no physical blanks at that point. Keep
        // word boundaries as execution evidence, but normalize the width of
        // those formatter-owned runs. In particular, `A B` never equals
        // `AB`: recovery must not turn an executed space into concatenation.
        value.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    normalize(reconstructed) == normalize(parsed)
}

/// Build the exact high-level operand stream that CVS `roff_parsetext()`
/// hands to tbl for this bounded text block. It is intentionally evidence,
/// not a second parser: requests are reduced only to the operands native tbl
/// itself receives, and the owned `TableCell` must corroborate the result.
fn table_source_operands(source: &str) -> String {
    source
        .lines()
        .filter_map(table_cell_content_line)
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn lower_table_cell_text(
    source: &str,
    line: u32,
    context: &LoweringContext<'_>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    // `roff_expand()` retains one encoded stream even when a continued cell
    // changes its escape state.  During `.eo`, a literal backslash becomes
    // `\\e`; after a later `.ec`, real formatting escapes remain encoded as
    // such.  Decode that native stream exactly once: the row's `tbl_escape`
    // records only the state while that AST row was allocated and cannot
    // describe a later transition inside the same `T{ ... T}` cell.
    let Some((opening, closing)) = context.equation_delimiters_at(line) else {
        return context.lower_text(source, formatter);
    };
    // Equation delimiters annotate part of one native `term_word()`; they do
    // not split formatter execution.  Re-encode the normalized expression as
    // a literal code-font run, then decode the complete cell once so `\z`,
    // `\p`, fonts, and word boundaries remain ordered across the delimiter.
    let mut parts = Vec::new();
    let mut remainder = source;
    while let Some(opening_index) = remainder.find(opening) {
        let after_opening = &remainder[opening_index + opening.len_utf8()..];
        let Some(closing_index) = after_opening.find(closing) else {
            break;
        };
        parts.push(crate::mandoc::inline::FormatterWordPart::Source(
            &remainder[..opening_index],
        ));
        let expression = &after_opening[..closing_index];
        if !expression.trim().is_empty() {
            parts.push(crate::mandoc::inline::FormatterWordPart::Code(
                context.normalize_equation(expression, line),
            ));
        }
        remainder = &after_opening[closing_index + closing.len_utf8()..];
    }
    parts.push(crate::mandoc::inline::FormatterWordPart::Source(remainder));
    context.lower_formatter_word_parts(&parts, formatter)
}

fn lower_raw_table_text_block(
    source: &str,
    context: &LoweringContext<'_>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    // `tbl_cdata()` appends every admitted physical input line to one cell
    // string, separated by exactly one ASCII blank. Native requests are
    // consumed before that point and contribute no cell word. Replaying the
    // admitted lines independently would invent formatter-word boundaries:
    // in particular, `\c` and deferred `\p` must observe tbl's inserted
    // blank inside this one word.
    let operands = source
        .lines()
        .filter_map(table_cell_content_line)
        .map(|line| {
            let line = line.trim();
            if line.starts_with(['.', '\'']) {
                format!("\\&{line}")
            } else {
                line.to_owned()
            }
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let operand_source = operands.join(" ");
    // A malformed raw operand must remain visible even if the richer bounded
    // fragment parser declines it. This final path decodes complete escape
    // operands but intentionally does not assign macro semantics.
    let mut builder = InlineBuilder::with_spacing(formatter.spacing);
    builder.inherit_vertical_space_debt(formatter.vertical_space_debt);
    if !operand_source.is_empty() {
        let lowered = context.lower_text(&operand_source, formatter);
        builder.begin_word_projection(mant_ir::has_printable_character(&lowered));
        builder.append_filled(lowered, FilledBoundary::Word);
    }
    formatter.spacing = builder.spacing_enabled();
    formatter.vertical_space_debt = builder.vertical_space_debt();
    builder.finish()
}

/// Native roff requests execute before tbl sees a high-level macro operand.
/// Do not feed a cell containing one to the isolated inline parser: it has no
/// document-session request state and must not reinterpret that boundary.
fn contains_native_table_request(source: &str) -> bool {
    source.lines().any(|line| {
        let Some(name) = crate::mandoc::inline::control_line_request(line) else {
            return false;
        };
        libmandoc_rs::is_native_roff_request(name)
    })
}

/// Extract the input `tbl_read()` receives from one physical `T{}` line.
///
/// In CVS mandoc, `roff_parsetext()` recognizes tables before dispatching a
/// man or mdoc macro. Unknown/high-level control words are removed and their
/// operands are passed to tbl verbatim. The small set of native requests
/// accepted in that table branch (`br`, `ce`, `rj`, and `sp`) is ignored. The
/// source-context service already executes `ec` and `eo` for lexical escape
/// state, so they likewise have no visible table payload here.
fn table_cell_content_line(line: &str) -> Option<&str> {
    let Some((name, operands)) = crate::mandoc::inline::control_line_parts(line) else {
        return Some(line);
    };
    (!libmandoc_rs::is_native_roff_request(name)).then_some(operands)
}

#[cfg(test)]
mod tests {
    use crate::mandoc::inline::plain_text;

    #[test]
    fn native_table_witness_preserves_internal_whitespace() {
        assert!(super::table_text_agrees(" A B ", "A B"));
        assert!(super::table_text_agrees("A  B", "A B"));
        assert!(!super::table_text_agrees("A B", "AB"));
    }

    #[test]
    fn complete_semantic_table_recovery_commits_its_formatter_state() {
        fn find(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
            if node.kind == libmandoc_rs::NodeKind::Table {
                Some(node)
            } else {
                node.children.iter().find_map(find)
            }
        }
        let report = libmandoc_rs::Parser::new(libmandoc_rs::ParseOptions::default())
            .parse_bytes("state.1", b".Dd September 8, 2026\n.Dt STATE 1\n.Os\n.Sh DESCRIPTION\n.TS\nl l.\nWORD\tNEXT\n.TE\n").unwrap();
        let mut node = find(&report.document.root).unwrap().clone();
        // This unit supplies an artificial text block; model the direct tbl
        // dispatch proof that a real parser report would carry with it.
        node.table_source_recovery_safe = true;
        node.table_cells[0].source_recovery_safe = true;
        let mut context = crate::mandoc::LoweringContext::new(None, None);
        context.macro_set = libmandoc_rs::MacroSet::Mdoc;
        for (source, native_text, expected_text, expected_spacing) in [
            (".Sm off\n.Em WORD", "off WORD", "WORD", false),
            (".Fl Fl help", "Fl help", "--help", true),
        ] {
            let mut cell = node.table_cells[0].clone();
            cell.text = Some(native_text.to_owned());
            cell.text_block = true;
            let block = super::TableTextBlock {
                source: source.to_owned(),
                escape: Some(b'\\'),
            };
            let mut state = crate::mandoc::formatter::FormatterState::default();
            state
                .font
                .push_scope(crate::mandoc::roff_escape::RoffFont::Strong);
            let result = super::lower_table_cell(
                &cell,
                super::CellPosition {
                    index: 0,
                    row: &node.table_cells,
                },
                &node,
                &context,
                Some(&block),
                &mut state,
            );
            assert_eq!(
                plain_text(&result.expect("decoded cell")),
                expected_text,
                "{source}"
            );
            assert_eq!(state.spacing, expected_spacing, "{source}");
        }
    }

    #[test]
    fn incomplete_cell_recovery_keeps_whole_native_payload_or_whole_source() {
        fn table_node(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
            if node.kind == libmandoc_rs::NodeKind::Table {
                return Some(node);
            }
            node.children.iter().find_map(table_node)
        }
        let report = libmandoc_rs::Parser::new(libmandoc_rs::ParseOptions {
            includes: libmandoc_rs::IncludePolicy::Deny,
            compression: libmandoc_rs::Compression::Plain,
        })
        .parse_bytes(
            "fallback.1",
            b".TH FALLBACK 1\n.SH DESCRIPTION\n.TS\nl.\nplaceholder\n.TE\n",
        )
        .unwrap();
        let mut node = table_node(&report.document.root).unwrap().clone();
        node.table_source_recovery_safe = true;
        node.table_cells[0].source_recovery_safe = true;
        let mut context = crate::mandoc::LoweringContext::new(None, None);
        context.macro_set = libmandoc_rs::MacroSet::Man;
        let block = super::TableTextBlock {
            source: ".B TOKENA\n.PP\nTOKENB".to_owned(),
            escape: Some(b'\\'),
        };
        for (native, expected) in [
            (None, "TOKENA TOKENB"),
            (Some(""), "TOKENA TOKENB"),
            (
                Some("TOKENA TOKENB complete native payload"),
                "TOKENA TOKENB complete native payload",
            ),
        ] {
            let mut cell = node.table_cells[0].clone();
            cell.text = native.map(str::to_owned);
            cell.text_block = true;
            let inlines = super::lower_table_cell(
                &cell,
                super::CellPosition {
                    index: 0,
                    row: std::slice::from_ref(&cell),
                },
                &node,
                &context,
                Some(&block),
                &mut crate::mandoc::formatter::FormatterState::default(),
            );
            assert_eq!(
                plain_text(&inlines.expect("complete fallback payload")),
                expected
            );
        }

        // `tbl_cdata()` builds one formatter word from admitted operands.
        // Requests contribute no text, `\c` observes the inserted blank,
        // and `\p` is realized there. Physical T{} source rows are not IR
        // hard lines; CVS may wrap the resulting word later from column width.
        for (source, expected) in [
            (".B TOKENA\n.br\nTOKENB", "TOKENA TOKENB"),
            (".B A\\c\n.br\nB", "A B"),
            (".B A\\p\n.br\nB C", "A\nB C"),
            ("\\&\n.br\nB", " B"),
            ("\\&\n.sp 0\nB", " B"),
            ("\\&\n.sp 1\nB", " B"),
            ("\\&\n.mc |\nB", " B"),
            ("\\&\n.ti 4n\nB", " B"),
        ] {
            let block = super::TableTextBlock {
                source: source.to_owned(),
                escape: Some(b'\\'),
            };
            for native in [None, Some("")] {
                let mut cell = node.table_cells[0].clone();
                cell.text = native.map(str::to_owned);
                cell.text_block = true;
                let inlines = super::lower_table_cell(
                    &cell,
                    super::CellPosition {
                        index: 0,
                        row: std::slice::from_ref(&cell),
                    },
                    &node,
                    &context,
                    Some(&block),
                    &mut crate::mandoc::formatter::FormatterState::default(),
                );
                assert_eq!(
                    plain_text(&inlines.expect("raw operand fallback")),
                    expected,
                    "source={source:?} native={native:?}"
                );
            }
        }
    }

    #[test]
    fn exhausted_semantic_enhancement_never_enters_raw_fallback() {
        fn table_node(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
            if node.kind == libmandoc_rs::NodeKind::Table {
                return Some(node);
            }
            node.children.iter().find_map(table_node)
        }
        let report = libmandoc_rs::Parser::new(libmandoc_rs::ParseOptions::default())
            .parse_bytes(
                "exhausted.1",
                b".TH EXHAUSTED 1\n.SH DESCRIPTION\n.TS\nl.\nplaceholder\n.TE\n",
            )
            .unwrap();
        let mut node = table_node(&report.document.root).unwrap().clone();
        node.table_source_recovery_safe = true;
        let mut cell = node.table_cells[0].clone();
        cell.text = None;
        cell.text_block = true;
        cell.source_recovery_safe = true;
        let block = super::TableTextBlock {
            source: "x".repeat(65_537),
            escape: Some(b'\\'),
        };
        let context = crate::mandoc::LoweringContext::new(None, None);
        let mut state = crate::mandoc::formatter::FormatterState::default();
        let before = state;

        // This is a ManT resource-policy assertion. Fixed CVS remains the
        // native-content authority; exhausting the optional overlay must not
        // authorize a second, less constrained interpretation of its source.
        assert_eq!(
            super::lower_table_cell(
                &cell,
                super::CellPosition {
                    index: 0,
                    row: std::slice::from_ref(&cell),
                },
                &node,
                &context,
                Some(&block),
                &mut state,
            ),
            None
        );
        assert_eq!(state, before);
        assert!(context.diagnostics.borrow().is_empty());
    }

    #[test]
    fn table_dispatch_preserves_raw_high_level_operands_and_hides_roff_requests() {
        assert_eq!(
            super::table_cell_content_line(".BR git (1)"),
            Some("git (1)")
        );
        assert_eq!(
            super::table_cell_content_line(r"\.BR linked (3)"),
            Some("linked (3)")
        );
        assert_eq!(super::table_cell_content_line(".Sm off"), Some("off"));
        assert_eq!(super::table_cell_content_line(".ll 50n"), None);
        assert_eq!(super::table_cell_content_line(".po 0n"), None);
        assert_eq!(
            super::table_cell_content_line("plain table payload"),
            Some("plain table payload")
        );
    }

    #[test]
    fn unrelated_recovered_table_text_never_replaces_a_parsed_cell() {
        assert!(super::table_text_agrees("git(1)", "git(1)"));
        assert!(!super::table_text_agrees(
            "project documentation ⟨https://example.test⟩",
            "project documentation"
        ));
        assert!(!super::table_text_agrees(
            "Core",
            "Production-grade, first-class"
        ));
    }

    #[test]
    fn native_requests_keep_table_cells_on_the_raw_recovery_path() {
        assert!(super::contains_native_table_request(".br\nvisible"));
        assert!(super::contains_native_table_request(".ll 80n\nvisible"));
        assert!(!super::contains_native_table_request(".No a Ns No b"));
        assert!(!super::contains_native_table_request(".BR git (1)"));
    }
}
