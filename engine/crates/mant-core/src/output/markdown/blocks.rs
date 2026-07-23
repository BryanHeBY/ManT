//! Maps native block nodes to portable `CommonMark` block constructs.

use mant_ast::{Block, DefinitionItem, ListItem, ListKind, TableCell, TableRow};

use super::MarkdownOptions;
use super::inline::{code_span, escape_text, fenced_code, flatten_inline, render_inline};

pub(super) fn render_blocks(blocks: &[Block], options: MarkdownOptions) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| render_block(block, options))
        .collect()
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
        Block::Unsupported { name, text, .. } => {
            let text = escape_text(text.trim());
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
                .join("  \n");
            let description = render_blocks(&item.description, options).join("\n\n");
            let content = match (terms.is_empty(), description.is_empty()) {
                (false, false) => {
                    let sep = if is_bullet_term(&terms) { " " } else { "\n" };
                    format!("{terms}{sep}{description}")
                }
                (false, true) => terms,
                (true, false) => description,
                (true, true) => return None,
            };
            prefix_item(&content, "- ").map(|content| (content, item.spacing_before_lines))
        })
        .collect::<Vec<_>>();
    join_definition_items(rendered, compact)
}

/// A single non-word character used as a man(7) bullet (`\u{2022}`, `-`,
/// `*`, etc.) should be rendered inline with its description rather than
/// on a separate term line.
fn is_bullet_term(terms: &str) -> bool {
    let stripped = terms.trim();
    !stripped.is_empty() && stripped.chars().all(|c| !c.is_alphanumeric())
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
    let rows = rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(plain_cell)
                .collect::<Vec<_>>()
                .join(" | ")
        })
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
        Block::Table { rows, .. } => nonempty(
            rows.iter()
                .map(|row| {
                    row.cells
                        .iter()
                        .map(plain_cell)
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
            nonempty(value.trim().to_owned())
        }
        Block::VerticalSpace { .. } => None,
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
