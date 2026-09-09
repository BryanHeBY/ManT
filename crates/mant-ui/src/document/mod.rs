//! Lowers a `ManT` query into width-aware terminal lines and stable anchors.
//!
//! This module owns wrapping instead of delegating it to a widget. As a result,
//! section navigation, scroll synchronization, links, and future search ranges
//! can all address the exact rows that Ratatui renders.

mod inline;
mod lower;
mod navigation;
use lower::DocumentBuilder;
mod model;
mod references;
mod search;
mod selection;
mod wrap;

use std::{collections::HashMap, sync::Arc};

use mant_ir::{
    Block, DocumentAddress, EntryKind, Inline, ListKind, ResolvedContent, Section, SemanticEntry,
    SemanticIndex, SourceFormat, TldrDocument,
};
#[cfg(test)]
use mant_ir::{TldrCommandPart, TldrOrigin};
#[cfg(test)]
use ratatui::text::Line;
use ratatui::{
    style::{Modifier, Style},
    text::{Span, Text},
};
#[cfg(test)]
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::theme;
#[cfg(test)]
use inline::styled_inline_lines;
use inline::{count_sections, inline_anchor_rows, shifted_links, spans_width, tldr_style};
pub use model::ExternalUri;
pub(crate) use model::LinkTarget;
use model::{
    LineSurface, LogicalLine, LogicalLinkRange, LogicalTableCell, LogicalTableLayout,
    StyledInlineLine, WrapMode,
};

pub use self::search::RenderedSearchMatch;
#[cfg(test)]
use self::search::{RenderedSearchFragment, RenderedSearchSourceCell};
use self::search::{RenderedSearchRecord, search_records_for_lines};
pub(crate) use self::selection::{RenderedSelection, TextPosition};
#[cfg(test)]
use self::wrap::wrap_line;
use self::wrap::{WrappedLine, wrap_line_with_links};

const TLDR_ID: &str = "tldr";
const ROOT_ID: &str = mant_ir::DOCUMENT_ROOT_ID;
const TLDR_VERTICAL_PADDING_ROWS: u16 = 1;

/// One addressable node displayed in the outline sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavNode {
    /// Stable sidebar identity.
    pub id: String,
    /// Document anchor selected when the item is activated.
    pub target_id: String,
    /// Visible sidebar label.
    pub title: String,
    /// Complete authored label used while selected or in full-label mode.
    pub full_title: Option<String>,
    /// Zero-based tree indentation depth.
    pub depth: usize,
    /// Semantic presentation category.
    pub kind: NavKind,
    /// Whether collapse/expand behavior applies.
    pub has_children: bool,
    /// Whether this is the final sibling at its depth.
    pub is_last: bool,
    /// Parent sidebar identity, when nested.
    pub parent_id: Option<String>,
}

/// Semantic presentation class for a navigation entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKind {
    /// Optional quick-reference entry.
    Tldr,
    /// Content preceding the first document section.
    Root,
    /// Ordinary document section.
    Section,
    /// Synthetic grouping for semantic entries.
    EntryGroup,
    /// Addressable semantic definition of the contained role.
    Entry(EntryKind),
    /// Collapsed orthogonal document-reference grouping, not source content.
    ReferenceGroup,
    /// One real, structurally located link occurrence.
    Reference,
    /// Honest disclosure that bounded discovery did not cover every reference.
    ReferenceNotice,
}

/// Renderer-independent terminal view before width-dependent wrapping.
#[derive(Debug, Clone)]
pub struct DocumentView {
    address: Option<DocumentAddress>,
    label: String,
    terminal_label: String,
    source_label: &'static str,
    top_level_count: usize,
    section_count: usize,
    has_tldr: bool,
    lines: Vec<LogicalLine>,
    navigation: Vec<NavNode>,
    anchors: HashMap<String, usize>,
    references: Vec<references::ReferenceRecord>,
}

