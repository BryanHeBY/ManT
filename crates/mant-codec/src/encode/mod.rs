//! Encodes already-loaded, source-neutral IR as deterministic portable `CommonMark`.

mod anchors;
mod blocks;
mod flat;
mod fragments;
mod inline;
mod mapped;
mod semantic;
mod source_map;

use std::{error::Error, fmt, ops::Range};

use mant_ir::{
    ContentContext, ContentRootKey, Document, DocumentBody, EntryOwner, FixedBody, FixedBodyError,
    InlineView, OutlinePath, Section, SourceRelationError, SourceSpan, TldrCommandPart,
    TldrDocument, TldrOrigin,
};

use self::{
    blocks::{RenderedBlocks, render_blocks, render_blocks_with_entries},
    inline::escape_text,
};
use crate::ResolvedContent;
use anchors::anchor_markers;
pub use fragments::{
    MarkdownFragmentOptions, commonmark_code_span, escape_commonmark, html_anchor,
    render_blocks_fragment, render_heading_fragment, render_inline_fragment,
    render_located_blocks_fragment, render_projected_inline_fragment, render_sections_fragment,
};
use mant_ir::DOCUMENT_ROOT_ID;

/// Optional report decoration expressed as root-local logical coordinates.
///
/// Document artifacts never accept this projection: their coordinates always
/// describe the canonical bytes. Report renderers decorate one borrowed root
/// at a time without cloning content or fabricating store entries.
pub trait MarkdownInlineProjection {
    /// Root-local Unicode-scalar ranges that receive presentation emphasis.
    ///
    /// Canonical content is never cloned or replaced with fabricated leaves.
    fn scalar_ranges(&self, nodes: &[mant_ir::Inline]) -> &[Range<usize>];
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
///
/// # Errors
/// Returns a typed error if an in-memory Fixed body has invalid display or
/// source relationships; it never exports a placeholder as a successful page.
pub fn render_markdown(query: &ResolvedContent) -> Result<String, EncodeError> {
    render_markdown_with_options(query, MarkdownOptions::default())
}

/// Render a complete query using explicit presentation-only options.
///
/// # Errors
/// Returns a typed error if an in-memory Fixed body is invalid.
pub fn render_markdown_with_options(
    query: &ResolvedContent,
    options: MarkdownOptions,
) -> Result<String, EncodeError> {
    Ok(render_markdown_artifact(query, options, false)?.into_text())
}

/// A hard invalid-result boundary for exporting an in-memory Fixed document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// The native surface or its mark relationships are malformed.
    InvalidFixed(FixedBodyError),
    /// A source-qualified Fixed relationship is not closed by the document.
    InvalidFixedSource(SourceRelationError),
    /// Fixed row/run bytes cannot be represented by the validated surface.
    InvalidFixedSurface(&'static str),
    /// A Fixed Markdown artifact would exceed its bounded derived-output budget.
    ResourceLimit {
        /// Maximum permitted derived artifact bytes.
        maximum: usize,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFixed(error) => write!(formatter, "invalid Fixed body: {error}"),
            Self::InvalidFixedSource(error) => write!(formatter, "invalid Fixed source: {error}"),
            Self::InvalidFixedSurface(reason) => {
                write!(formatter, "invalid Fixed surface: {reason}")
            }
            Self::ResourceLimit { maximum } => {
                write!(
                    formatter,
                    "Fixed Markdown export exceeds the {maximum}-byte resource limit"
                )
            }
        }
    }
}

impl Error for EncodeError {}

/// Canonical Markdown bytes and coordinates borrowing their exact source snapshot.
///
/// Coordinates are UTF-8 byte ranges into [`Self::text`], not Unicode scalar offsets.
/// Keeping the artifact alive also keeps its borrowed entry and section context valid.
pub struct MarkdownArtifact<'src> {
    text: String,
    nodes: Vec<MarkdownNodeRange<'src>>,
    sections: Vec<MarkdownSection<'src>>,
    anchors: std::sync::OnceLock<Vec<Range<usize>>>,
    roots: Vec<source_map::RenderedRootRange>,
}

impl<'src> MarkdownArtifact<'src> {
    /// Final, trimmed bytes against which all node and anchor ranges are indexed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Read-only node ranges; callers cannot detach them from the final bytes.
    pub fn nodes(&self) -> &[MarkdownNodeRange<'src>] {
        &self.nodes
    }

