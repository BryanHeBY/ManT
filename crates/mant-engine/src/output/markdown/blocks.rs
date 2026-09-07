//! Maps native block nodes to portable `CommonMark` block constructs.

use mant_ir::{
    Block, DefinitionIdentity, DefinitionItem, ListItem, ListKind, SourceSpan, TableCell, TableRow,
};

use super::MarkdownOptions;
use super::inline::{
    code_span, escape_text, fenced_code, flatten_inline, html_anchor, protect_block_prefix,
    render_inline,
};
use crate::definitions::definition_entries;

pub(super) struct RenderedBlocks {
    pub(super) text: String,
    pub(super) entries: Vec<RenderedEntry>,
}

pub(super) struct RenderedEntry {
    pub(super) indices: Vec<usize>,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) identity: DefinitionIdentity,
    pub(super) source: Option<SourceSpan>,
}

pub(super) fn render_blocks(blocks: &[Block], options: MarkdownOptions) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| render_block(block, options))
        .collect()
}

pub(super) fn render_blocks_with_entries(
    blocks: &[Block],
    options: MarkdownOptions,
) -> RenderedBlocks {
    let text = render_blocks(blocks, options).join("\n\n");
    let mut entries = Vec::new();
    if options.preserve_anchors {
        let markers = super::anchor_markers(&text);
        let mut cursor = 0;
        for located in definition_entries(blocks) {
            let entry = located.item;
            let Some(identity) = &entry.identity else {
                continue;
            };
            let anchor = html_anchor(&identity.id);
            // Located entries follow rendered source order. Skip the consumed
            // prefix rather than rescanning it for every entry of large pages.
            let first = markers.partition_point(|marker| marker.range.start < cursor);
            let Some(marker) = markers[first..]
                .iter()
                .find(|marker| text[marker.range.clone()] == anchor)
            else {
                continue;
            };
            let start = marker.range.start;
            let end = marker.item_end.unwrap_or(marker.range.end);
            entries.push(RenderedEntry {
                indices: located.indices,
                start,
                end,
                identity: identity.clone(),
                source: located.source,
            });
            cursor = start.saturating_add(anchor.len());
        }
    }
    RenderedBlocks { text, entries }
}

fn render_block(block: &Block, options: MarkdownOptions) -> Option<String> {
    match block {
        Block::Paragraph { children, .. } => nonempty(render_inline(children, options)),
        Block::Preformatted {
            children, language, ..
        } => Some(fenced_code(&flatten_inline(children), language.as_deref())),
        Block::List {
            kind,
            start,
            compact,
            items,
            ..
        } => render_list(*kind, *start, *compact, items, options),
        Block::DefinitionList { items, compact, .. } => {
            render_definition_list(items, *compact, options)
        }
        Block::Table { rows, .. } => render_table(rows),
        Block::Equation { value, display, .. } => {
            if *display {
                Some(fenced_code(value, Some("math")))
            } else {
                nonempty(format!("Equation: {}", code_span(value)))
            }
        }
        Block::VerticalSpace { .. } => None,
        Block::ThematicBreak { .. } => Some("---".to_owned()),
        Block::Unsupported { name, text, .. } => {
            let text = escape_text(text.trim())
                .lines()
                .map(protect_block_prefix)
                .collect::<Vec<_>>()
                .join("  \n");
            if text.is_empty() {
                None
            } else {
                Some(name.as_deref().map_or(text.clone(), |name| {
                    format!("**{}:** {text}", escape_text(name))
                }))
            }
        }
    }
}

fn render_list(
    kind: ListKind,
    start: Option<u64>,
    compact: bool,
    items: &[ListItem],
    options: MarkdownOptions,
) -> Option<String> {
    let rendered = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let marker = match kind {
                ListKind::Ordered => format!(
                    "{}. ",
                    start
                        .unwrap_or(1)
                        .saturating_add(u64::try_from(index).unwrap_or(u64::MAX))
                ),
                ListKind::Bullet | ListKind::Plain => "- ".to_owned(),
            };
            prefix_item(&render_blocks(&item.blocks, options).join("\n\n"), &marker)
        })
        .collect::<Vec<_>>();
    (!rendered.is_empty()).then(|| rendered.join(if compact { "\n" } else { "\n\n" }))
}

