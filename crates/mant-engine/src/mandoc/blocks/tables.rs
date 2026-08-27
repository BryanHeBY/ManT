//! Reconstructs native tbl rows and source-backed semantic cell content.

use mant_ir::{
    Block, Inline, LayoutHint, TableAlignment as AstTableAlignment, TableCell as AstTableCell,
    TableRow,
};

use super::super::{
    LoweringContext, TableTextBlock,
    inline::{
        FilledBoundary, InlineBuilder, lower_inline_nodes_with_spacing, lower_man_link,
        lower_source_alternating_fonts, lower_source_mdoc_request, parse_roff_text, plain_text,
    },
    layout::layout,
    roff_escape::visible_text,
    source_span,
    syntax::{
        Node, NodeKind, TableAlignment as MandocTableAlignment, TableCell as MandocTableCell,
    },
};

pub(super) struct TableEmbedding<'a> {
    blocks: Vec<TableTextBlock>,
    nodes: Vec<&'a Node>,
}

pub(super) fn table_embeddings<'a>(
    nodes: &'a [Node],
    context: &LoweringContext<'_>,
) -> (Vec<Option<TableEmbedding<'a>>>, Vec<bool>) {
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
    (embeddings, consumed)
}

pub(super) fn append_table_row(
    output: &mut Vec<Block>,
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    embedding: Option<&TableEmbedding<'_>>,
) {
    if node.table_cells.is_empty() {
        return;
    }
    let mut text_block_index = 0;
    let source_cells = context.tab_separated_table_cells(node.line);
    let cell_count = node
        .table_cells
        .len()
        .max(source_cells.as_ref().map_or(0, Vec::len));
    let row = TableRow {
        cells: (0..cell_count)
            .map(|index| {
                let cell = node.table_cells.get(index);
                let vertical_continuation = cell.is_some_and(|cell| cell.vertical_continuation);
                let text_block = if cell.is_some_and(|cell| cell.text_block) {
                    let block =
                        embedding.and_then(|embedding| embedding.blocks.get(text_block_index));
                    text_block_index += 1;
                    block
                } else {
                    None
                };
                let raw_source = source_cells
                    .as_ref()
                    .and_then(|cells| cells.get(index))
                    .copied();
                let blocks = if vertical_continuation {
                    // `\^` is tbl's vertical-span control marker. The
                    // preceding cell owns the actual content and its copied
                    // `row_span`; rendering the marker as text would invent
                    // a visible token that groff and mandoc both suppress.
                    Vec::new()
                } else {
                    let children = cell.map_or_else(
                        || lower_missing_table_cell(raw_source, node, context),
                        |cell| {
                            let lowered = lower_table_cell(
                                cell,
                                index,
                                &node.table_cells,
                                node,
                                context,
                                text_block,
                                embedding.map_or(&[], |embedding| embedding.nodes.as_slice()),
                            );
                            if lowered.is_empty()
                                && raw_source.is_some_and(|source| !source.is_empty())
                            {
                                lower_missing_table_cell(raw_source, node, context)
                            } else {
                                lowered
                            }
                        },
                    );
                    vec![Block::Paragraph {
                        children,
                        layout: LayoutHint::default(),
                        source: source_span(node),
                    }]
                };
                AstTableCell {
                    blocks,
                    column_span: cell.map_or(1, |cell| cell.column_span),
                    row_span: cell.map_or(1, |cell| cell.row_span),
                    alignment: Some(match cell.map(|cell| cell.alignment) {
                        None | Some(MandocTableAlignment::Left) => AstTableAlignment::Left,
                        Some(MandocTableAlignment::Center) => AstTableAlignment::Center,
                        Some(MandocTableAlignment::Right) => AstTableAlignment::Right,
                    }),
                }
            })
            .collect(),
    };
    if let Some(Block::Table { rows, .. }) = output.last_mut() {
        rows.push(row);
    } else {
        output.push(Block::Table {
            rows: vec![row],
            layout: layout(indent_columns),
            source: source_span(node),
        });
    }
}

fn lower_missing_table_cell(
    source: Option<&str>,
    node: &Node,
    context: &LoweringContext<'_>,
) -> Vec<Inline> {
    let source = source.unwrap_or_default().trim();
    if source.is_empty() {
        return Vec::new();
    }
    let lowered = lower_table_cell_text(source, node.line, context);
    if !lowered.is_empty() {
        return lowered;
    }
    context.warn_unexpanded_table_cell(node.line);
    vec![Inline::Code {
        value: source.to_owned(),
    }]
}