/// Exact terminal rows and anchor positions for one content width.
#[derive(Debug, Clone)]
pub struct RenderedDocument {
    /// Fully styled terminal rows.
    pub text: Text<'static>,
    /// Number of visual terminal rows before virtual viewport padding.
    pub row_count: usize,
    /// Presentation surface associated with each visual row.
    surfaces: Vec<LineSurface>,
    /// First visual row for each logical source row, followed by one sentinel.
    logical_rows: Vec<usize>,
    anchor_rows: HashMap<String, usize>,
    links: Vec<RenderedLinkRegion>,
    search_records: Vec<RenderedSearchRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedLinkRegion {
    target: LinkTarget,
    row: usize,
    start_column: usize,
    end_column: usize,
}

impl DocumentView {
    pub(crate) fn reference_location(&self, id: &str) -> Option<&mant_ir::ContentLocation> {
        self.references
            .iter()
            .find(|reference| reference.id.as_ref() == id)
            .map(|reference| &reference.location)
    }
    pub(crate) fn reference_target(&self, id: &str) -> Option<&mant_ir::LinkTarget> {
        self.references
            .iter()
            .find(|reference| reference.id.as_ref() == id)
            .map(|reference| &reference.target)
    }

    pub(crate) fn reference_text(&self, id: &str) -> Option<String> {
        self.reference_target(id).map(references::target_text)
    }

    pub(crate) fn activation_target(&self, target: &mant_ir::LinkTarget) -> Option<LinkTarget> {
        inline::local_link_target(target, self.address.as_ref())
    }
    /// Build one immutable view from the normalized query contract.
    #[must_use]
    pub fn new(bundle: &ResolvedContent) -> Self {
        let mut builder = DocumentBuilder::new(bundle.label.clone(), bundle.address.clone());
        let mut references = bundle.document.as_ref().map_or_else(
            references::ReferenceNavigation::default,
            references::ReferenceNavigation::build,
        );
        builder.reference_origins = Arc::new(std::mem::take(&mut references.origins));
        let source_label = bundle.document.as_ref().map_or("MANUAL", |document| {
            if document.source.format == SourceFormat::Markdown {
                "MARKDOWN"
            } else {
                "MANUAL"
            }
        });
        let top_level_count = bundle
            .document
            .as_ref()
            .map_or(0, |document| document.sections.len());
        let terminal_label = bundle.document.as_ref().map_or_else(
            || bundle.label.clone(),
            |document| {
                document.meta.manual_section.as_ref().map_or_else(
                    || bundle.label.clone(),
                    |section| format!("{}({section})", bundle.label),
                )
            },
        );
        let section_count = bundle
            .document
            .as_ref()
            .map_or(0, |document| count_sections(&document.sections));

        if let Some(tldr) = &bundle.tldr {
            let document_gap = u16::from(
                bundle
                    .document
                    .as_ref()
                    .is_none_or(|document| document.source.format != SourceFormat::Markdown),
            );
            builder.tldr(tldr, bundle.document.is_some(), source_label, document_gap);
        }

        if let Some(document) = &bundle.document {
            let semantic_index = SemanticIndex::build(document);
            builder.entry_styles = Arc::new(mant_protocol::EntryStyleMap::for_document(document));
            if document.heading.is_some()
                || !document.blocks.is_empty()
                || !document.fragment_aliases.is_empty()
            {
                let entries = semantic_index.root();
                builder.anchor(NavNode {
                    id: ROOT_ID.to_owned(),
                    target_id: ROOT_ID.to_owned(),
                    title: "OVERVIEW".to_owned(),
                    full_title: None,
                    depth: 0,
                    kind: NavKind::Root,
                    has_children: !entries.is_empty(),
                    is_last: document.sections.is_empty(),
                    parent_id: None,
                });
                for alias in &document.fragment_aliases {
                    builder
                        .anchors
                        .entry(alias.to_string())
                        .or_insert(builder.lines.len());
                }
                builder.entry_group(ROOT_ID, ROOT_ID, entries, 1, document.sections.is_empty());
                if let Some(heading) = &document.heading {
                    builder.heading(heading, 0);
                }
                builder.blocks(&document.blocks, 0);
            }
            let section_count = document.sections.len();
            for (index, section) in document.sections.iter().enumerate() {
                builder.section_with_position(
                    section,
                    &semantic_index,
                    0,
                    index + 1 == section_count,
                    None,
                );
            }
        }

        let mut built = builder.finish();
        references.append_navigation(&mut built.navigation);
        Self {
            address: bundle.address.clone(),
            label: built.label,
            terminal_label,
            source_label,
            top_level_count,
            section_count,
            has_tldr: bundle.tldr.is_some(),
            lines: built.content.lines,
            navigation: built.navigation,
            anchors: built.content.anchors,
            references: references.records,
        }
    }

