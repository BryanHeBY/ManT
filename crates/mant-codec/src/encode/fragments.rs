//! Safe report-facing fragments, without document-wide semantic declarations.

use mant_ir::{Block, Heading, Inline, Section};

use super::{MarkdownInlineProjection, MarkdownOptions};

/// Presentation controls for detached report fragments.
///
/// Semantic declarations require whole-document representability validation and
/// are deliberately unavailable here. Arbitrary valid IR fragments retain their
/// visible content even when they cannot be reimported as semantic Markdown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkdownFragmentOptions {
    /// Preserve document-local destinations and typed links in the fragment.
    pub preserve_anchors: bool,
}

impl MarkdownFragmentOptions {
    fn document_options(self) -> MarkdownOptions {
        MarkdownOptions {
            preserve_anchors: self.preserve_anchors,
            preserve_semantics: false,
        }
    }
}

/// Encode a detached inline root without semantic declaration metadata.
#[must_use]
pub fn render_inline_fragment(children: &[Inline], options: MarkdownFragmentOptions) -> String {
    super::inline::render_inline(children, options.document_options())
}

/// Encode detached blocks without semantic declaration metadata.
#[must_use]
pub fn render_blocks_fragment(blocks: &[Block], options: MarkdownFragmentOptions) -> Vec<String> {
    super::blocks::render_blocks(blocks, options.document_options())
}

/// Encode report blocks with optional decoration of their original inline roots.
///
/// Unmodified roots may remain borrowed. Decoration does not alter canonical
/// document artifacts or their source-coordinate maps.
#[must_use]
pub fn render_located_blocks_fragment(
    blocks: &[Block],
    options: MarkdownFragmentOptions,
    projection: Option<&dyn MarkdownInlineProjection>,
) -> Vec<String> {
    super::blocks::render_located_blocks(blocks, options.document_options(), projection)
}

/// Append a detached section forest as Markdown blocks, starting at `depth`.
pub fn render_sections_fragment(
    output: &mut Vec<String>,
    sections: &[Section],
    depth: usize,
    options: MarkdownFragmentOptions,
) {
    super::render_sections(output, sections, depth, options.document_options());
}

/// Encode a rich heading without semantic declaration metadata.
#[must_use]
pub fn render_heading_fragment(
    depth: usize,
    heading: &Heading,
    options: MarkdownFragmentOptions,
) -> String {
    super::render_heading(depth, heading, options.document_options())
}

/// Encode literal text as a `CommonMark` code span with safe delimiters.
#[must_use]
pub fn commonmark_code_span(value: &str) -> String {
    super::inline::code_span(value)
}

/// Escape plain text for the encoder's supported `CommonMark` syntax.
#[must_use]
pub fn escape_commonmark(value: &str) -> String {
    super::inline::escape_text(value)
}

/// Encode one raw-HTML destination with an escaped attribute value.
#[must_use]
pub fn html_anchor(id: &str) -> String {
    super::inline::html_anchor(id)
}