    /// Source-bound section context shared by every mapped entry in that section.
    pub fn section(&self, slot: usize) -> Option<&MarkdownSection<'src>> {
        self.sections.get(slot)
    }

    /// Consume the complete artifact when no coordinate mapping is needed.
    pub fn into_text(self) -> String {
        self.text
    }

    /// Internal markers are indexed against final bytes, on demand. The
    /// operation-local artifact owns the map; presentation-only rendering
    /// does not pay for a second `CommonMark` parse.
    pub fn anchor_ranges(&self) -> &[Range<usize>] {
        self.anchors.get_or_init(|| {
            anchor_markers(&self.text)
                .into_iter()
                .map(|marker| marker.range)
                .collect()
        })
    }

    /// Project an authoritative root-relative logical range into canonical
    /// Markdown byte placements recorded by this render.
    #[must_use]
    pub fn markdown_projections(
        &self,
        content: ContentContext<'_>,
        root: ContentRootKey,
        logical: Range<usize>,
    ) -> Vec<Range<usize>> {
        let Some(logical_text) = content.root_logical_text(root) else {
            return Vec::new();
        };
        source_map::project(&self.text, &self.roots, root, &logical_text, logical)
    }

    /// Rendered Markdown ranges known to contain one logical root.
    ///
    /// These broad ranges are intended for assigning presentation-only node
    /// context. Use [`Self::markdown_projections`] for exact match ranges.
    pub fn root_markdown_ranges(
        &self,
        root: ContentRootKey,
    ) -> impl Iterator<Item = Range<usize>> + '_ {
        self.roots
            .iter()
            .filter(move |candidate| candidate.root == root)
            .map(|candidate| candidate.markdown.clone())
    }
}

/// One source-owned node and its canonical output byte range.
pub struct MarkdownNodeRange<'src> {
    range: Range<usize>,
    node: MarkdownNode<'src>,
}

impl<'src> MarkdownNodeRange<'src> {
    /// Half-open UTF-8 byte range into the owning artifact's text.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Borrowed source identity for this output range.
    #[must_use]
    pub fn node(&self) -> &MarkdownNode<'src> {
        &self.node
    }
}

/// One borrowed section and its parent slot, never a copied title/DTO trail.
pub struct MarkdownSection<'src> {
    path: OutlinePath,
    section: &'src Section,
    parent: Option<usize>,
}

impl<'src> MarkdownSection<'src> {
    /// Structural path in the source snapshot.
    #[must_use]
    pub fn path(&self) -> &OutlinePath {
        &self.path
    }

    /// Original section, without cloning its heading or content.
    #[must_use]
    pub fn section(&self) -> &'src Section {
        self.section
    }

    /// Parent section slot in the same artifact, or `None` for a top-level section.
    #[must_use]
    pub fn parent(&self) -> Option<usize> {
        self.parent
    }
}

/// Source-neutral identity of a mapped Markdown range.
pub enum MarkdownNode<'src> {
    /// The attached quick-reference page.
    Tldr,
    /// The document's authored heading.
    DocumentHeading {
        /// Original heading source span, when available.
        source: Option<SourceSpan>,
    },
    /// Content preceding the first section.
    DocumentRoot,
    /// A section and its content, referenced through an artifact-local slot.
    DocumentSection {
        /// Slot accepted by [`MarkdownArtifact::section`].
        section: usize,
        /// Original heading source span, when available.
        source: Option<SourceSpan>,
    },
    /// A semantic entry mapped directly to its borrowed source owner.
    DocumentEntry {
        /// Entry structural path in the source snapshot.
        path: OutlinePath,
        /// Original list item or definition owning the entry facts.
        owner: EntryOwner<'src>,
        /// Names borrowed from the source entry index.
        names: &'src [String],
        /// Containing section slot, or `None` for root content.
        section: Option<usize>,
        /// Original entry source span, when available.
        source: Option<SourceSpan>,
    },
}

