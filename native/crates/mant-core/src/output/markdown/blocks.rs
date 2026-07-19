//! Maps native block nodes to portable `CommonMark` block constructs.

use mant_ast::{Block, DefinitionItem, ListItem, ListKind, TableCell, TableRow};

use super::inline::{code_span, escape_text, fenced_code, flatten_inline, render_inline};

pub(super) fn render_blocks(blocks: &[Block]) -> Vec<String> {
    blocks.iter().filter_map(render_block).collect()
}

fn render_block(block: &Block) -> Option<String> {
    match block {
        Block::Paragraph { children, .. } => nonempty(render_inline(children)),
        Block::Preformatted {
            children, language, ..
        } => Some(fenced_code(&flatten_inline(children), language.as_deref())),
        Block::List {
            kind,
            start,
            compact,
            items,
            ..
        } => render_list(*kind, *start, *compact, items),
        Block::DefinitionList { items, compact, .. } => render_definition_list(items, *compact),
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
            prefix_item(&render_blocks(&item.blocks).join("\n\n"), &marker)
        })
        .collect::<Vec<_>>();
    (!rendered.is_empty()).then(|| rendered.join(if compact { "\n" } else { "\n\n" }))
}

fn render_definition_list(items: &[DefinitionItem], compact: bool) -> Option<String> {
    let rendered = items
        .iter()
        .filter_map(|item| {
            let terms = item
                .terms
                .iter()
                .map(|term| render_inline(term))
                .filter(|term| !term.is_empty())
                .collect::<Vec<_>>()
                .join("  \n");
            let description = render_blocks(&item.description).join("\n\n");
            let content = match (terms.is_empty(), description.is_empty()) {
                (false, false) => format!("{terms}\n\n{description}"),
                (false, true) => terms,
                (true, false) => description,
                (true, true) => return None,
            };
            prefix_item(&content, "- ")
        })
        .collect::<Vec<_>>();
    (!rendered.is_empty()).then(|| rendered.join(if compact { "\n" } else { "\n\n" }))
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