fn lower_table_cell(
    cell: &MandocTableCell,
    cell_index: usize,
    row_cells: &[MandocTableCell],
    node: &Node,
    context: &LoweringContext<'_>,
    text_block: Option<&TableTextBlock>,
    semantic_nodes: &[&Node],
) -> Vec<Inline> {
    if let Some(text_block) = text_block {
        let semantic_nodes = semantic_nodes
            .iter()
            .copied()
            .filter(|candidate| text_block.contains_line(candidate.line))
            .collect::<Vec<_>>();
        // A tbl `T{ ... T}` cell retains its source requests, while the
        // flattened libmandoc cell text has already discarded request-level
        // font and spacing semantics. Reconstruct from the bounded source
        // block first even when no printable AST siblings escaped the table.
        let reconstructed = lower_table_text_block(text_block, &semantic_nodes, context);
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
            // Request-level reconstruction owns styles and links that tbl's
            // flattened cell text cannot represent.  Prefer it even when a
            // loose text-containment check happens to match another cell
            // across a word boundary (for example `using` + `perl` matching
            // the first column's `gperl`).
            let reconstructed_has_semantics = reconstructed
                .iter()
                .any(|inline| !matches!(inline, Inline::Text { .. }));
            if agrees_with_current || !belongs_to_other_cell || reconstructed_has_semantics {
                return reconstructed;
            }
        }
    }
    if cell.text.as_deref().is_some_and(|text| !text.is_empty()) {
        return lower_table_cell_text(cell.text.as_deref().unwrap_or_default(), node.line, context);
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
                Some(parse_roff_text(argument))
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

fn lower_table_cell_text(source: &str, line: u32, context: &LoweringContext<'_>) -> Vec<Inline> {
    let Some((opening, closing)) = context.equation_delimiters_at(line) else {
        return parse_roff_text(source);
    };
    let mut output = Vec::new();
    let mut remainder = source;
    while let Some(opening_index) = remainder.find(opening) {
        let after_opening = &remainder[opening_index + opening.len_utf8()..];
        let Some(closing_index) = after_opening.find(closing) else {
            break;
        };
        output.extend(parse_roff_text(&remainder[..opening_index]));
        let expression = &after_opening[..closing_index];
        if !expression.trim().is_empty() {
            output.push(Inline::Code {
                value: context.normalize_equation(expression, line),
            });
        }
        remainder = &after_opening[closing_index + closing.len_utf8()..];
    }
    output.extend(parse_roff_text(remainder));
    output
}

fn lower_table_text_block(
    block: &TableTextBlock,
    semantic_nodes: &[&Node],
    context: &LoweringContext<'_>,
) -> Vec<Inline> {
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
            if let Some(inline) = source_table_inline(source_line.trim(), context.default_name) {
                if requires_legacy_table_warning(source_line) {
                    context.warn_unhandled_table_text_block_line(line);
                }
                builder.append_filled(inline, FilledBoundary::Word);
                continue;
            }
            for node in nodes {
                let spacing_enabled = builder.spacing_enabled();
                let lowered = if matches!(node.macro_name.as_deref(), Some("UR" | "MT")) {
                    lower_man_link(node, context.default_name, spacing_enabled)
                } else {
                    lower_inline_nodes_with_spacing(
                        std::slice::from_ref(node),
                        context.default_name,
                        spacing_enabled,
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
        if let Some(inline) = source_table_inline(source_line, context.default_name) {
            if requires_legacy_table_warning(source_line) {
                context.warn_unhandled_table_text_block_line(line);
            }
            builder.append_filled(inline, FilledBoundary::Word);
        } else if source_line.starts_with('.') || source_line.starts_with('\'') {
            context.warn_unhandled_table_text_block_line(line);
        } else {
            builder.append_filled(parse_roff_text(source_line), FilledBoundary::Word);
        }
    }
    builder.finish()
}

fn requires_legacy_table_warning(source_line: &str) -> bool {
    let Some(request) = source_line.trim_start().strip_prefix(['.', '\'']) else {
        return false;
    };
    let name = request
        .split_once(char::is_whitespace)
        .map_or(request, |(name, _)| name);
    // The native source recovery understands `.SM`, but the legacy engine
    // deliberately reported this tbl request as semantically unretained.
    // Retain that diagnostic in its compatibility IR without throwing away
    // the now-recoverable text.
    name == "SM"
}

fn source_table_inline(source_line: &str, default_name: Option<&str>) -> Option<Vec<Inline>> {
    let request = source_line.strip_prefix(['.', '\''])?;
    let (name, rest) = request
        .split_once(char::is_whitespace)
        .unwrap_or((request, ""));
    let argument = rest.trim();
    if matches!(name, "BI" | "BR" | "IB" | "IR" | "RB" | "RI") {
        return lower_source_alternating_fonts(name, argument);
    }
    if name == "SM" {
        return Some(parse_roff_text(argument));
    }
    // The widely used groff fallback `.MR name section [punctuation]` is a
    // source-defined convenience macro. tbl preserves the call but not its
    // definition body, so recover its stable manual-reference semantics here.
    if name == "MR" {
        return lower_source_mdoc_request("Xr", argument, default_name);
    }
    lower_source_mdoc_request(name, argument, default_name)
}

#[cfg(test)]
mod tests {
    use mant_ir::Inline;

    use crate::mandoc::inline::plain_text;

    #[test]
    fn source_requests_dispatch_to_man_and_mdoc_inline_lowering() {
        let man =
            super::source_table_inline(".BR git (1)", None).expect("recognized man source request");
        assert_eq!(plain_text(&man), "git(1)");

        let mdoc = super::source_table_inline(".Xr git 1 ,", None)
            .expect("recognized mdoc source request");
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

    #[test]
    fn keeps_the_legacy_tbl_warning_for_recovered_small_text() {
        assert!(super::requires_legacy_table_warning(".SM value"));
        assert!(super::requires_legacy_table_warning("  'SM value"));
        assert!(!super::requires_legacy_table_warning(".BR value (1)"));
        assert!(!super::requires_legacy_table_warning("value"));
    }
}