/// Encode canonical addressable bytes with exact, source-bound node coordinates.
///
/// # Errors
/// Returns a typed error if an in-memory Fixed body is invalid.
pub fn render_addressable_markdown(
    query: &ResolvedContent,
) -> Result<MarkdownArtifact<'_>, EncodeError> {
    render_markdown_artifact(query, MarkdownOptions::ADDRESSABLE, true)
}

fn render_markdown_artifact(
    query: &ResolvedContent,
    mut options: MarkdownOptions,
    track: bool,
) -> Result<MarkdownArtifact<'_>, EncodeError> {
    if let Some(document) = &query.document {
        match &document.body {
            DocumentBody::Fixed(fixed) => {
                return render_fixed_artifact(query, document, fixed, track);
            }
            DocumentBody::Flow(_) => {}
        }
    }
    let heading_links = query.document.as_ref().is_some_and(|document| {
        let Some(flow) = document.flow() else {
            return false;
        };
        flow.heading
            .as_ref()
            .is_some_and(|heading| heading_has_local_link(document.content(), heading))
            || section_headings_have_local_links(document.content(), &flow.sections)
    });
    options.preserve_semantics &=
        !heading_links && query.document.as_ref().is_some_and(semantic::supported);
    // Raw HTML anchor blocks are not part of semantic reimport. Real heading
    // links require their destinations, so content preservation takes priority
    // over optional entry metadata for that document.
    options.preserve_anchors =
        heading_links || (options.preserve_anchors && !options.preserve_semantics);
    let mut output = ArtifactBuilder {
        track,
        ..ArtifactBuilder::default()
    };
    if let Some(heading) = query
        .document
        .as_ref()
        .and_then(|document| document.flow())
        .and_then(|flow| flow.heading.as_ref())
    {
        if options.preserve_anchors {
            output.push(&inline::html_anchors(
                DOCUMENT_ROOT_ID,
                &query
                    .document
                    .as_ref()
                    .expect("heading owner")
                    .fragment_aliases,
            ));
        }
        let document = query.document.as_ref().expect("heading owner");
        let range = output.push(&render_heading(document.content(), 1, heading, options));
        output.push_root(
            blocks::inline_root(document.content(), &heading.content),
            range.clone(),
        );
        if track {
            output.nodes.push(MarkdownNodeRange {
                range,
                node: MarkdownNode::DocumentHeading {
                    source: heading.source,
                },
            });
        }
    } else {
        output.push(&heading(1, &query.label));
    }

    if let Some(tldr) = &query.tldr {
        for (index, block) in render_tldr(tldr).into_iter().enumerate() {
            let range = output.push(&block);
            if index == 0 {
                output.begin_tldr(range.start);
            }
        }
        if query.document.is_some() {
            output.push("---");
        }
    }

    if let Some(document) = &query.document {
        let flow = match &document.body {
            DocumentBody::Flow(flow) => flow,
            DocumentBody::Fixed(fixed) => {
                return render_fixed_artifact(query, document, fixed, track);
            }
        };
        let content = document.content();
        if !flow.blocks.is_empty()
            || (flow.heading.is_none() && !document.fragment_aliases.is_empty())
        {
            let start = if options.preserve_anchors && flow.heading.is_none() {
                output
                    .push(&inline::html_anchors(
                        DOCUMENT_ROOT_ID,
                        &document.fragment_aliases,
                    ))
                    .start
            } else {
                output.text.len()
            };
            output.begin_root(start);
            let rendered = render_blocks_with_entries(content, &flow.blocks, options, track);
            output.push_scope(rendered, None, None);
        }
        render_artifact_sections(content, &mut output, &flow.sections, &[], None, 2, options);
    }
    Ok(output.finish())
}

// Fixed has no logical roots or Markdown-addressable semantic nodes.  Keep
// its native row order and terminal columns in one literal body rather than
// pretending it is an empty Flow tree.
fn render_fixed_artifact<'a>(
    query: &'a ResolvedContent,
    document: &Document,
    fixed: &FixedBody,
    track: bool,
) -> Result<MarkdownArtifact<'a>, EncodeError> {
    render_fixed_artifact_with_limit(query, document, fixed, track, MAX_FIXED_MARKDOWN_BYTES)
}

// This limit also bounds the temporary surface and fence strings. It is a
// derived-artifact budget, independent of the native capture byte budget:
// sparse terminal columns can expand a small run arena into many spaces.
const MAX_FIXED_MARKDOWN_BYTES: usize = 64 * 1024 * 1024;

