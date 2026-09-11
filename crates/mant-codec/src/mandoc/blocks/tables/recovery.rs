//! Plan source-backed tbl cells and recover only complete semantic payloads.
use crate::mandoc::{
    LoweringContext, TableTextBlock,
    inline::{
        FilledBoundary, InlineBuilder, lower_man_link, lower_source_fragment_with_formatter_state,
        plain_text,
    },
    roff_escape::visible_text,
};
use libmandoc_rs::{Node, NodeKind};
use mant_ir::Inline;

pub(in crate::mandoc::blocks) struct TableEmbedding<'a> {
    pub(super) blocks: Vec<TableTextBlock>,
    pub(super) nodes: Vec<&'a Node>,
}

fn table_embeddings<'a>(
    nodes: &'a [Node],
    context: &LoweringContext<'_>,
) -> TableEmbeddingPlan<'a> {
    let mut embeddings = (0..nodes.len()).map(|_| None).collect::<Vec<_>>();
    let mut consumed = vec![false; nodes.len()];
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
        );
        let Some(last_line) = blocks.iter().map(|block| block.end_line).max() else {
            continue;
        };
        let mut semantic_nodes = Vec::new();
        for (candidate_index, candidate) in nodes.iter().enumerate().skip(index + 1) {
            if candidate.line > last_line {
                break;
            }
            if blocks
                .iter()
                .any(|block| block.contains_line(candidate.line))
            {
                consumed[candidate_index] = true;
                semantic_nodes.push(candidate);
            }
        }
        embeddings[index] = Some(TableEmbedding {
            blocks,
            nodes: semantic_nodes,
        });
    }
    TableEmbeddingPlan {
        embeddings,
        consumed,
    }
}