fn render_definition_list(
    items: &[DefinitionItem],
    compact: bool,
    options: MarkdownOptions,
) -> Option<String> {
    let rendered = items
        .iter()
        .filter_map(|item| {
            let terms = item
                .terms
                .iter()
                .map(|term| render_inline(term, options))
                .filter(|term| !term.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            let description = render_blocks(&item.description, options).join("\n\n");
            let has_terms = !terms.is_empty();
            let mut content = match (terms.is_empty(), description.is_empty()) {
                (false, false) => {
                    // A definition term can share a physical line only with
                    // inline prose. Gluing a fenced code block, nested list,
                    // table, or display equation to the term produces invalid
                    // CommonMark and changes the block's meaning.
                    let sep = if item.inline_term
                        && matches!(item.description.first(), Some(Block::Paragraph { .. }))
                    {
                        " "
                    } else {
                        "\n"
                    };
                    format!("{terms}{sep}{description}")
                }
                (false, true) => terms,
                (true, false) => description,
                (true, true) => return None,
            };
            // `render_inline` protects block markers visible inside one inline
            // run, but a hanging definition can assemble the marker only when
            // its term and description are joined (`1.` + ` text`). Protect
            // that final first line so a semantic definition cannot reparse as
            // a nested ordered list in the public CommonMark projection.
            if has_terms {
                content = protect_block_prefix(&content);
            }
            prefix_item(&content, "- ").map(|content| (content, item.spacing_before_lines))
        })
        .collect::<Vec<_>>();
    join_definition_items(rendered, compact)
}

/// Preserve a man(7) `.PD` override when one is present, otherwise fall back
/// to the list-wide compactness used by mdoc(7) and HTML inputs.
fn join_definition_items(items: Vec<(String, Option<u16>)>, compact: bool) -> Option<String> {
    let mut items = items.into_iter();
    let (mut output, _) = items.next()?;
    for (item, spacing_before_lines) in items {
        let blank_lines = spacing_before_lines.unwrap_or(u16::from(!compact));
        output.push_str(&"\n".repeat(usize::from(blank_lines) + 1));
        output.push_str(&item);
    }
    Some(output)
}

fn render_table(rows: &[TableRow]) -> Option<String> {
    let rows = super::super::table::table_rows(rows, plain_cell)
        .into_iter()
        .filter(|row| !row.trim().is_empty())
        .collect::<Vec<_>>();
    (!rows.is_empty()).then(|| fenced_code(&rows.join("\n"), None))
}

fn plain_cell(cell: &TableCell) -> String {
    cell.blocks
        .iter()
        .filter_map(plain_block)
        .collect::<Vec<_>>()
        .join("; ")
}

fn plain_block(block: &Block) -> Option<String> {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            nonempty(flatten_inline(children).trim().to_owned())
        }
        Block::List { items, .. } => nonempty(
            items
                .iter()
                .flat_map(|item| item.blocks.iter())
                .filter_map(plain_block)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        Block::DefinitionList { items, .. } => nonempty(
            items
                .iter()
                .map(|item| {
                    let terms = item
                        .terms
                        .iter()
                        .map(|term| flatten_inline(term))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let description = item
                        .description
                        .iter()
                        .filter_map(plain_block)
                        .collect::<Vec<_>>()
                        .join("; ");
                    format!("{terms}: {description}")
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        Block::Table { rows, .. } => {
            nonempty(super::super::table::table_rows(rows, plain_cell).join("; "))
        }
        Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
            nonempty(value.trim().to_owned())
        }
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => None,
    }
}

fn prefix_item(content: &str, marker: &str) -> Option<String> {
    if content.trim().is_empty() {
        return None;
    }
    let continuation = " ".repeat(marker.chars().count());
    let mut lines = content.lines();
    let first = lines.next()?;
    let mut output = format!("{marker}{first}");
    for line in lines {
        output.push('\n');
        if !line.is_empty() {
            output.push_str(&continuation);
            output.push_str(line);
        }
    }
    Some(output)
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}
