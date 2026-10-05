//! Encodes already-loaded, source-neutral IR as deterministic portable `CommonMark`.

mod anchors;
mod artifact;
mod blocks;
mod fragments;
mod inline;
mod mapped;
mod semantic;
mod table_projection;

use std::borrow::Cow;

use mant_ir::{Section, TldrCommandPart, TldrDocument, TldrOrigin};

use self::{blocks::render_blocks, inline::escape_text};
use crate::ResolvedContent;
use artifact::render_markdown_artifact;
pub use artifact::{
    MarkdownArtifact, MarkdownNode, MarkdownNodeRange, MarkdownSection,
    render_addressable_markdown, render_addressable_markdown_with_options,
};
pub use fragments::{
    MarkdownFragmentOptions, commonmark_code_span, escape_commonmark, html_anchor,
    render_blocks_fragment, render_heading_fragment, render_inline_content_fragment,
    render_inline_fragment, render_located_blocks_fragment, render_sections_fragment,
};

/// Optional report decoration expressed only as source-neutral inline IR.
///
/// Document artifacts never accept this projection: their coordinates always
/// describe the canonical bytes. Report renderers may project one root at a
/// time, borrowing untouched roots and allocating only decorated roots.
pub trait MarkdownInlineProjection {
    /// Project one original inline root, borrowing it when no decoration is needed.
    fn project<'a>(&self, nodes: &'a [mant_ir::Inline]) -> Cow<'a, [mant_ir::Inline]>;
}

/// Markdown serialization controls that do not alter the query IR.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkdownOptions {
    /// Emit stable raw-HTML destinations and links for document-local references.
    pub preserve_anchors: bool,
    /// Emit nonvisible declarations for the supported ordinary-list subset.
    /// Unsupported documents retain portable content without semantic comments;
    /// this is not a lossless serialization (use IR JSON for that).
    pub preserve_semantics: bool,
}

impl MarkdownOptions {
    /// Addressable Markdown used by consumers of `mant.markdown/v1`.
    pub const ADDRESSABLE: Self = Self {
        preserve_anchors: true,
        preserve_semantics: false,
    };
}

/// Render a complete query as clean Markdown without a trailing newline.
#[must_use]
pub fn render_markdown(query: &ResolvedContent) -> String {
    render_markdown_with_options(query, MarkdownOptions::default())
}

/// Render a complete query using explicit presentation-only options.
#[must_use]
pub fn render_markdown_with_options(query: &ResolvedContent, options: MarkdownOptions) -> String {
    render_markdown_artifact(query, options, false).into_text()
}

pub(super) fn render_sections(
    output: &mut Vec<String>,
    sections: &[Section],
    depth: usize,
    options: MarkdownOptions,
) {
    for section in sections {
        if options.preserve_anchors {
            output.push(format!(
                "{}\n\n{}",
                inline::html_anchors(&section.id, &section.fragment_aliases),
                render_heading(depth, &section.heading, options)
            ));
        } else {
            output.push(render_heading(depth, &section.heading, options));
        }
        output.extend(render_blocks(&section.blocks, options));
        render_sections(output, &section.children, depth.saturating_add(1), options);
    }
}

/// Encode a quick-reference page as independently joinable Markdown blocks.
#[must_use]
pub fn render_tldr(page: &TldrDocument) -> Vec<String> {
    let mut output = vec![heading(2, "TLDR")];
    output.extend(
        page.description
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| escape_text(line.trim())),
    );

    if let Some(value) = page.more_information.as_deref() {
        output.push(render_more_information(value));
    }
    if !page.examples.is_empty() {
        output.push(heading(3, "Examples"));
        for example in &page.examples {
            if !example.description.trim().is_empty() {
                output.push(format!("**{}**", escape_text(example.description.trim())));
            }
            if !example.command.is_empty() {
                let resolved = example
                    .command_parts
                    .iter()
                    .map(|part| match part {
                        TldrCommandPart::Text { value }
                        | TldrCommandPart::Placeholder { value } => value.as_str(),
                    })
                    .collect::<String>();
                output.push(inline::fenced_code(
                    if resolved.is_empty() {
                        &example.command
                    } else {
                        &resolved
                    },
                    Some("sh"),
                ));
            }
        }
    }
    if page.origin == TldrOrigin::TldrPages {
        output.push(format!(
            "*tldr-pages · CC BY 4.0 · {} · {}*",
            escape_text(&page.platform),
            escape_text(&page.language)
        ));
    }
    output
}

fn render_more_information(value: &str) -> String {
    let value = value.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        let (url, punctuation) = value
            .strip_suffix('.')
            .map_or((value, ""), |url| (url, "."));
        if !url.chars().any(char::is_whitespace) && !url.contains(['<', '>']) {
            return format!("**More information:** <{url}>{punctuation}");
        }
    }
    format!("**More information:** {}", escape_text(value))
}

/// Encode a plain heading, clamping its level to the `CommonMark` range 1–6.
#[must_use]
pub fn heading(depth: usize, title: &str) -> String {
    format!("{} {}", "#".repeat(depth.clamp(1, 6)), escape_text(title))
}

/// A visible heading must not silently lose a local target merely because
/// ordinary portable body export omits optional raw-HTML destinations.
#[must_use]
pub fn heading_has_local_link(heading: &mant_ir::Heading) -> bool {
    fn inlines_have_local_link(content: &[mant_ir::Inline]) -> bool {
        content.iter().any(|inline| match inline {
            mant_ir::Inline::Link {
                target: mant_ir::LinkTarget::Section { .. },
                ..
            } => true,
            mant_ir::Inline::Link { children, .. }
            | mant_ir::Inline::Strong { children }
            | mant_ir::Inline::Emphasis { children } => inlines_have_local_link(children),
            mant_ir::Inline::Text { .. }
            | mant_ir::Inline::Code { .. }
            | mant_ir::Inline::Equation { .. }
            | mant_ir::Inline::Anchor { .. }
            | mant_ir::Inline::LineBreak { .. } => false,
        })
    }
    inlines_have_local_link(&heading.content)
}

/// Whether any heading in this section forest contains a typed local link.
#[must_use]
pub fn section_headings_have_local_links(sections: &[Section]) -> bool {
    sections.iter().any(|section| {
        heading_has_local_link(&section.heading)
            || section_headings_have_local_links(&section.children)
    })
}

pub(super) fn render_heading(
    depth: usize,
    heading: &mant_ir::Heading,
    options: MarkdownOptions,
) -> String {
    let content = inline::render_heading_content(
        mant_ir::InlineContentRef {
            content: &heading.content,
            layout: &heading.inline_layout,
        },
        options,
    );
    if depth <= 2 && content.contains('\n') {
        // Setext headings are the portable CommonMark form that retains
        // explicit inline breaks; an ATX newline would end the heading.
        return format!("{content}\n{}", if depth == 1 { "===" } else { "---" });
    }
    // CommonMark has no multiline ATX heading. Keep its hierarchy and
    // linked content on one line rather than accidentally emitting body
    // paragraphs; IR/JSON retain the original explicit line breaks.
    format!(
        "{} {}",
        "#".repeat(depth.clamp(1, 6)),
        content.replace("<br>\n", " ").replace("  \n", " ")
    )
}

#[cfg(test)]
mod tests;
