//! Plain document bodies: apply source layout without report selection policy.

use super::{blocks, document_label, flow};
use crate::presentation::{EntryStyleMap, TextPresentation, TextRole};
use mant_ir::{Block, ResolvedContent, TldrCommandPart, TldrDocument, TldrOrigin};

pub(in crate::output) fn render_located_blocks<'a>(
    blocks: &'a [Block],
    locations: &'a crate::output::styles::LocatedStyles<'a>,
    decorate: &'a dyn Fn(TextPresentation, &str) -> String,
) -> String {
    blocks::BlockRenderer {
        names: None,
        locations: Some(locations),
        decorate,
    }
    .render_blocks(blocks, 0)
}

/// Render a complete query without Markdown or terminal escape sequences.
#[must_use]
pub fn render_query_text(query: &ResolvedContent) -> String {
    render_query_body(query, true)
}

/// Render source-aware text spans without recovering roles from rendered lines.
/// The decorator must preserve visible text, newlines and boundary whitespace;
/// it may add zero-width styles or replace unsafe scalars one-for-one.
#[must_use]
pub fn render_query_text_with(
    query: &ResolvedContent,
    decorate: impl Fn(TextPresentation, &str) -> String,
) -> String {
    render_query_body_with(query, true, &decorate, true)
}

/// Render the manual as `man(1)`-faithful plain text.
///
/// Identical to [`render_query_text`] except the prepended tldr block is
/// omitted, so the output stays a faithful, noise-free subset of the manual
/// page (no page furniture, overstrike, or hyphenation — those never enter
/// the document model because the source is parsed directly).
#[must_use]
pub fn render_query_man(query: &ResolvedContent) -> String {
    if query.document.is_none() {
        return String::new();
    }
    render_query_body(query, false)
}

fn render_query_body(query: &ResolvedContent, include_tldr: bool) -> String {
    render_query_body_with(query, include_tldr, &|_, text| text.to_owned(), false)
}

fn render_query_body_with(
    query: &ResolvedContent,
    include_tldr: bool,
    decorate: &dyn Fn(TextPresentation, &str) -> String,
    styled: bool,
) -> String {
    let section = query
        .document
        .as_ref()
        .and_then(|document| document.meta.manual_section.as_deref());
    let title = query
        .document
        .as_ref()
        .and_then(|document| document.heading.as_ref())
        .map_or_else(
            || {
                decorate(
                    TextRole::Document.into(),
                    &document_label(&query.label, section),
                )
            },
            |heading| {
                blocks::BlockRenderer {
                    names: None,
                    decorate,
                    locations: None,
                }
                .inline_text(&heading.content, TextRole::Document)
            },
        );
    let mut output = flow::Flow::text(title);
    if include_tldr && let Some(tldr) = &query.tldr {
        output.gap(1);
        output.push_text(render_tldr_text(tldr));
    }
    if let Some(document) = &query.document {
        let renderer = blocks::BlockRenderer {
            names: styled.then(|| EntryStyleMap::for_document(document)),
            decorate,
            locations: None,
        };
        let mut content = renderer.block_flow(&document.blocks, 0);
        content.extend(renderer.sections_flow(&document.sections, 0));
        if !content.is_empty() {
            // Page furniture has one explicit presentation separator. All
            // source-owned section/block gaps remain in the same flow.
            output.gap(1);
            output.extend(content);
        }
    }
    output.finish(false)
}

pub(super) fn render_tldr_text(tldr: &TldrDocument) -> String {
    let mut lines = vec!["TLDR".to_owned()];
    lines.extend(tldr.description.iter().map(|line| line.trim().to_owned()));
    if let Some(information) = &tldr.more_information {
        lines.push(format!("More information: {}", information.trim()));
    }
    for example in &tldr.examples {
        if !example.description.trim().is_empty() {
            lines.push(example.description.trim().to_owned());
        }
        let command = example
            .command_parts
            .iter()
            .map(|part| match part {
                TldrCommandPart::Text { value } | TldrCommandPart::Placeholder { value } => {
                    value.as_str()
                }
            })
            .collect::<String>();
        lines.push(if command.is_empty() {
            example.command.clone()
        } else {
            command
        });
    }
    if tldr.origin == TldrOrigin::TldrPages {
        lines.push(format!(
            "tldr-pages · CC BY 4.0 · {} · {}",
            tldr.platform, tldr.language
        ));
    }
    lines.join("\n\n")
}
