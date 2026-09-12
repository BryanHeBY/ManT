//! Plan source-backed tbl cells and replay tbl's bounded lexical execution.
use crate::mandoc::{
    LoweringContext, TableTextBlock,
    inline::{
        FilledBoundary, InlineBuilder, lower_source_fragment_with_formatter_state, plain_text,
    },
    roff_escape::visible_text,
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

impl CellCandidate {
    /// An empty normalized cell can shift source-block association. Accept
    /// styles only when the candidate agrees with this cell or is not proven
    /// to belong to another one; a control-only empty cell still commits state.
    fn belongs_to(
        &self,
        cell: &libmandoc_rs::TableCell,
        position: CellPosition<'_>,
        source_operands: &str,
        source_recovery_safe: bool,
    ) -> bool {
        // A complete synthetic parse has no authority on its own.  The
        // parser records this row as safe only when tbl received the original
        // source rather than a user-macro expansion or renamed request.
        if !source_recovery_safe {
            return false;
        }
        let native = cell.text.as_deref().filter(|text| !text.is_empty());
        if self.inlines.is_empty() {
            return native.is_none();
        }
        let text = plain_text(&self.inlines);
        // CVS mandoc invokes roff_expand() before tbl_read(). Strings,
        // registers, and macro arguments therefore need the original
        // session's tbl payload. Source recovery may fill an empty native
        // cell, but never replaces non-empty native content unless the
        // normalized visible text is exactly the same.
        if let Some(native) = native {
            let native = visible_text(native);
            // `roff_parsetext()` gives tbl direct high-level macro operands,
            // and the owned cell retains the executed operand stream. A
            // source fragment can deliberately change its presentation
            // (`.Fl Fl help` -> `--help`, `.MR printf 3` -> a typed
            // reference), so its display text is not evidence. The original
            // direct operand stream must match native text exactly.
            return table_text_agrees(source_operands, &native);
        }
        !position.row.iter().enumerate().any(|(index, candidate)| {
            index != position.index
                && candidate
                    .text
                    .as_deref()
                    .is_some_and(|native| table_text_agrees(&text, &visible_text(native)))
        })
    }

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
        let source_operands = table_source_operands(context, &source);
        // CVS mandoc passes high-level macro operands into tbl, while GNU
        // tbl expands the same inline macro language. A `T{}` source block
        // may enrich its already-associated native cell, but an isolated
        // parse is never execution evidence on its own: redefined macros,
        // control characters, and conditionals belong to the original roff
        // session. Commit the candidate only after it agrees with this exact
        // native payload. This keeps unrelated document-local macros from
        // disabling recovery without allowing a synthetic parser to replace
        // what the native execution actually produced.
        if !contains_native_table_request(context, &source)
            && let Some(recovered) = lower_source_fragment_with_formatter_state(
                &source,
                text_block.escape,
                context.macro_set,
                context.default_name,
                node.flags.synopsis_pretty,
                initial_state,
            )
            && recovered.complete
        {
            let candidate = CellCandidate {
                inlines: recovered.inlines,
                formatter: recovered.formatter,
                diagnostics: Vec::new(),
            };
            if candidate.belongs_to(
                cell,
                position,
                &source_operands,
                node.table_source_recovery_safe,
            ) {
                return Some(candidate.commit(context, formatter));
            }
        }

        let mut candidate_state = initial_state;
        let recovered = lower_raw_table_text_block(&source, context, &mut candidate_state);
        // Recovery remains transactional: it can replace native text only
        // when a raw candidate agrees with this exact cell. A declined
        // semantic fragment therefore cannot replace a complete native cell
        // with a partial subset of its source.
        let candidate_diagnostics = context.diagnostics.borrow_mut().split_off(diagnostic_start);
        let candidate = CellCandidate {
            inlines: recovered,
            formatter: candidate_state,
            diagnostics: candidate_diagnostics,
        };
        if candidate.belongs_to(
            cell,
            position,
            &source_operands,
            node.table_source_recovery_safe,
        ) {
            return Some(candidate.commit(context, formatter));
        }
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
        visible_text(value)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    normalize(reconstructed) == normalize(parsed)
}

/// Build the exact high-level operand stream that CVS `roff_parsetext()`
/// hands to tbl for this bounded text block. It is intentionally evidence,
/// not a second parser: requests are reduced only to the operands native tbl
/// itself receives, and the owned `TableCell` must corroborate the result.
fn table_source_operands(context: &LoweringContext<'_>, source: &str) -> String {
    source
        .lines()
        .filter_map(|line| table_cell_content_line(context, line))
        .map(visible_text)
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
    let Some((opening, closing)) = context.equation_delimiters_at(line) else {
        return context.lower_text(source, formatter);
    };
    let mut output = Vec::new();
    let mut remainder = source;
    while let Some(opening_index) = remainder.find(opening) {
        let after_opening = &remainder[opening_index + opening.len_utf8()..];
        let Some(closing_index) = after_opening.find(closing) else {
            break;
        };
        output.extend(context.lower_text(&remainder[..opening_index], formatter));
        let expression = &after_opening[..closing_index];
        if !expression.trim().is_empty() {
            output.push(Inline::Code {
                value: context.normalize_equation(expression, line),
            });
        }
        remainder = &after_opening[closing_index + closing.len_utf8()..];
    }
    output.extend(context.lower_text(remainder, formatter));
    output
}

fn lower_raw_table_text_block(
    source: &str,
    context: &LoweringContext<'_>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(formatter.spacing);
    for source_line in source.lines() {
        let Some(source_line) = table_cell_content_line(context, source_line) else {
            continue;
        };
        let source_line = source_line.trim();
        if source_line.is_empty() {
            continue;
        }
        // `tbl_read()` receives the operand after the control name. Its
        // ordinary roff escapes remain active, but it has no high-level
        // macro/font state to reconstruct.
        builder.append_filled(
            context.lower_text(source_line, formatter),
            FilledBoundary::Word,
        );
    }
    formatter.spacing = builder.spacing_enabled();
    builder.finish()
}

/// Native roff requests execute before tbl sees a high-level macro operand.
/// Do not feed a cell containing one to the isolated inline parser: it has no
/// document-session request state and must not reinterpret that boundary.
fn contains_native_table_request(context: &LoweringContext<'_>, source: &str) -> bool {
    source.lines().any(|line| {
        let Some(request) = line.trim_start().strip_prefix(['.', '\'']) else {
            return false;
        };
        let name = request.split_whitespace().next().unwrap_or_default();
        context.is_native_table_request(name)
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
fn table_cell_content_line<'a>(context: &LoweringContext<'_>, line: &'a str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    let Some(request) = trimmed
        .strip_prefix('.')
        .or_else(|| trimmed.strip_prefix('\''))
    else {
        return Some(trimmed);
    };
    let request_end = request.find(' ').unwrap_or(request.len());
    let name = &request[..request_end];
    let operands = request[request_end..].trim_start_matches(' ');
    (!context.is_native_table_request(name)).then_some(operands)
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
    }

    #[test]
    fn table_dispatch_preserves_raw_high_level_operands_and_hides_roff_requests() {
        let mut context = crate::mandoc::LoweringContext::new(None, None);
        context.macro_set = libmandoc_rs::MacroSet::Mdoc;
        assert_eq!(
            super::table_cell_content_line(&context, ".BR git (1)"),
            Some("git (1)")
        );
        assert_eq!(
            super::table_cell_content_line(&context, ".Sm off"),
            Some("off")
        );
        assert_eq!(super::table_cell_content_line(&context, ".ll 50n"), None);
        assert_eq!(super::table_cell_content_line(&context, ".po 0n"), None);
        assert_eq!(
            super::table_cell_content_line(&context, "plain table payload"),
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
        let context = crate::mandoc::LoweringContext::new(None, None);
        assert!(super::contains_native_table_request(
            &context,
            ".br\nvisible"
        ));
        assert!(super::contains_native_table_request(
            &context,
            ".ll 80n\nvisible"
        ));
        assert!(!super::contains_native_table_request(
            &context,
            ".No a Ns No b"
        ));
        assert!(!super::contains_native_table_request(
            &context,
            ".BR git (1)"
        ));
    }
}
