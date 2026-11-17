//! Safe report-facing fragments, without document-wide semantic declarations.

use mant_ir::{Block, Diagnostic, DocumentValidation, Heading, Inline, InlineContentRef, Section};

use super::math_markdown::{MathContext, RenderOptions};
use super::navigation::LocalDestinations;
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

/// One presentation and destination policy for detached fragments of one document.
pub type MarkdownFragments = ScopedMarkdownFragments<'static>;

/// A fragment renderer that can borrow validation from its immutable source.
///
/// An excerpt or evidence report can apply known source-wide navigation
/// findings even when it includes only one of the owners responsible for them.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScopedMarkdownFragments<'a> {
    options: MarkdownFragmentOptions,
    destinations: LocalDestinations<'a>,
}

impl From<MarkdownFragmentOptions> for ScopedMarkdownFragments<'_> {
    fn from(options: MarkdownFragmentOptions) -> Self {
        Self {
            options,
            ..Self::default()
        }
    }
}

impl<'a> ScopedMarkdownFragments<'a> {
    /// Retain the original document's machine-readable navigation findings.
    ///
    /// Detached alias conflicts provide no trusted alias spelling. In that
    /// case, keep labels and ordinary links without activating local targets.
    #[must_use]
    pub fn for_report(options: MarkdownFragmentOptions, diagnostics: &[Diagnostic]) -> Self {
        Self {
            options,
            destinations: LocalDestinations::from_diagnostics(diagnostics),
        }
    }

    /// Render fragments projected from the exact original snapshot.
    ///
    /// Use this during the source's projection operation. Independently loaded
    /// report DTOs require [`Self::for_report`], even if their address matches.
    #[must_use]
    pub fn for_snapshot(
        options: MarkdownFragmentOptions,
        snapshot: &'a DocumentValidation<'_>,
    ) -> Self {
        Self {
            options,
            destinations: LocalDestinations::Indexed(snapshot.index()),
        }
    }

    /// Preserve destinations when an authored heading link requires them.
    #[must_use]
    pub fn with_anchors(mut self) -> Self {
        self.options.preserve_anchors = true;
        self
    }

    fn document_options<'ctx>(
        self,
        math: &'ctx MathContext,
        emit_overview: bool,
    ) -> RenderOptions<'ctx>
    where
        'a: 'ctx,
    {
        let options = MarkdownOptions {
            math: self.options.math,
            preserve_anchors: self.options.preserve_anchors,
            preserve_semantics: false,
        };
        RenderOptions {
            options,
            math: (options.math != MarkdownMath::Readable).then_some(math),
            destinations: self.destinations,
            emit_overview,
        }
    }

    /// Encode a detached inline root without introducing reading-layout padding.
    #[must_use]
    pub fn inline(self, content: InlineContentRef<'_>) -> String {
        let math = MathContext::export(true, self.options.math);
        super::inline::render_inline_content(content, self.document_options(&math, false))
    }

    /// Encode detached blocks, optionally decorating their original inline roots.
    #[must_use]
    pub fn blocks(
        self,
        blocks: &[Block],
        projection: Option<&dyn MarkdownInlineProjection>,
    ) -> Vec<String> {
        // A decorated inline root can own temporary formulas. Their addresses
        // remain valid only for that root, so avoid pointer memoization.
        let math = MathContext::export(projection.is_none(), self.options.math);
        super::blocks::render_located_blocks(
            blocks,
            self.document_options(&math, false),
            projection,
        )
    }

    /// Append a detached section forest as Markdown blocks, starting at `depth`.
    pub fn sections(self, output: &mut Vec<String>, sections: &[Section], depth: usize) {
        let math = MathContext::export(true, self.options.math);
        super::render_sections(output, sections, depth, self.document_options(&math, false));
    }

    /// Encode a selected overview and emit one destination when it is addressable.
    #[must_use]
    pub fn overview(
        self,
        heading: Option<&Heading>,
        blocks: &[Block],
        depth: usize,
    ) -> Vec<String> {
        let math = MathContext::export(true, self.options.math);
        let options = self.document_options(&math, true);
        let mut output = Vec::new();
        if options.preserve_anchors {
            output.push(options.destinations.overview_markers());
        }
        if let Some(heading) = heading {
            output.push(super::render_heading(depth, heading, options));
        }
        output.extend(super::blocks::render_blocks(blocks, options));
        output
    }
}

/// Encode a detached inline root without semantic declaration metadata.
#[must_use]
pub fn render_inline_fragment(children: &[Inline], options: MarkdownFragmentOptions) -> String {
    MarkdownFragments::from(options).inline(InlineContentRef::unpositioned(children))
}

/// Encode a detached owner root as ordinary Markdown phrasing.
/// Reading layout hints stay on the owner without generating text cells.
#[must_use]
pub fn render_inline_content_fragment(
    content: InlineContentRef<'_>,
    options: MarkdownFragmentOptions,
) -> String {
    MarkdownFragments::from(options).inline(content)
}

/// Encode detached blocks without semantic declaration metadata.
/// Adjacent roots may share a fragment to retain their required hard-row seam.
/// Returned fragments can be joined with a blank Markdown source line.
#[must_use]
pub fn render_blocks_fragment(blocks: &[Block], options: MarkdownFragmentOptions) -> Vec<String> {
    MarkdownFragments::from(options).blocks(blocks, None)
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
    MarkdownFragments::from(options).blocks(blocks, projection)
}

/// Append a detached section forest as Markdown blocks, starting at `depth`.
pub fn render_sections_fragment(
    output: &mut Vec<String>,
    sections: &[Section],
    depth: usize,
    options: MarkdownFragmentOptions,
) {
    MarkdownFragments::from(options).sections(output, sections, depth);
}

/// Encode a rich heading without semantic declaration metadata.
#[must_use]
pub fn render_heading_fragment(
    depth: usize,
    heading: &Heading,
    options: MarkdownFragmentOptions,
) -> String {
    let math = MathContext::export(true, options.math);
    super::render_heading(
        depth,
        heading,
        MarkdownFragments::from(options).document_options(&math, false),
    )
}

/// Encode the selected document overview as independently joinable blocks.
///
/// The overview anchor is emitted once, even when the source also carries
/// its explicit inline destination. If validation found another owner of
/// that ID, retain the excerpt text without activating the disputed target.
#[must_use]
pub fn render_overview_fragment(
    heading: Option<&Heading>,
    blocks: &[Block],
    depth: usize,
    options: MarkdownFragmentOptions,
    ambiguous_overview: bool,
) -> Vec<String> {
    MarkdownFragments {
        options,
        destinations: if ambiguous_overview {
            LocalDestinations::OverviewConflict
        } else {
            LocalDestinations::Unrestricted
        },
    }
    .overview(heading, blocks, depth)
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