    /// Return the original human-readable query label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Label used in terminal chrome, including the resolved manual section.
    #[must_use]
    pub fn terminal_label(&self) -> &str {
        &self.terminal_label
    }

    /// Return the immutable navigation tree in source order.
    #[must_use]
    pub fn navigation(&self) -> &[NavNode] {
        &self.navigation
    }

    /// Return the source-family label displayed in terminal chrome.
    #[must_use]
    pub const fn source_label(&self) -> &'static str {
        self.source_label
    }

    /// Return the number of top-level document sections.
    #[must_use]
    pub const fn top_level_count(&self) -> usize {
        self.top_level_count
    }

    /// Return the total number of nested and top-level sections.
    #[must_use]
    pub const fn section_count(&self) -> usize {
        self.section_count
    }

    /// Return whether optional quick-reference content is present.
    #[must_use]
    pub const fn has_tldr(&self) -> bool {
        self.has_tldr
    }

    /// Wrap logical lines to the actual content width and translate anchors.
    #[must_use]
    pub fn render(&self, width: u16) -> RenderedDocument {
        let width = usize::from(width.max(1));
        let mut rows = Vec::new();
        let mut links = Vec::new();
        let mut search_records = Vec::new();
        let mut surfaces = Vec::new();
        let mut logical_rows = Vec::with_capacity(self.lines.len() + 1);
        let mut anchor_rows = HashMap::new();

        for line in &self.lines {
            logical_rows.push(rows.len());
            let wrapped_lines = wrap_line_with_links(line, width);
            search_records.extend(search_records_for_lines(&wrapped_lines, rows.len()));
            for wrapped in wrapped_lines {
                let row = rows.len();
                for id in wrapped.anchors {
                    anchor_rows.entry(id).or_insert(row);
                }
                links.extend(wrapped.links.into_iter().map(|link| RenderedLinkRegion {
                    target: link.target,
                    row,
                    start_column: link.start_column,
                    end_column: link.end_column,
                }));
                rows.push(wrapped.line);
                surfaces.push(line.surface);
            }
        }
        logical_rows.push(rows.len());

        anchor_rows.extend(self.anchors.iter().map(|(id, logical_line)| {
            (
                id.clone(),
                logical_rows.get(*logical_line).copied().unwrap_or_default(),
            )
        }));

        RenderedDocument {
            row_count: rows.len(),
            text: Text::from(rows),
            surfaces,
            logical_rows,
            anchor_rows,
            links,
            search_records,
        }
    }
}

impl RenderedDocument {
    /// Return the first visual row associated with a document-local anchor.
    #[must_use]
    pub fn anchor_row(&self, id: &str) -> Option<usize> {
        self.anchor_rows.get(id).copied()
    }

    pub(crate) fn viewport_anchor(&self, row: usize) -> Option<(usize, usize)> {
        if self.logical_rows.len() < 2 {
            return None;
        }
        let logical_line = self
            .logical_rows
            .partition_point(|start| *start <= row)
            .saturating_sub(1)
            .min(self.logical_rows.len() - 2);
        Some((
            logical_line,
            row.saturating_sub(self.logical_rows[logical_line]),
        ))
    }

    pub(crate) fn row_for_viewport_anchor(&self, anchor: (usize, usize)) -> Option<usize> {
        let (logical_line, wrapped_offset) = anchor;
        let start = *self.logical_rows.get(logical_line)?;
        let end = *self.logical_rows.get(logical_line + 1)?;
        Some(start + wrapped_offset.min(end.saturating_sub(start).saturating_sub(1)))
    }

    #[must_use]
    pub(super) fn link_target_at(&self, row: usize, column: usize) -> Option<&LinkTarget> {
        self.links
            .iter()
            .find(|link| link.row == row && link.start_column <= column && column < link.end_column)
            .map(|link| &link.target)
    }
}

#[cfg(test)]
mod tests;
