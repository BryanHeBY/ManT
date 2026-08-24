//! Lowers a `ManT` query into width-aware terminal lines and stable anchors.
//!
//! This module owns wrapping instead of delegating it to a widget. As a result,
//! section navigation, scroll synchronization, links, and future search ranges
//! can all address the exact rows that Ratatui renders.

mod inline;
mod model;
mod search;
mod selection;
mod wrap;

use std::{collections::HashMap, sync::Arc};

use mant_ir::DocumentAddress;
use mant_ir::{
    Block, DefinitionIdentity, DefinitionRole, Inline, ListKind, ResolvedContent, Section,
    SourceFormat, TldrDocument,
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
use inline::safe_external_uri;
use inline::{
    count_sections, inline_anchor_ids, section_semantic_entries, shifted_links, spans_width,
    styled_inline_lines, tldr_style,
};
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
const ROOT_ID: &str = "document-root";
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
    Entry(DefinitionRole),
}

/// Renderer-independent terminal view before width-dependent wrapping.
#[derive(Debug, Clone)]
pub struct DocumentView {
    label: String,
    terminal_label: String,
    source_label: &'static str,
    top_level_count: usize,
    section_count: usize,
    has_tldr: bool,
    lines: Vec<LogicalLine>,
    navigation: Vec<NavNode>,
    anchors: HashMap<String, usize>,
}

/// Exact terminal rows and anchor positions for one content width.
#[derive(Debug, Clone)]
pub struct RenderedDocument {
    /// Fully styled terminal rows.
    pub text: Text<'static>,
    /// Number of visual terminal rows before virtual viewport padding.
    pub row_count: usize,
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
    /// Build one immutable view from the normalized query contract.
    #[must_use]
    pub fn new(bundle: &ResolvedContent) -> Self {
        let mut builder = DocumentBuilder::new(bundle.label.clone(), bundle.address.clone());
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
            if !document.blocks.is_empty() {
                builder.anchor(NavNode {
                    id: ROOT_ID.to_owned(),
                    target_id: ROOT_ID.to_owned(),
                    title: "OVERVIEW".to_owned(),
                    depth: 0,
                    kind: NavKind::Root,
                    has_children: false,
                    is_last: document.sections.is_empty(),
                    parent_id: None,
                });
                builder.blocks(&document.blocks, 0);
            }
            let section_count = document.sections.len();
            for (index, section) in document.sections.iter().enumerate() {
                builder.section_with_position(section, 0, index + 1 == section_count, None);
            }
        }

