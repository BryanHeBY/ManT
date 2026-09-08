//! Plan source-backed tbl cells and recover only complete semantic payloads.
use crate::mandoc::{
    LoweringContext, TableTextBlock,
    inline::{
        FilledBoundary, InlineBuilder, lower_man_link, lower_source_fragment_with_font_state,
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

pub(super) fn lower_table_cell(
    cell: &libmandoc_rs::TableCell,
    position: CellPosition<'_>,
    node: &Node,
    context: &LoweringContext<'_>,
    text_block: Option<&TableTextBlock>,
    semantic_nodes: &[&Node],
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    let CellPosition {
        index: cell_index,
        row: row_cells,
    } = position;
    if let Some(text_block) = text_block {
        let initial_font = formatter.font;
        let semantic_nodes = semantic_nodes
            .iter()
            .copied()
            .filter(|candidate| text_block.contains_line(candidate.line))
            .collect::<Vec<_>>();
        // A tbl `T{ ... T}` cell retains its source requests, while the
        // flattened libmandoc cell text has already discarded request-level
        // font and spacing semantics. Reconstruct from the bounded source
        // block first even when no printable AST siblings escaped the table.
        let recovered = lower_table_text_block(
            text_block,
            &semantic_nodes,
            context,
            node.flags.synopsis_pretty,
            formatter,
        );
        // Recovery is transactional: a partially lowered cell is not a
        // replacement for the native payload. If that payload is absent,
        // retain the entire source rather than only the supported lines.
        let reconstructed = match recovered {
            TableTextRecovery::Complete(inlines) => inlines,
            TableTextRecovery::Incomplete => {
                formatter.font = initial_font;
                if let Some(text) = cell.text.as_deref().filter(|text| !text.is_empty()) {
                    return lower_table_cell_text(text, node.line, context, formatter);
                }
                context.lower_text(&text_block.source, formatter)
            }
        };
        if !reconstructed.is_empty() {
            // libmandoc associates the row with its first physical input
            // line, but an empty `T{ T}` cell can be normalized to an
            // ordinary empty cell. Source recovery then has fewer semantic
            // cells than physical text blocks and may hand the next cell's
            // block to this one. Never replace the parser's known visible
            // payload with unrelated recovered text; retain source-derived
            // styles and links whenever the two representations still
            // contain one another.
            let reconstructed_text = plain_text(&reconstructed);
            let agrees_with_current = cell
                .text
                .as_deref()
                .is_none_or(|text| table_text_agrees(&reconstructed_text, &visible_text(text)));
            let belongs_to_other_cell = row_cells.iter().enumerate().any(|(index, candidate)| {
                index != cell_index
                    && candidate.text.as_deref().is_some_and(|text| {
                        table_text_agrees(&reconstructed_text, &visible_text(text))
                    })
            });
            if agrees_with_current || !belongs_to_other_cell {
                return reconstructed;
            }
        }
        formatter.font = initial_font;
    }
    if cell.text.as_deref().is_some_and(|text| !text.is_empty()) {
        return lower_table_cell_text(
            cell.text.as_deref().unwrap_or_default(),
            node.line,
            context,
            formatter,
        );
    }
    if !cell.text_block {
        return Vec::new();
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
        return vec![Inline::Strong { children }];
    }

    context.warn_unhandled_table_text_block(node);
    Vec::new()
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
    if let Some(recovered) = lower_source_fragment_with_font_state(
        &block.source,
        context.macro_set,
        context.default_name,
        synopsis,
        formatter.font,
    ) {
        if !recovered.complete {
            context.warn_unhandled_table_text_block_line(block.start_line);
            return TableTextRecovery::Incomplete;
        }
        formatter.font = recovered.font;
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
    let mut builder = InlineBuilder::new();
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
    TableTextRecovery::Complete(builder.finish())
}

#[cfg(test)]
mod tests {
    use mant_ir::Inline;

    use crate::mandoc::inline::plain_text;

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
                plain_text(&inlines),
                native
                    .filter(|text| !text.is_empty())
                    .unwrap_or(&block.source)
            );
        }
    }

    #[test]
    fn source_requests_dispatch_to_man_and_mdoc_inline_lowering() {
        let man = super::lower_source_fragment_with_font_state(
            ".BR git (1)",
            libmandoc_rs::MacroSet::Man,
            None,
            false,
            crate::mandoc::inline::FontState::new(),
        )
        .unwrap()
        .inlines;
        assert_eq!(plain_text(&man), "git(1)");

        let mdoc = super::lower_source_fragment_with_font_state(
            ".Xr git 1 ,",
            libmandoc_rs::MacroSet::Mdoc,
            None,
            false,
            crate::mandoc::inline::FontState::new(),
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