fn render_fixed_artifact_with_limit<'a>(
    query: &'a ResolvedContent,
    document: &Document,
    fixed: &FixedBody,
    track: bool,
    maximum: usize,
) -> Result<MarkdownArtifact<'a>, EncodeError> {
    fixed.validate().map_err(EncodeError::InvalidFixed)?;
    mant_ir::validate_document_sources(document).map_err(EncodeError::InvalidFixedSource)?;
    preflight_fixed_inputs(query, maximum)?;
    let plan = fixed_surface_plan(fixed, maximum)?;
    let title = heading(1, &query.label);
    let tldr_blocks = query.tldr.as_ref().map(render_tldr);
    let artifact_bytes = fixed_artifact_bytes(&title, tldr_blocks.as_deref(), plan, maximum)?;
    let mut output = ArtifactBuilder {
        track,
        ..ArtifactBuilder::default()
    };
    output
        .text
        .try_reserve_exact(artifact_bytes)
        .map_err(|_| EncodeError::ResourceLimit { maximum })?;
    output.push(&title);
    if let Some(blocks) = tldr_blocks {
        for (index, block) in blocks.into_iter().enumerate() {
            let range = output.push(&block);
            if index == 0 {
                output.begin_tldr(range.start);
            }
        }
        output.push("---");
    }
    let text = fixed_surface_text(fixed, plan, maximum)?;
    output.push(&inline::fenced_code(&text, None));
    Ok(output.finish())
}

#[derive(Clone, Copy)]
struct FixedSurfacePlan {
    bytes: usize,
    fence_width: usize,
    boundary_newline: usize,
}

fn charge_fixed(total: &mut usize, amount: usize, maximum: usize) -> Result<(), EncodeError> {
    *total = total
        .checked_add(amount)
        .ok_or(EncodeError::ResourceLimit { maximum })?;
    if *total > maximum {
        return Err(EncodeError::ResourceLimit { maximum });
    }
    Ok(())
}

fn preflight_fixed_inputs(query: &ResolvedContent, maximum: usize) -> Result<(), EncodeError> {
    // CommonMark escaping expands one input byte by at most five bytes (`&`
    // becomes `&amp;`). Per-block slack covers headings, fences and joins.
    // This guard runs before rendering caller-owned TLDR and label strings.
    let mut upper = 64usize;
    let mut input = |value: &str, slack: usize| -> Result<(), EncodeError> {
        let bytes = value
            .len()
            .checked_mul(5)
            .and_then(|bytes| bytes.checked_add(slack))
            .ok_or(EncodeError::ResourceLimit { maximum })?;
        charge_fixed(&mut upper, bytes, maximum)
    };
    input(&query.label, 4)?;
    if let Some(tldr) = &query.tldr {
        input("", 64)?;
        for line in &tldr.description {
            input(line, 4)?;
        }
        if let Some(information) = &tldr.more_information {
            input(information, 64)?;
        }
        for example in &tldr.examples {
            input(&example.description, 64)?;
            input(&example.command, 16)?;
            for part in &example.command_parts {
                let value = match part {
                    TldrCommandPart::Text { value } | TldrCommandPart::Placeholder { value } => {
                        value
                    }
                };
                input(value, 4)?;
            }
        }
        if tldr.origin == TldrOrigin::TldrPages {
            input(&tldr.platform, 40)?;
            input(&tldr.language, 4)?;
        }
    }
    Ok(())
}