        Self {
            label: builder.label,
            terminal_label,
            source_label,
            top_level_count,
            section_count,
            has_tldr: bundle.tldr.is_some(),
            lines: builder.lines,
            navigation: builder.navigation,
            anchors: builder.anchors,
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
        let mut logical_rows = Vec::with_capacity(self.lines.len() + 1);

        for line in &self.lines {
            logical_rows.push(rows.len());
            let wrapped_lines = wrap_line_with_links(line, width);
            search_records.extend(search_records_for_lines(&wrapped_lines, rows.len()));
            for wrapped in wrapped_lines {
                let row = rows.len();
                links.extend(wrapped.links.into_iter().map(|link| RenderedLinkRegion {
                    target: link.target,
                    row,
                    start_column: link.start_column,
                    end_column: link.end_column,
                }));
                rows.push(wrapped.line);
            }
        }
        logical_rows.push(rows.len());

        let anchor_rows = self
            .anchors
            .iter()
            .map(|(id, logical_line)| {
                (
                    id.clone(),
                    logical_rows.get(*logical_line).copied().unwrap_or_default(),
                )
            })
            .collect();

        RenderedDocument {
            row_count: rows.len(),
            text: Text::from(rows),
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

struct DocumentBuilder {
    label: String,
    address: Option<DocumentAddress>,
    lines: Vec<LogicalLine>,
    navigation: Vec<NavNode>,
    anchors: HashMap<String, usize>,
}

impl DocumentBuilder {
    fn new(label: String, address: Option<DocumentAddress>) -> Self {
        Self {
            label,
            address,
            lines: Vec::new(),
            navigation: Vec::new(),
            anchors: HashMap::new(),
        }
    }

    fn push(&mut self, line: LogicalLine) {
        self.lines.push(line);
    }

    fn tldr(
        &mut self,
        tldr: &TldrDocument,
        has_document: bool,
        source_label: &'static str,
        document_gap: u16,
    ) {
        self.anchor(NavNode {
            id: TLDR_ID.to_owned(),
            target_id: TLDR_ID.to_owned(),
            title: "TLDR QUICK REFERENCE".to_owned(),
            depth: 0,
            kind: NavKind::Tldr,
            has_children: false,
            is_last: false,
            parent_id: None,
        });
        self.push(LogicalLine::empty().surface(LineSurface::TldrTop));
        for _ in 0..TLDR_VERTICAL_PADDING_ROWS {
            self.push(LogicalLine::empty().surface(LineSurface::Tldr));
        }
        for line in crate::tldr::layout_tldr(tldr) {
            let command = line.spans.iter().any(|span| {
                matches!(
                    span.role,
                    crate::tldr::TldrRole::Command | crate::tldr::TldrRole::Placeholder
                )
            });
            let links = line
                .spans
                .iter()
                .filter(|span| span.role == crate::tldr::TldrRole::Link)
                .filter_map(|span| tldr.more_information.as_ref().map(|uri| (span, uri)))
                .map(|(span, uri)| LogicalLinkRange {
                    target: LinkTarget::External(uri.clone()),
                    start_column: 0,
                    end_column: UnicodeWidthStr::width(span.text.as_str()),
                })
                .collect();
            self.push(LogicalLine {
                indent: line.indent,
                continuation_indent: line.indent,
                spans: line
                    .spans
                    .into_iter()
                    .map(|span| Span::styled(span.text, tldr_style(span.role)))
                    .collect(),
                surface: LineSurface::Tldr,
                wrap_mode: if command {
                    WrapMode::Character
                } else {
                    WrapMode::Word
                },
                table_row: None,
                links,
            });
        }
        for _ in 0..TLDR_VERTICAL_PADDING_ROWS {
            self.push(LogicalLine::empty().surface(LineSurface::Tldr));
        }
        self.push(LogicalLine::empty().surface(LineSurface::TldrBottom));
        self.spacing(document_gap);
        if has_document {
            self.push(LogicalLine::empty().surface(LineSurface::Divider));
            self.push(LogicalLine::plain(
                0,
                source_label,
                Style::default().fg(theme::SUBTEXT),
            ));
            self.spacing(document_gap);
        } else {
            self.push(LogicalLine::empty().surface(LineSurface::Divider));
            self.push(LogicalLine::plain(
                0,
                "No local man page was found; showing the cached tldr quick reference.",
                Style::default().fg(theme::YELLOW),
            ));
        }
    }

    fn anchor(&mut self, node: NavNode) {
        self.anchors
            .insert(node.target_id.clone(), self.lines.len());
        self.navigation(node);
    }

    fn navigation(&mut self, node: NavNode) {
        self.navigation.push(node);
    }

    fn section_with_position(
        &mut self,
        section: &Section,
        depth: usize,
        is_last: bool,
        parent_id: Option<&str>,
    ) {
        self.spacing(section.spacing_before_lines);
        let entries = section_semantic_entries(&section.blocks);
        let has_children = !entries.is_empty() || !section.children.is_empty();
        self.anchor(NavNode {
            id: section.id.to_string(),
            target_id: section.id.to_string(),
            title: section.title.clone(),
            depth,
            kind: NavKind::Section,
            has_children,
            is_last,
            parent_id: parent_id.map(str::to_owned),
        });
        if !entries.is_empty() {
            let group_id = format!("__mant-entries__{}", section.id);
            self.navigation(NavNode {
                id: group_id.clone(),
                target_id: section.id.to_string(),
                title: format!("ENTRIES ({})", entries.len()),
                depth: depth + 1,
                kind: NavKind::EntryGroup,
                has_children: true,
                is_last: section.children.is_empty(),
                parent_id: Some(section.id.to_string()),
            });
            let entry_count = entries.len();
            for (index, identity) in entries.into_iter().enumerate() {
                self.navigation(NavNode {
                    id: identity.id.to_string(),
                    target_id: identity.id.to_string(),
                    title: identity.names.join(", "),
                    depth: depth + 2,
                    kind: NavKind::Entry(identity.role),
                    has_children: false,
                    is_last: index + 1 == entry_count,
                    parent_id: Some(group_id.clone()),
                });
            }
        }
        self.push(LogicalLine::plain(
            depth * 4,
            section.title.clone(),
            Style::default()
                .fg(theme::HEADING)
                .add_modifier(Modifier::BOLD),
        ));
        self.blocks(&section.blocks, depth * 4 + 3);
        let child_count = section.children.len();
        for (index, child) in section.children.iter().enumerate() {
            self.section_with_position(
                child,
                depth + 1,
                index + 1 == child_count,
                Some(&section.id),
            );
        }
    }

    fn blocks(&mut self, blocks: &[Block], base_indent: usize) {
        for block in blocks {
            self.block(block, base_indent);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn block(&mut self, block: &Block, base_indent: usize) {
        match block {
            Block::Paragraph {
                children, layout, ..
            } => {
                self.spacing(layout.spacing_before_lines);
                self.inline_lines(
                    children,
                    base_indent + usize::from(layout.indent_columns),
                    Style::default().fg(theme::TEXT),
                );
            }
            Block::Preformatted {
                children, layout, ..
            } => {
                self.spacing(layout.spacing_before_lines);
                self.inline_lines_with_surface(
                    children,
                    base_indent + usize::from(layout.indent_columns),
                    Style::default().fg(theme::TEXT),
                    LineSurface::Code,
                );
            }
            Block::List {
                kind,
                start,
                compact,
                items,
                layout,
                ..
            } => {
                self.spacing(layout.spacing_before_lines);
                let indent = base_indent + usize::from(layout.indent_columns);
                for (index, item) in items.iter().enumerate() {
                    if index > 0 && !compact {
                        self.spacing(1);
                    }
                    let marker = match kind {
                        ListKind::Bullet => "• ".to_owned(),
                        ListKind::Ordered => format!(
                            "{}. ",
                            start
                                .unwrap_or(1)
                                .saturating_add(u64::try_from(index).unwrap_or(u64::MAX))
                        ),
                        ListKind::Plain => String::new(),
                    };
                    let has_marker = !marker.is_empty();
                    if has_marker
                        && let Some(Block::Paragraph {
                            children, layout, ..
                        }) = item.blocks.first()
                    {
                        self.spacing(layout.spacing_before_lines);
                        let marker_width = UnicodeWidthStr::width(marker.as_str());
                        let content_indent =
                            indent + marker_width + usize::from(layout.indent_columns);
                        let mut inline_lines = styled_inline_lines(
                            children,
                            Style::default().fg(theme::TEXT),
                            self.address.as_ref(),
                        );
                        let first = inline_lines
                            .first_mut()
                            .map_or_else(StyledInlineLine::default, std::mem::take);
                        let mut spans =
                            vec![Span::styled(marker, Style::default().fg(theme::HEADING))];
                        spans.push(Span::raw(" ".repeat(usize::from(layout.indent_columns))));
                        spans.extend(first.spans);
                        self.push(
                            LogicalLine::hanging(indent, content_indent, spans).with_links(
                                shifted_links(first.links, content_indent.saturating_sub(indent)),
                            ),
                        );
                        for line in inline_lines.into_iter().skip(1) {
                            self.push(
                                LogicalLine::hanging(content_indent, content_indent, line.spans)
                                    .with_links(line.links),
                            );
                        }
                        self.blocks(&item.blocks[1..], indent + marker_width);
                    } else {
                        if has_marker {
                            self.push(LogicalLine::plain(
                                indent,
                                marker,
                                Style::default().fg(theme::HEADING),
                            ));
                        }
                        self.blocks(&item.blocks, indent + usize::from(has_marker) * 2);
                    }
                }
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                self.spacing(layout.spacing_before_lines);
                let indent = base_indent + usize::from(layout.indent_columns);
                for (index, item) in items.iter().enumerate() {
                    let spacing = item
                        .spacing_before_lines
                        .unwrap_or(u16::from(index > 0 && !compact));
                    self.spacing(spacing);
                    if let Some(identity) = &item.identity {
                        self.anchors
                            .insert(identity.id.to_string(), self.lines.len());
                    }
                    if item.inline_term {
                        self.inline_definition(item, indent);
                    } else {
                        for term in &item.terms {
                            self.inline_lines(
                                term,
                                indent,
                                Style::default().fg(theme::SUBTEXT_BRIGHT),
                            );
                        }
                        self.blocks(&item.description, indent + 4);
                    }
                }
            }
            Block::Table { rows, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                let indent = base_indent + usize::from(layout.indent_columns);
                let rows = rows
                    .iter()
                    .map(|row| {
                        row.cells
                            .iter()
                            .map(|cell| {
                                let mut builder = Self::new(String::new(), self.address.clone());
                                builder.blocks(&cell.blocks, 0);
                                LogicalTableCell::new(builder.lines, cell.alignment)
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let table_layout = Arc::new(LogicalTableLayout::for_rows(&rows));
                for cells in rows {
                    self.push(LogicalLine::table(indent, cells, Arc::clone(&table_layout)));
                }
            }
            Block::Equation { value, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                self.push(
                    LogicalLine::plain(
                        base_indent + usize::from(layout.indent_columns),
                        value.clone(),
                        Style::default().fg(theme::YELLOW),
                    )
                    .wrap_mode(WrapMode::Character),
                );
            }
            Block::VerticalSpace { lines, .. } => self.spacing(*lines),
            Block::ThematicBreak { .. } => self.push(LogicalLine::rule(base_indent)),
            Block::Unsupported { text, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                self.push(LogicalLine::plain(
                    base_indent + usize::from(layout.indent_columns),
                    text.clone(),
                    Style::default().fg(theme::PEACH),
                ));
            }
        }
    }

    fn spacing(&mut self, lines: u16) {
        for _ in 0..lines {
            self.lines.push(LogicalLine::empty());
        }
    }

    fn inline_definition(&mut self, item: &mant_ir::DefinitionItem, indent: usize) {
        let mut term_spans = Vec::new();
        let mut term_links = Vec::new();
        for (index, term) in item.terms.iter().enumerate() {
            for line in styled_inline_lines(
                term,
                Style::default().fg(theme::SUBTEXT_BRIGHT),
                self.address.as_ref(),
            ) {
                let offset = spans_width(&term_spans);
                term_links.extend(shifted_links(line.links, offset));
                term_spans.extend(line.spans);
            }
            term_spans.push(Span::styled(
                if index + 1 < item.terms.len() {
                    ", "
                } else {
                    " "
                },
                Style::default().fg(theme::SUBTEXT_BRIGHT),
            ));
        }
        let term_width = spans_width(&term_spans);

        if let Some(Block::Paragraph {
            children, layout, ..
        }) = item.description.first()
            && layout.spacing_before_lines == 0
        {
            let description_indent = indent + term_width + usize::from(layout.indent_columns);
            term_spans.push(Span::raw(" ".repeat(usize::from(layout.indent_columns))));
            let mut description_lines = styled_inline_lines(
                children,
                Style::default().fg(theme::TEXT),
                self.address.as_ref(),
            );
            let first = description_lines
                .first_mut()
                .map_or_else(StyledInlineLine::default, std::mem::take);
            let description_offset = spans_width(&term_spans);
            term_links.extend(shifted_links(first.links, description_offset));
            term_spans.extend(first.spans);
            self.push(
                LogicalLine::hanging(indent, description_indent, term_spans).with_links(term_links),
            );
            for line in description_lines.into_iter().skip(1) {
                self.push(
                    LogicalLine::hanging(description_indent, description_indent, line.spans)
                        .with_links(line.links),
                );
            }
            self.blocks(&item.description[1..], description_indent);
        } else {
            self.push(LogicalLine::hanging(indent, indent, term_spans).with_links(term_links));
            self.blocks(&item.description, indent + term_width);
        }
    }

    fn inline_lines(&mut self, nodes: &[Inline], indent: usize, base_style: Style) {
        self.inline_lines_with_surface(nodes, indent, base_style, LineSurface::Normal);
    }

    fn inline_lines_with_surface(
        &mut self,
        nodes: &[Inline],
        indent: usize,
        base_style: Style,
        surface: LineSurface,
    ) {
        for id in inline_anchor_ids(nodes) {
            self.anchors.entry(id).or_insert(self.lines.len());
        }
        let lines = styled_inline_lines(nodes, base_style, self.address.as_ref())
            .into_iter()
            .map(|line| {
                let spans = if surface == LineSurface::Code {
                    crate::code::highlight(line.spans)
                } else {
                    line.spans
                };
                LogicalLine {
                    indent,
                    continuation_indent: indent,
                    spans,
                    surface,
                    wrap_mode: if surface == LineSurface::Code {
                        WrapMode::Character
                    } else {
                        WrapMode::Word
                    },
                    table_row: None,
                    links: line.links,
                }
            })
            .collect::<Vec<_>>();

        for line in lines {
            self.push(line);
        }
    }
}

#[cfg(test)]
mod tests;
