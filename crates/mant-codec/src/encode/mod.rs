//! Encodes already-loaded, source-neutral IR as deterministic portable `CommonMark`.

mod anchors;
mod blocks;
mod flat;
mod fragments;
mod inline;
mod mapped;
mod semantic;

use std::{borrow::Cow, ops::Range};

use mant_ir::{
    EntryOwner, OutlinePath, Section, SourceSpan, TldrCommandPart, TldrDocument, TldrOrigin,
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
    render_located_blocks_fragment, render_sections_fragment,
};
use mant_ir::DOCUMENT_ROOT_ID;

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

/// Canonical Markdown bytes and coordinates borrowing their exact source snapshot.
///
/// Coordinates are UTF-8 byte ranges into [`Self::text`], not Unicode scalar offsets.
/// Keeping the artifact alive also keeps its borrowed entry and section context valid.
pub struct MarkdownArtifact<'src> {
    text: String,
    nodes: Vec<MarkdownNodeRange<'src>>,
    sections: Vec<MarkdownSection<'src>>,
    anchors: std::sync::OnceLock<Vec<Range<usize>>>,
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
#[must_use]
pub fn render_addressable_markdown(query: &ResolvedContent) -> MarkdownArtifact<'_> {
    render_markdown_artifact(query, MarkdownOptions::ADDRESSABLE, true)
}

fn render_markdown_artifact(
    query: &ResolvedContent,
    mut options: MarkdownOptions,
    track: bool,
) -> MarkdownArtifact<'_> {
    let heading_links = query.document.as_ref().is_some_and(|document| {
        document
            .heading
            .as_ref()
            .is_some_and(heading_has_local_link)
            || section_headings_have_local_links(&document.sections)
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
        .and_then(|document| document.heading.as_ref())
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
        let range = output.push(&render_heading(1, heading, options));
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
        if !document.blocks.is_empty()
            || (document.heading.is_none() && !document.fragment_aliases.is_empty())
        {
            let start = if options.preserve_anchors && document.heading.is_none() {
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
            let rendered = render_blocks_with_entries(&document.blocks, options, track);
            output.push_scope(rendered, None, None);
        }
        render_artifact_sections(&mut output, &document.sections, &[], None, 2, options);
    }
    output.finish()
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
        }
    }
}

fn render_artifact_sections<'src>(
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
                render_heading(depth, &section.heading, options)
            )
        } else {
            render_heading(depth, &section.heading, options)
        };
        let range = output.push(&rendered_heading);
        if !output.track {
            // Stream one scope at a time without retaining the whole document
            // as intermediate block strings or constructing semantic paths.
            output.push_scope(
                render_blocks_with_entries(&section.blocks, options, false),
                None,
                None,
            );
            render_artifact_sections(
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
            render_blocks_with_entries(&section.blocks, options, true),
            Some(slot),
            Some(&coordinates),
        );
        render_artifact_sections(
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
            | mant_ir::Inline::Anchor { .. }
            | mant_ir::Inline::LineBreak => false,
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
    let content = inline::render_heading_inline(&heading.content, options);
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