fn fixed_surface_plan(fixed: &FixedBody, maximum: usize) -> Result<FixedSurfacePlan, EncodeError> {
    fn invalid(reason: &'static str) -> EncodeError {
        EncodeError::InvalidFixedSurface(reason)
    }
    let surface = &fixed.surface;
    let mut bytes = 0usize;
    let mut longest_ticks = 0usize;
    let mut active_ticks = 0usize;
    let mut last_byte = None;
    for row in &surface.rows {
        let first =
            usize::try_from(row.first_run.get() - 1).map_err(|_| invalid("run index overflow"))?;
        let end = first
            .checked_add(usize::try_from(row.run_count).map_err(|_| invalid("run count overflow"))?)
            .ok_or_else(|| invalid("run count overflow"))?;
        let runs = surface
            .runs
            .get(first..end)
            .ok_or_else(|| invalid("missing display runs"))?;
        let mut column = 0_u32;
        for run in runs {
            let gap = run
                .column
                .checked_sub(column)
                .ok_or_else(|| invalid("overlapping display runs"))?;
            charge_fixed(&mut bytes, gap as usize, maximum)?;
            if gap > 0 {
                active_ticks = 0;
            }
            let text = surface
                .run_text(run.key)
                .ok_or_else(|| invalid("invalid display text range"))?;
            charge_fixed(&mut bytes, text.len(), maximum)?;
            for byte in text.bytes() {
                if byte == b'`' {
                    active_ticks = active_ticks.saturating_add(1);
                    longest_ticks = longest_ticks.max(active_ticks);
                } else {
                    active_ticks = 0;
                }
            }
            last_byte = text.as_bytes().last().copied();
            column = run
                .column
                .checked_add(run.width)
                .ok_or_else(|| invalid("column overflow"))?;
        }
        let trailing = row
            .column_count
            .checked_sub(column)
            .ok_or_else(|| invalid("row width mismatch"))?;
        charge_fixed(&mut bytes, trailing as usize, maximum)?;
        if trailing > 0 {
            active_ticks = 0;
            last_byte = Some(b' ');
        }
        if row.break_after {
            charge_fixed(&mut bytes, 1, maximum)?;
            active_ticks = 0;
            last_byte = Some(b'\n');
        }
    }
    Ok(FixedSurfacePlan {
        bytes,
        fence_width: longest_ticks.saturating_add(1).max(3),
        boundary_newline: usize::from(last_byte != Some(b'\n')),
    })
}

fn fixed_artifact_bytes(
    title: &str,
    tldr_blocks: Option<&[String]>,
    plan: FixedSurfacePlan,
    maximum: usize,
) -> Result<usize, EncodeError> {
    let mut bytes = 0usize;
    charge_fixed(&mut bytes, title.len(), maximum)?;
    if let Some(blocks) = tldr_blocks {
        for block in blocks.iter().filter(|block| !block.is_empty()) {
            charge_fixed(&mut bytes, 2, maximum)?;
            charge_fixed(&mut bytes, block.len(), maximum)?;
        }
        charge_fixed(&mut bytes, 5, maximum)?; // "\n\n---"
    }
    charge_fixed(&mut bytes, 2, maximum)?; // separator before literal
    charge_fixed(
        &mut bytes,
        plan.fence_width
            .checked_mul(2)
            .ok_or(EncodeError::ResourceLimit { maximum })?,
        maximum,
    )?;
    charge_fixed(&mut bytes, 1, maximum)?; // newline after opening fence
    charge_fixed(&mut bytes, plan.bytes, maximum)?;
    charge_fixed(&mut bytes, plan.boundary_newline, maximum)?;
    Ok(bytes)
}

fn fixed_surface_text(
    fixed: &FixedBody,
    plan: FixedSurfacePlan,
    maximum: usize,
) -> Result<String, EncodeError> {
    fn invalid(reason: &'static str) -> EncodeError {
        EncodeError::InvalidFixedSurface(reason)
    }
    let surface = &fixed.surface;
    let mut text = String::new();
    text.try_reserve_exact(plan.bytes)
        .map_err(|_| EncodeError::ResourceLimit { maximum })?;
    for row in &surface.rows {
        let first =
            usize::try_from(row.first_run.get() - 1).map_err(|_| invalid("run index overflow"))?;
        let end = first
            .checked_add(usize::try_from(row.run_count).map_err(|_| invalid("run count overflow"))?)
            .ok_or_else(|| invalid("run count overflow"))?;
        let runs = surface
            .runs
            .get(first..end)
            .ok_or_else(|| invalid("missing display runs"))?;
        let mut column = 0_u32;
        for run in runs {
            let gap = run
                .column
                .checked_sub(column)
                .ok_or_else(|| invalid("overlapping display runs"))?;
            text.extend(std::iter::repeat_n(' ', gap as usize));
            let start =
                usize::try_from(run.byte_start).map_err(|_| invalid("byte offset overflow"))?;
            let count =
                usize::try_from(run.byte_count).map_err(|_| invalid("byte count overflow"))?;
            let end = start
                .checked_add(count)
                .ok_or_else(|| invalid("byte range overflow"))?;
            text.push_str(
                surface
                    .text
                    .get(start..end)
                    .ok_or_else(|| invalid("invalid display text range"))?,
            );
            column = run
                .column
                .checked_add(run.width)
                .ok_or_else(|| invalid("column overflow"))?;
        }
        let trailing = row
            .column_count
            .checked_sub(column)
            .ok_or_else(|| invalid("row width mismatch"))?;
        text.extend(std::iter::repeat_n(' ', trailing as usize));
        if row.break_after {
            text.push('\n');
        }
    }
    Ok(text)
}