/// One sibling stream owns both embeddings and their consumed-node bitmap.
pub(in crate::mandoc::blocks) struct TableEmbeddingPlan<'a> {
    embeddings: Vec<Option<TableEmbedding<'a>>>,
    consumed: Vec<bool>,
}
impl<'a> TableEmbeddingPlan<'a> {
    pub(in crate::mandoc::blocks) fn new(nodes: &'a [Node], context: &LoweringContext<'_>) -> Self {
        table_embeddings(nodes, context)
    }
    pub(in crate::mandoc::blocks) fn consumes(&self, index: usize) -> bool {
        self.consumed[index]
    }
    pub(in crate::mandoc::blocks) fn embedding(&self, index: usize) -> Option<&TableEmbedding<'a>> {
        self.embeddings[index].as_ref()
    }
}
pub(super) fn lower_missing_table_cell(
    source: Option<&str>,
    node: &Node,
    context: &LoweringContext<'_>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    let source = source.unwrap_or_default().trim();
    if source.is_empty() {
        return Vec::new();
    }
    let lowered = lower_table_cell_text(source, node.line, context, formatter);
    if !lowered.is_empty() {
        return lowered;
    }
    context.warn_unexpanded_table_cell(node.line);
    vec![Inline::Code {
        value: source.to_owned(),
    }]
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
    fn belongs_to(&self, cell: &libmandoc_rs::TableCell, position: CellPosition<'_>) -> bool {
        let native = cell.text.as_deref().filter(|text| !text.is_empty());
        if self.inlines.is_empty() {
            return native.is_none();
        }
        let text = plain_text(&self.inlines);
        // tbl_dat::string is produced after roff_expand() in the original
        // parser session. If native evaluated text exists, it is the content
        // authority: source-fragment recovery cannot recreate arbitrary
        // string/register state in a synthetic parser. Source recovery still
        // supplies macro/font structure when its visible result agrees.
        if let Some(native) = native {
            return table_text_agrees(&text, &visible_text(native));
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
    semantic_nodes: &[&Node],
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Option<Vec<Inline>> {
    if let Some(text_block) = text_block {
        let initial_state = *formatter;
        let diagnostic_start = context.diagnostics.borrow().len();
        let semantic_nodes = semantic_nodes
            .iter()
            .copied()
            .filter(|candidate| text_block.contains_line(candidate.line))
            .collect::<Vec<_>>();
        // A tbl `T{ ... T}` cell retains its source requests, while the
        // flattened libmandoc cell text has already discarded request-level
        // font and spacing semantics. Reconstruct from the bounded source
        // block first even when no printable AST siblings escaped the table.
        let mut candidate_state = initial_state;
        let recovered = lower_table_text_block(
            text_block,
            &semantic_nodes,
            context,
            node.flags.synopsis_pretty,
            &mut candidate_state,
        );
        // Recovery is transactional: a partially lowered cell is not a
        // replacement for the native payload. If that payload is absent,
        // retain the entire source rather than only the supported lines.
        let candidate_diagnostics = context.diagnostics.borrow_mut().split_off(diagnostic_start);
        let reconstructed = match recovered {
            TableTextRecovery::Complete(inlines) => inlines,
            TableTextRecovery::Incomplete => {
                context
                    .diagnostics
                    .borrow_mut()
                    .extend(candidate_diagnostics);
                *formatter = initial_state;
                if let Some(text) = cell.text.as_deref().filter(|text| !text.is_empty()) {
                    return Some(lower_table_cell_text(text, node.line, context, formatter));
                }
                return Some(context.lower_text(&text_block.source, formatter));
            }
        };
        let candidate = CellCandidate {
            inlines: reconstructed,
            formatter: candidate_state,
            diagnostics: candidate_diagnostics,
        };
        if candidate.belongs_to(cell, position) {
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
    if !cell.text_block {
        return None;
    }

    let request = text_block.and_then(|block| {
        block
            .source
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
    });
    let name = request
        .and_then(|line| {
            line.strip_prefix(".Nm")
                .or_else(|| line.strip_prefix("'Nm"))
        })
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        .map(str::trim)
        .and_then(|argument| {
            if argument.is_empty() {
                context.default_name.map(|name| {
                    vec![Inline::Text {
                        value: name.to_owned(),
                    }]
                })
            } else {
                Some(context.lower_text(argument, formatter))
            }
        });
    if let Some(children) = name.filter(|children| !children.is_empty()) {
        return Some(vec![Inline::Strong { children }]);
    }

    context.warn_unhandled_table_text_block(node);
    None
}

fn table_text_agrees(reconstructed: &str, parsed: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    };
    let reconstructed = normalize(reconstructed);
    let parsed = normalize(parsed);
    reconstructed == parsed
        || (!reconstructed.is_empty()
            && !parsed.is_empty()
            && (reconstructed.contains(&parsed) || parsed.contains(&reconstructed)))
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

enum TableTextRecovery {
    Complete(Vec<Inline>),
    Incomplete,
}

fn lower_table_text_block(
    block: &TableTextBlock,
    semantic_nodes: &[&Node],
    context: &LoweringContext<'_>,
    synopsis: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> TableTextRecovery {
    if let Some(recovered) = lower_source_fragment_with_formatter_state(
        &block.source,
        context.macro_set,
        context.default_name,
        synopsis,
        *formatter,
    ) {
        if !recovered.complete {
            context.warn_unhandled_table_text_block_line(block.start_line);
            return TableTextRecovery::Incomplete;
        }
        *formatter = recovered.formatter;
        return TableTextRecovery::Complete(recovered.inlines);
    }
    // A rejected request sequence cannot be proven complete by stitching
    // together whichever AST siblings escaped tbl. Even a present node may
    // only represent part of that sequence. Keep the whole cell instead.
    if let Some(offset) = block
        .source
        .lines()
        .position(|line| line.trim_start().starts_with(['.', '\'']))
    {
        context.warn_unhandled_table_text_block_line(
            block
                .start_line
                .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)),
        );
        return TableTextRecovery::Incomplete;
    }
    let mut builder = InlineBuilder::with_spacing(formatter.spacing);
    for (offset, source_line) in block.source.lines().enumerate() {
        let line = block
            .start_line
            .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX));
        let nodes = semantic_nodes
            .iter()
            .copied()
            .filter(|node| node.line == line)
            .collect::<Vec<_>>();
        if !nodes.is_empty() {
            for node in nodes {
                let spacing_enabled = builder.spacing_enabled();
                let lowered = if matches!(node.macro_name.as_deref(), Some("UR" | "MT")) {
                    lower_man_link(node, context.default_name, spacing_enabled)
                } else {
                    context.lower_inline_with_spacing(
                        std::slice::from_ref(node),
                        spacing_enabled,
                        formatter,
                    )
                };
                builder.inherit_spacing(formatter.spacing);
                builder.append_filled(lowered, FilledBoundary::Word);
            }
            continue;
        }

        let source_line = source_line.trim();
        if source_line.is_empty() {
            continue;
        }
        builder.append_filled(
            context.lower_text(source_line, formatter),
            FilledBoundary::Word,
        );
    }
    formatter.spacing = builder.spacing_enabled();
    TableTextRecovery::Complete(builder.finish())
}

#[cfg(test)]
mod tests {
    use mant_ir::Inline;

    use crate::mandoc::inline::plain_text;

    #[test]
    fn candidates_commit_or_discard_the_whole_formatter_state() {
        fn find(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
            if node.kind == libmandoc_rs::NodeKind::Table {
                Some(node)
            } else {
                node.children.iter().find_map(find)
            }
        }
        let report = libmandoc_rs::Parser::new(libmandoc_rs::ParseOptions::default())
            .parse_bytes("state.1", b".Dd September 8, 2026\n.Dt STATE 1\n.Os\n.Sh DESCRIPTION\n.TS\nl l.\nWORD\tNEXT\n.TE\n").unwrap();
        let node = find(&report.document.root).unwrap();
        let mut context = crate::mandoc::LoweringContext::new(None, None);
        context.macro_set = libmandoc_rs::MacroSet::Mdoc;
        for (source, expected_spacing, expected_text, native) in [
            (".Sm off\n.Em NEXT", true, "WORD", "WORD"),
            (".Sm off\n.Em WORD", false, "WORD", "WORD"),
            (".Sm off", false, "", ""),
            (".Sm off\n.Pp\nWORD", true, "WORD", "WORD"),
        ] {
            let mut cell = node.table_cells[0].clone();
            cell.text = Some(native.to_owned());
            cell.text_block = true;
            let block = super::TableTextBlock {
                source: source.to_owned(),
                start_line: 7,
                end_line: 9,
            };
            let mut state = crate::mandoc::formatter::FormatterState::default();
            state
                .font
                .push_scope(crate::mandoc::roff_escape::RoffFont::Strong);
            let mut expected = state;
            expected.spacing = expected_spacing;
            if source == ".Sm off\n.Em WORD" {
                let saved = expected
                    .font
                    .push_scope(crate::mandoc::roff_escape::RoffFont::Emphasis);
                expected.font.pop_scope(saved);
            }
            let result = super::lower_table_cell(
                &cell,
                super::CellPosition {
                    index: 0,
                    row: &node.table_cells,
                },
                node,
                &context,
                Some(&block),
                &[],
                &mut state,
            );
            assert_eq!(
                plain_text(&result.expect("decoded cell")),
                expected_text,
                "{source}"
            );
            assert_eq!(state.spacing, expected_spacing, "{source}");
            assert_eq!(state, expected, "{source}");
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
        let node = table_node(&report.document.root).unwrap();
        let mut context = crate::mandoc::LoweringContext::new(None, None);
        context.macro_set = libmandoc_rs::MacroSet::Man;
        let block = super::TableTextBlock {
            source: ".B TOKENA\n.PP\nTOKENB".to_owned(),
            start_line: 6,
            end_line: 8,
        };
        for native in [
            None,
            Some(""),
            Some("TOKENA TOKENB complete native payload"),
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
                node,
                &context,
                Some(&block),
                &[],
                &mut crate::mandoc::formatter::FormatterState::default(),
            );
            assert_eq!(
                plain_text(&inlines.expect("complete fallback payload")),
                native
                    .filter(|text| !text.is_empty())
                    .unwrap_or(&block.source)
            );
        }
    }

    #[test]
    fn source_requests_dispatch_to_man_and_mdoc_inline_lowering() {
        let man = super::lower_source_fragment_with_formatter_state(
            ".BR git (1)",
            libmandoc_rs::MacroSet::Man,
            None,
            false,
            crate::mandoc::formatter::FormatterState::default(),
        )
        .unwrap()
        .inlines;
        assert_eq!(plain_text(&man), "git(1)");

        let mdoc = super::lower_source_fragment_with_formatter_state(
            ".Xr git 1 ,",
            libmandoc_rs::MacroSet::Mdoc,
            None,
            false,
            crate::mandoc::formatter::FormatterState::default(),
        )
        .unwrap()
        .inlines;
        assert_eq!(plain_text(&mdoc), "git(1),");
        assert!(matches!(
            mdoc.first(),
            Some(Inline::Link {
                target:
                    mant_ir::LinkTarget::Manual {
                        name,
                        manual_section: Some(section),
                    },
                ..
            }) if name == "git" && section == "1"
        ));
    }

    #[test]
    fn unrelated_recovered_table_text_never_replaces_a_parsed_cell() {
        assert!(super::table_text_agrees("git(1)", "git(1)"));
        assert!(super::table_text_agrees(
            "project documentation ⟨https://example.test⟩",
            "project documentation"
        ));
        assert!(!super::table_text_agrees(
            "Core",
            "Production-grade, first-class"
        ));
    }
}
