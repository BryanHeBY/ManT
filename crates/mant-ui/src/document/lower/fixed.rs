//! Fixed-surface style and zero-width anchor lookup over logical content.

use super::super::{Block, HashMap, Inline, Modifier, Style, theme};
use mant_ir::{ContentContext, ContentPointKey};

pub(super) fn collect_inline_anchors(
    nodes: &[Inline],
    anchors: &mut HashMap<ContentPointKey, Vec<String>>,
) {
    for node in nodes {
        match node {
            Inline::Anchor {
                point,
                id,
                fragment_aliases,
            } => {
                let ids = anchors.entry(*point).or_default();
                ids.push(id.to_string());
                ids.extend(
                    fragment_aliases
                        .iter()
                        .map(|alias| alias.as_str().to_owned()),
                );
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect_inline_anchors(children, anchors),
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak { .. } => {}
        }
    }
}

pub(super) fn atom_style(content: ContentContext<'_>, atom: &mant_ir::ContentAtom) -> Style {
    let mut style = Style::default().fg(theme::TEXT);
    if atom.style.strong {
        style = style.fg(theme::STRONG).add_modifier(Modifier::BOLD);
    }
    if atom.style.emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if atom.style.literal {
        style = style.fg(theme::HEADING);
    }
    if atom.style.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if let Some(link) = atom.link {
        let external = content.occurrence(link).is_some_and(|occurrence| {
            matches!(
                &occurrence.target,
                mant_ir::LinkTarget::External { .. } | mant_ir::LinkTarget::Email { .. }
            )
        });
        style = style
            .fg(if external { theme::BLUE } else { theme::LINK })
            .add_modifier(Modifier::UNDERLINED);
    }
    style
}

pub(super) fn collect_block_anchors(
    blocks: &[Block],
    anchors: &mut HashMap<ContentPointKey, Vec<String>>,
) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. }
            | Block::Preformatted { children, .. }
            | Block::FixedDisplay { children, .. } => {
                collect_inline_anchors(children, anchors);
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_block_anchors(&item.blocks, anchors);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &item.terms {
                        collect_inline_anchors(term, anchors);
                    }
                    collect_block_anchors(&item.description, anchors);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_block_anchors(&cell.blocks, anchors);
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}