#[derive(Default)]
struct ArtifactBuilder<'src> {
    text: String,
    nodes: Vec<MarkdownNodeRange<'src>>,
    sections: Vec<MarkdownSection<'src>>,
    track: bool,
    tldr: Option<usize>,
    root: Option<usize>,
    last_section: Option<usize>,
    roots: Vec<source_map::RenderedRootRange>,
}

impl<'src> ArtifactBuilder<'src> {
    fn push(&mut self, block: &str) -> Range<usize> {
        if block.is_empty() {
            return self.text.len()..self.text.len();
        }
        if !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        let start = self.text.len();
        self.text.push_str(block);
        start..self.text.len()
    }

    fn begin_tldr(&mut self, start: usize) {
        self.tldr = self.node(start, MarkdownNode::Tldr);
    }

    fn begin_root(&mut self, start: usize) {
        self.close_tldr(start);
        self.root = self.node(start, MarkdownNode::DocumentRoot);
    }

    fn begin_section(&mut self, start: usize, section: usize, source: Option<SourceSpan>) {
        self.close_tldr(start);
        if let Some(root) = self.root.take() {
            self.nodes[root].range.end = start;
        }
        if let Some(previous) = self.last_section {
            self.nodes[previous].range.end = start;
        }
        self.last_section = self.node(start, MarkdownNode::DocumentSection { section, source });
    }

    fn push_scope(
        &mut self,
        rendered: RenderedBlocks<'src>,
        section: Option<usize>,
        coordinates: Option<&[usize]>,
    ) {
        if rendered.text.is_empty() {
            return;
        }
        let block = self.push(&rendered.text);
        self.roots
            .extend(rendered.roots.into_iter().map(|mut root| {
                root.markdown.start += block.start;
                root.markdown.end += block.start;
                root
            }));
        for entry in rendered.entries {
            let path = OutlinePath::nested_entry(coordinates, &entry.indices)
                .expect("enumerated entry paths are one-based");
            self.nodes.push(MarkdownNodeRange {
                range: block.start + entry.start..block.start + entry.end,
                node: MarkdownNode::DocumentEntry {
                    path,
                    owner: entry.owner,
                    names: entry.names,
                    section,
                    source: entry.source,
                },
            });
        }
    }

    fn push_root(&mut self, root: Option<ContentRootKey>, markdown: Range<usize>) {
        if self.track
            && !markdown.is_empty()
            && let Some(root) = root
        {
            self.roots
                .push(source_map::RenderedRootRange { root, markdown });
        }
    }

    fn node(&mut self, start: usize, node: MarkdownNode<'src>) -> Option<usize> {
        if !self.track {
            return None;
        }
        let index = self.nodes.len();
        self.nodes.push(MarkdownNodeRange {
            range: start..self.text.len(),
            node,
        });
        Some(index)
    }

    fn close_tldr(&mut self, end: usize) {
        if let Some(tldr) = self.tldr.take() {
            self.nodes[tldr].range.end = end;
        }
    }

    fn finish(mut self) -> MarkdownArtifact<'src> {
        let end = self.text.trim_end().len();
        self.text.truncate(end);
        self.close_tldr(end);
        if let Some(root) = self.root.take() {
            self.nodes[root].range.end = end;
        }
        if let Some(section) = self.last_section {
            self.nodes[section].range.end = end;
        }
        for node in &mut self.nodes {
            node.range.end = node.range.end.min(end);
        }
        MarkdownArtifact {
            text: self.text,
            nodes: self.nodes,
            sections: self.sections,
            anchors: std::sync::OnceLock::new(),
            roots: self.roots,
        }
    }
}

