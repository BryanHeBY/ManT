//! Safe report-facing fragments, without document-wide semantic declarations.

use mant_ir::{Block, Heading, Inline, InlineContentRef, Section};

use super::math_markdown::MathContext;
use super::math_markdown::RenderOptions;
use super::{MarkdownInlineProjection, MarkdownMath, MarkdownOptions};

/// Presentation controls for detached report fragments.
///
/// Semantic declarations require whole-document representability validation and
/// are deliberately unavailable here. Arbitrary valid IR fragments retain their
/// visible content even when they cannot be reimported as semantic Markdown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkdownFragmentOptions {
    /// Mathematical representation shared with full-document exports.
    pub math: MarkdownMath,
    /// Preserve document-local destinations and typed links in the fragment.
    pub preserve_anchors: bool,
}

impl MarkdownFragmentOptions {
    fn document_options(self, math: &MathContext) -> RenderOptions<'_> {
        let options = MarkdownOptions {
            math: self.math,
            preserve_anchors: self.preserve_anchors,
            preserve_semantics: false,
        };
        RenderOptions {
            options,
            math: (self.math == MarkdownMath::Latex).then_some(math),
        }
    }
}

/// Encode a detached inline root without semantic declaration metadata.
#[must_use]
pub fn render_inline_fragment(children: &[Inline], options: MarkdownFragmentOptions) -> String {
    let math = MathContext::export(true);
    super::inline::render_inline(children, options.document_options(&math))
}

/// Encode a detached owner root as ordinary Markdown phrasing.
/// Reading layout hints stay on the owner without generating text cells.
#[must_use]
pub fn render_inline_content_fragment(
    content: InlineContentRef<'_>,
    options: MarkdownFragmentOptions,
) -> String {
    let math = MathContext::export(true);
    super::inline::render_inline_content(content, options.document_options(&math))
}

/// Encode detached blocks without semantic declaration metadata.
/// Adjacent roots may share a fragment to retain their required hard-row seam.
/// Returned fragments can be joined with a blank Markdown source line.
#[must_use]
pub fn render_blocks_fragment(blocks: &[Block], options: MarkdownFragmentOptions) -> Vec<String> {
    let math = MathContext::export(true);
    super::blocks::render_blocks(blocks, options.document_options(&math))
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
    // A projected root can own temporary expression copies. Their addresses
    // remain valid only for that root, so this route avoids pointer memoization.
    let math = MathContext::export(projection.is_none());
    super::blocks::render_located_blocks(blocks, options.document_options(&math), projection)
}

/// Append a detached section forest as Markdown blocks, starting at `depth`.
pub fn render_sections_fragment(
    output: &mut Vec<String>,
    sections: &[Section],
    depth: usize,
    options: MarkdownFragmentOptions,
) {
    let math = MathContext::export(true);
    super::render_sections(output, sections, depth, options.document_options(&math));
}

/// Encode a rich heading without semantic declaration metadata.
#[must_use]
pub fn render_heading_fragment(
    depth: usize,
    heading: &Heading,
    options: MarkdownFragmentOptions,
) -> String {
    let math = MathContext::export(true);
    super::render_heading(depth, heading, options.document_options(&math))
}

/// Encode literal text as a `CommonMark` code span with safe delimiters.
///
/// The single-span grammar normalizes CR, LF and CRLF to spaces and masks
/// terminal-unsafe controls. A multi-row `Inline::Code` instead retains
/// logical LF rows as separate spans joined by hard breaks during ordinary
/// document export; hand-built CR/CRLF rows are first canonicalized to LF.
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
