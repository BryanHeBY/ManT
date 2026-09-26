//! Materialized excerpt furniture and breadcrumbs around original source bodies.

use super::{blocks, body::render_tldr_text, document_label};
use crate::presentation::{EntryStyleMap, TextPresentation, TextRole};
use mant_ir::ContentContext;
use mant_protocol::{ExcerptSelection, QueryExcerpt};

/// Render selected query nodes as unstyled text with outline context.
#[must_use]
pub fn render_excerpt_text(excerpt: &QueryExcerpt) -> String {
    render_excerpt_with(excerpt, &|_, text| text.to_owned(), false)
}

/// Render materialized selections with source roles and validated name ranges.
/// The decoration contract is the same as [`super::render_query_text_with`].
#[must_use]
pub fn render_excerpt_text_with(
    excerpt: &QueryExcerpt,
    decorate: impl Fn(TextPresentation, &str) -> String,
) -> String {
    render_excerpt_with(excerpt, &decorate, true)
}

fn render_excerpt_with(
    excerpt: &QueryExcerpt,
    decorate: &dyn Fn(TextPresentation, &str) -> String,
    styled: bool,
) -> String {
    let mut parts = vec![decorate(
        TextRole::Document.into(),
        &document_label(
            excerpt.display_title.as_deref().unwrap_or(&excerpt.label),
            excerpt
                .meta
                .as_ref()
                .and_then(|meta| meta.manual_section.as_deref()),
        ),
    )];
    if !excerpt.semantics_complete {
        parts.push("Semantic entries are incomplete; use search to inspect unclassified or rejected content.".to_owned());
    }
    for selection in &excerpt.selections {
        parts.push(render_selection(
            selection,
            excerpt
                .content_projection
                .as_ref()
                .map(mant_ir::ContentProjection::content),
            decorate,
            styled,
        ));
    }
    join_parts(parts)
}

fn render_selection<'a>(
    selection: &'a ExcerptSelection,
    content: Option<ContentContext<'a>>,
    decorate: &dyn Fn(TextPresentation, &str) -> String,
    styled: bool,
) -> String {
    let breadcrumb = decorate(
        TextRole::Heading.into(),
        &render_outline_trail(selection.outline()),
    );
    match selection {
        ExcerptSelection::Tldr { document, .. } => {
            join_parts(vec![breadcrumb, render_tldr_text(document)])
        }
        ExcerptSelection::FixedDocumentRoot { view, .. }
        | ExcerptSelection::FixedDocumentSection { view, .. }
        | ExcerptSelection::FixedDocumentEntry { view, .. } => join_parts(vec![
            breadcrumb,
            crate::output::fixed_excerpt::render_fixed_selection(view, decorate),
        ]),
        selection => {
            let content = content.expect("validated retained excerpt has a content projection");
            let names = styled.then(|| match selection {
                ExcerptSelection::DocumentRoot { blocks, .. } => {
                    EntryStyleMap::for_blocks(content, blocks)
                }
                ExcerptSelection::DocumentSection { section, .. } => {
                    EntryStyleMap::for_section(content, section)
                }
                ExcerptSelection::DocumentEntry { entry, .. } => {
                    EntryStyleMap::for_blocks(content, std::slice::from_ref(entry))
                }
                ExcerptSelection::Tldr { .. }
                | ExcerptSelection::FixedDocumentRoot { .. }
                | ExcerptSelection::FixedDocumentSection { .. }
                | ExcerptSelection::FixedDocumentEntry { .. } => unreachable!(),
            });
            let renderer = blocks::BlockRenderer {
                content,
                names,
                decorate,
                locations: None,
            };
            match selection {
                ExcerptSelection::DocumentRoot {
                    heading, blocks, ..
                } => join_parts(vec![
                    breadcrumb,
                    heading
                        .as_ref()
                        .map(|heading| renderer.inline_text(&heading.content, TextRole::Heading))
                        .unwrap_or_default(),
                    renderer.render_blocks(blocks, 0),
                ]),
                ExcerptSelection::DocumentSection { section, .. } => {
                    join_parts(vec![breadcrumb, renderer.render_section(section, 0)])
                }
                ExcerptSelection::DocumentEntry { entry, .. } => join_parts(vec![
                    breadcrumb,
                    renderer.render_blocks(std::slice::from_ref(entry), 0),
                ]),
                ExcerptSelection::Tldr { .. }
                | ExcerptSelection::FixedDocumentRoot { .. }
                | ExcerptSelection::FixedDocumentSection { .. }
                | ExcerptSelection::FixedDocumentEntry { .. } => unreachable!(),
            }
        }
    }
}

fn render_outline_trail(trail: &mant_protocol::OutlineTrail) -> String {
    let breadcrumb = trail
        .ancestors
        .iter()
        .map(|ancestor| ancestor.title.as_str())
        .chain(std::iter::once(trail.title()))
        .collect::<Vec<_>>()
        .join(" > ");
    format!("Outline {}: {breadcrumb}", trail.path())
}

// Excerpt metadata panels have a presentation separator. Their rendered
// source bodies are opaque here: never trim literal rows or resolved gaps.
fn join_parts(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