fn render_artifact_sections<'src>(
    content: ContentContext<'src>,
    output: &mut ArtifactBuilder<'src>,
    sections: &'src [Section],
    parent: &[usize],
    parent_slot: Option<usize>,
    depth: usize,
    options: MarkdownOptions,
) {
    for (index, section) in sections.iter().enumerate() {
        let rendered_heading = if options.preserve_anchors {
            format!(
                "{}\n\n{}",
                inline::html_anchors(&section.id, &section.fragment_aliases),
                render_heading(content, depth, &section.heading, options)
            )
        } else {
            render_heading(content, depth, &section.heading, options)
        };
        let range = output.push(&rendered_heading);
        output.push_root(
            blocks::inline_root(content, &section.heading.content),
            range.clone(),
        );
        if !output.track {
            // Stream one scope at a time without retaining the whole document
            // as intermediate block strings or constructing semantic paths.
            output.push_scope(
                render_blocks_with_entries(content, &section.blocks, options, false),
                None,
                None,
            );
            render_artifact_sections(
                content,
                output,
                &section.children,
                &[],
                None,
                depth.saturating_add(1),
                options,
            );
            continue;
        }
        let mut coordinates = parent.to_vec();
        coordinates.push(index + 1);
        let path =
            OutlinePath::section(&coordinates).expect("enumerated section paths are one-based");
        let slot = output.sections.len();
        output.sections.push(MarkdownSection {
            path,
            section,
            parent: parent_slot,
        });
        output.begin_section(range.start, slot, section.source);
        output.push_scope(
            render_blocks_with_entries(content, &section.blocks, options, true),
            Some(slot),
            Some(&coordinates),
        );
        render_artifact_sections(
            content,
            output,
            &section.children,
            &coordinates,
            Some(slot),
            depth.saturating_add(1),
            options,
        );
    }
}

pub(super) fn render_sections(
    content: ContentContext<'_>,
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
                render_heading(content, depth, &section.heading, options)
            ));
        } else {
            output.push(render_heading(content, depth, &section.heading, options));
        }
        output.extend(render_blocks(content, &section.blocks, options));
        render_sections(
            content,
            output,
            &section.children,
            depth.saturating_add(1),
            options,
        );
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
pub fn heading_has_local_link(content: ContentContext<'_>, heading: &mant_ir::Heading) -> bool {
    fn inlines_have_local_link(content: ContentContext<'_>, nodes: &[mant_ir::Inline]) -> bool {
        nodes.iter().any(|inline| {
            match content
                .inline(inline)
                .expect("validated heading content resolves through its content store")
            {
                InlineView::Link(link) => {
                    matches!(link.target(), mant_ir::LinkTarget::Section { .. })
                        || inlines_have_local_link(content, link.children())
                }
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    inlines_have_local_link(content, children)
                }
                _ => false,
            }
        })
    }
    inlines_have_local_link(content, &heading.content)
}

/// Whether any heading in this section forest contains a typed local link.
#[must_use]
pub fn section_headings_have_local_links(
    content: ContentContext<'_>,
    sections: &[Section],
) -> bool {
    sections.iter().any(|section| {
        heading_has_local_link(content, &section.heading)
            || section_headings_have_local_links(content, &section.children)
    })
}

pub(super) fn render_heading(
    content: ContentContext<'_>,
    depth: usize,
    heading: &mant_ir::Heading,
    options: MarkdownOptions,
) -> String {
    let rendered = inline::render_heading_inline(content, &heading.content, options);
    if depth <= 2 && rendered.contains('\n') {
        // Setext headings are the portable CommonMark form that retains
        // explicit inline breaks; an ATX newline would end the heading.
        return format!("{rendered}\n{}", if depth == 1 { "===" } else { "---" });
    }
    // CommonMark has no multiline ATX heading. Keep its hierarchy and
    // linked content on one line rather than accidentally emitting body
    // paragraphs; IR/JSON retain the original explicit line breaks.
    format!(
        "{} {}",
        "#".repeat(depth.clamp(1, 6)),
        rendered.replace("<br>\n", " ").replace("  \n", " ")
    )
}

#[cfg(test)]
mod tests;
