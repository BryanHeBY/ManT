//! A single plain/decorated tree with primary titles and hanging metadata.
mod metadata;
mod references;
use super::text::document_label;
use crate::presentation::{TextPresentation, TextRole, sanitize_terminal_text};
use mant_protocol::{OutlineNode, QueryOutline};
pub(super) use metadata::entry_kind_label;
use metadata::outline_summary;
pub use metadata::{render_outline_entry_summary, render_outline_relationships};

/// Render a complete outline with full titles and separately labeled identities.
#[must_use]
pub fn render_outline_text(outline: &QueryOutline) -> String {
    render_outline_text_with(outline, |_, text| text.to_owned())
}

/// Decorate the same tree without moving, shortening or rediscovering its facts.
/// The callback must preserve visible text and boundary whitespace.
#[must_use]
pub fn render_outline_text_with(
    outline: &QueryOutline,
    decorate: impl Fn(TextPresentation, &str) -> String,
) -> String {
    let paint = |role: TextRole, value: &str| decorate(role.into(), &sanitize_terminal_text(value));
    let mut lines = vec![paint(
        TextRole::Document,
        &document_label(
            &outline.label,
            outline
                .meta
                .as_ref()
                .and_then(|m| m.manual_section.as_deref()),
        ),
    )];
    if let Some(message) = super::outline_empty_message(outline) {
        lines.push(paint(TextRole::Notice, &message));
    } else {
        let badges = references::Badges::build(outline);
        nodes(&outline.nodes, "", &paint, &badges, &mut lines);
    }
    let references =
        crate::presentation::render_reference_inventory_with(&outline.references, &decorate);
    if !references.is_empty() {
        lines.push(references);
    }
    lines.join("\n")
}

fn nodes(
    items: &[OutlineNode],
    prefix: &str,
    paint: &dyn Fn(TextRole, &str) -> String,
    badges: &references::Badges,
    lines: &mut Vec<String>,
) {
    for (index, node) in items.iter().enumerate() {
        let last = index + 1 == items.len();
        let kind = match node {
            OutlineNode::DocumentEntry { entry_kind, .. } => TextRole::EntryLabel(*entry_kind),
            _ => TextRole::Heading,
        };
        lines.push(format!(
            "{} {} {}{}",
            paint(
                TextRole::Guide,
                &format!("{prefix}{}", if last { "└─" } else { "├─" })
            ),
            paint(TextRole::Path, node.path()),
            paint(kind, node.title()),
            badges.get(node).map_or_else(String::new, |badge| format!(
                " {}",
                paint(TextRole::Reference, badge)
            ))
        ));
        let child_prefix = format!("{prefix}{}", if last { "  " } else { "│ " });
        let hang = paint(TextRole::Guide, &format!("{child_prefix}   "));
        lines.push(format!(
            "{hang}{}{}",
            paint(TextRole::Metadata, "ID: "),
            paint(TextRole::Coordinate, node.id())
        ));
        if let Some(summary) = outline_summary(node) {
            let value = render_outline_entry_summary(summary);
            if let Some(value) = value.strip_prefix(" — ") {
                lines.push(format!(
                    "{hang}{}",
                    paint(TextRole::Metadata, &format!("Entries: {value}"))
                ));
            }
        }
        let relationships = render_outline_relationships(node);
        if let Some(value) = relationships.strip_prefix(" — ") {
            lines.push(format!(
                "{hang}{}",
                paint(TextRole::Metadata, &format!("Relationships: {value}"))
            ));
        }
        nodes(node.children(), &child_prefix, paint, badges, lines);
    }
}
