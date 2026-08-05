//! Lowers a `ManT` query into width-aware terminal lines and stable anchors.
//!
//! This module owns wrapping instead of delegating it to a widget. As a result,
//! section navigation, scroll synchronization, links, and future search ranges
//! can all address the exact rows that Ratatui renders.

use std::collections::{BTreeMap, HashMap};

use mant_ast::{
    Block, DefinitionIdentity, DefinitionRole, Inline, ListKind, QueryBundle, Section,
    SourceFormat, TldrCommandPart, TldrDocument, TldrOrigin,
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span, Text},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme;

const TLDR_ID: &str = "tldr";
const ROOT_ID: &str = "document-root";

/// One addressable entry displayed in the navigation sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    pub id: String,
    pub target_id: String,
    pub title: String,
    pub depth: usize,
    pub kind: NavKind,
    pub has_children: bool,
    pub is_last: bool,
    pub parent_id: Option<String>,
}

/// Semantic presentation class for a navigation entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKind {
    Tldr,
    Root,
    Section,
    EntryGroup,
    Option,
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
    navigation: Vec<NavItem>,
    anchors: HashMap<String, usize>,
}

/// Exact terminal rows and anchor positions for one content width.
#[derive(Debug, Clone)]
pub struct RenderedDocument {
    pub text: Text<'static>,
    pub row_count: usize,
    anchor_rows: HashMap<String, usize>,
    links: Vec<RenderedLinkRegion>,
    search_records: Vec<RenderedSearchRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedLinkRegion {
    target: String,
    row: usize,
    start_column: usize,
    end_column: usize,
}

/// One exact visual-row range found in a width-dependent document rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSearchMatch {
    pub row: usize,
    pub start_column: usize,
    pub end_column: usize,
    additional_fragments: Vec<RenderedSearchFragment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderedSearchFragment {
    row: usize,
    start_column: usize,
    end_column: usize,
}

#[derive(Debug, Clone)]
struct RenderedSearchRecord {
    text: String,
    cells: Vec<RenderedSearchSourceCell>,
}

#[derive(Debug, Clone, Copy)]
struct RenderedSearchSourceCell {
    source_start: usize,
    source_end: usize,
    fragment: RenderedSearchFragment,
}

#[derive(Debug, Clone)]
struct LogicalLine {
    indent: usize,
    continuation_indent: usize,
    spans: Vec<Span<'static>>,
    surface: LineSurface,
    wrap_mode: WrapMode,
    table_cells: Option<Vec<Vec<LogicalLine>>>,
    links: Vec<LogicalLinkRange>,
}

#[derive(Debug, Clone)]
struct LogicalLinkRange {
    target: String,
    start_column: usize,
    end_column: usize,
}

#[derive(Debug, Clone, Default)]
struct StyledInlineLine {
    spans: Vec<Span<'static>>,
    links: Vec<LogicalLinkRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WrapMode {
    Word,
    Character,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineSurface {
    Normal,
    Code,
    Tldr,
    TldrTop,
    TldrBottom,
    Divider,
    Rule,
}

impl LogicalLine {
    fn empty() -> Self {
        Self {
            indent: 0,
            continuation_indent: 0,
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_cells: None,
            links: Vec::new(),
        }
    }

    fn plain(indent: usize, value: impl Into<String>, style: Style) -> Self {
        Self {
            indent,
            continuation_indent: indent,
            spans: vec![Span::styled(value.into(), style)],
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_cells: None,
            links: Vec::new(),
        }
    }

    fn surface(mut self, surface: LineSurface) -> Self {
        self.surface = surface;
        self
    }

    fn wrap_mode(mut self, wrap_mode: WrapMode) -> Self {
        self.wrap_mode = wrap_mode;
        self
    }

    fn with_links(mut self, links: Vec<LogicalLinkRange>) -> Self {
        self.links = links;
        self
    }

    fn hanging(indent: usize, continuation_indent: usize, spans: Vec<Span<'static>>) -> Self {
        Self {
            indent,
            continuation_indent,
            spans,
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_cells: None,
            links: Vec::new(),
        }
    }

    fn table(indent: usize, cells: Vec<Vec<Self>>) -> Self {
        Self {
            indent,
            continuation_indent: indent,
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_cells: Some(cells),
            links: Vec::new(),
        }
    }

    fn rule(indent: usize) -> Self {
        let mut line = Self::empty();
        line.indent = indent;
        line.continuation_indent = indent;
        line.surface = LineSurface::Rule;
        line
    }
}

impl DocumentView {
    /// Build one immutable view from the normalized query contract.
    #[must_use]
    pub fn new(bundle: &QueryBundle) -> Self {
        let mut builder = DocumentBuilder::new(bundle.label.clone());
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
                document.meta.section.as_ref().map_or_else(
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
            builder.tldr(tldr, bundle.document.is_some(), source_label);
        }

        if let Some(document) = &bundle.document {
            if !document.blocks.is_empty() {
                builder.anchor(
                    ROOT_ID,
                    "OVERVIEW",
                    0,
                    NavKind::Root,
                    false,
                    document.sections.is_empty(),
                    None,
                );
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

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Label used in terminal chrome, including the resolved manual section.
    #[must_use]
    pub fn terminal_label(&self) -> &str {
        &self.terminal_label
    }

    #[must_use]
    pub fn navigation(&self) -> &[NavItem] {
        &self.navigation
    }

    #[must_use]
    pub const fn source_label(&self) -> &'static str {
        self.source_label
    }

    #[must_use]
    pub const fn top_level_count(&self) -> usize {
        self.top_level_count
    }

    #[must_use]
    pub const fn section_count(&self) -> usize {
        self.section_count
    }

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
            anchor_rows,
            links,
            search_records,
        }
    }
}

impl RenderedDocument {
    #[must_use]
    pub fn anchor_row(&self, id: &str) -> Option<usize> {
        self.anchor_rows.get(id).copied()
    }

    #[must_use]
    pub fn link_target_at(&self, row: usize, column: usize) -> Option<&str> {
        self.links
            .iter()
            .find(|link| link.row == row && link.start_column <= column && column < link.end_column)
            .map(|link| link.target.as_str())
    }

    /// Search visible terminal rows without rebuilding or traversing the AST.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<RenderedSearchMatch> {
        let needle = query.to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.search_records
            .iter()
            .flat_map(|record| {
                let folded = fold_for_search(&record.text);
                folded
                    .value
                    .match_indices(&needle)
                    .filter_map(|(start, value)| {
                        let source_start = folded.starts[start];
                        let source_end = folded.ends[start + value.len() - 1];
                        search_match_for_range(record, source_start, source_end)
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Clone the rendered rows and decorate ordinary and active search hits.
    #[must_use]
    pub fn highlighted_text(
        &self,
        matches: &[RenderedSearchMatch],
        active: Option<usize>,
    ) -> Text<'static> {
        let mut text = self.text.clone();
        let mut by_row: HashMap<usize, Vec<(usize, RenderedSearchFragment)>> = HashMap::new();
        for (index, search_match) in matches.iter().enumerate() {
            let first = RenderedSearchFragment {
                row: search_match.row,
                start_column: search_match.start_column,
                end_column: search_match.end_column,
            };
            for fragment in
                std::iter::once(first).chain(search_match.additional_fragments.iter().copied())
            {
                by_row
                    .entry(fragment.row)
                    .or_default()
                    .push((index, fragment));
            }
        }
        for (row, ranges) in by_row {
            let Some(line) = text.lines.get_mut(row) else {
                continue;
            };
            *line = highlight_line(line, &ranges, active);
        }
        text
    }
}

fn search_records_for_lines(lines: &[WrappedLine], first_row: usize) -> Vec<RenderedSearchRecord> {
    #[derive(Default)]
    struct RecordBuilder {
        text: String,
        cells: Vec<RenderedSearchSourceCell>,
        last_line: Option<usize>,
    }

    let mut records: BTreeMap<usize, RecordBuilder> = BTreeMap::new();
    for (line_index, line) in lines.iter().enumerate() {
        for cell in &line.search_cells {
            let record = records.entry(cell.group).or_default();
            if record.last_line != Some(line_index) {
                if record.last_line.is_some() && cell.join_before {
                    record.text.push(' ');
                }
                record.last_line = Some(line_index);
            }
            let source_start = record.text.len();
            record.text.push(cell.character);
            record.cells.push(RenderedSearchSourceCell {
                source_start,
                source_end: record.text.len(),
                fragment: RenderedSearchFragment {
                    row: first_row + line_index,
                    start_column: cell.start_column,
                    end_column: cell.end_column,
                },
            });
        }
    }
    records
        .into_values()
        .filter_map(|record| {
            (!record.text.is_empty()).then_some(RenderedSearchRecord {
                text: record.text,
                cells: record.cells,
            })
        })
        .collect()
}

fn search_match_for_range(
    record: &RenderedSearchRecord,
    source_start: usize,
    source_end: usize,
) -> Option<RenderedSearchMatch> {
    let mut fragments: Vec<RenderedSearchFragment> = Vec::new();
    for cell in record
        .cells
        .iter()
        .filter(|cell| cell.source_start < source_end && cell.source_end > source_start)
    {
        if let Some(last) = fragments.last_mut()
            && last.row == cell.fragment.row
            && last.end_column == cell.fragment.start_column
        {
            last.end_column = cell.fragment.end_column;
        } else {
            fragments.push(cell.fragment);
        }
    }
    let first = fragments.first().copied()?;
    Some(RenderedSearchMatch {
        row: first.row,
        start_column: first.start_column,
        end_column: first.end_column,
        additional_fragments: fragments.into_iter().skip(1).collect(),
    })
}

struct FoldedText {
    value: String,
    starts: Vec<usize>,
    ends: Vec<usize>,
}

fn fold_for_search(value: &str) -> FoldedText {
    let mut folded = String::new();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for (source_start, character) in value.char_indices() {
        let source_end = source_start + character.len_utf8();
        let lowered = character.to_lowercase().to_string();
        folded.push_str(&lowered);
        starts.resize(folded.len(), source_start);
        ends.resize(folded.len(), source_end);
    }
    FoldedText {
        value: folded,
        starts,
        ends,
    }
}

fn highlight_line(
    line: &Line<'static>,
    ranges: &[(usize, RenderedSearchFragment)],
    active: Option<usize>,
) -> Line<'static> {
    let mut spans = Vec::new();
    let mut column = 0;
    for span in &line.spans {
        let mut segment = String::new();
        let mut segment_style = None;
        for character in span.content.chars() {
            let width = character.width().unwrap_or(0);
            let next_column = column + width;
            let matched = ranges
                .iter()
                .find(|(_, range)| range.start_column < next_column && range.end_column > column);
            let style = match matched {
                Some((index, _)) if Some(*index) == active => span
                    .style
                    .fg(theme::BASE)
                    .bg(theme::SEARCH_ACTIVE)
                    .add_modifier(Modifier::BOLD),
                Some(_) => span.style.bg(theme::SEARCH_MATCH),
                None => span.style,
            };
            if segment_style.is_some_and(|current| current != style) {
                spans.push(Span::styled(
                    std::mem::take(&mut segment),
                    segment_style.unwrap(),
                ));
            }
            segment_style = Some(style);
            segment.push(character);
            column = next_column;
        }
        if !segment.is_empty() {
            spans.push(Span::styled(segment, segment_style.unwrap_or(span.style)));
        }
    }
    Line::from(spans)
}

struct DocumentBuilder {
    label: String,
    lines: Vec<LogicalLine>,
    navigation: Vec<NavItem>,
    anchors: HashMap<String, usize>,
}

impl DocumentBuilder {
    fn new(label: String) -> Self {
        Self {
            label,
            lines: Vec::new(),
            navigation: Vec::new(),
            anchors: HashMap::new(),
        }
    }

    fn push(&mut self, line: LogicalLine) {
        self.lines.push(line);
    }

    fn tldr(&mut self, tldr: &TldrDocument, has_document: bool, source_label: &'static str) {
        self.anchor(
            TLDR_ID,
            "TLDR QUICK REFERENCE",
            0,
            NavKind::Tldr,
            false,
            false,
            None,
        );
        self.push(LogicalLine::empty().surface(LineSurface::TldrTop));
        self.push(
            LogicalLine::plain(
                0,
                format!("TLDR QUICK REFERENCE · {}", tldr.title),
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            )
            .surface(LineSurface::Tldr),
        );
        for description in &tldr.description {
            self.push(
                LogicalLine::plain(0, description.clone(), Style::default().fg(theme::TEXT))
                    .surface(LineSurface::Tldr),
            );
        }
        for example in &tldr.examples {
            self.tldr_example(example);
        }
        if let Some(link) = &tldr.more_information {
            self.push(LogicalLine::empty().surface(LineSurface::Tldr));
            self.push(
                LogicalLine::plain(
                    0,
                    format!("More information: {link}"),
                    Style::default()
                        .fg(theme::BLUE)
                        .add_modifier(Modifier::UNDERLINED),
                )
                .surface(LineSurface::Tldr),
            );
        }
        if tldr.origin == TldrOrigin::TldrPages {
            self.push(
                LogicalLine::plain(
                    0,
                    format!(
                        "tldr-pages · CC BY 4.0 · {} · {}",
                        tldr.platform, tldr.language
                    ),
                    Style::default().fg(theme::SUBTEXT),
                )
                .surface(LineSurface::Tldr),
            );
        }
        self.push(LogicalLine::empty().surface(LineSurface::TldrBottom));
        if has_document {
            self.push(LogicalLine::empty().surface(LineSurface::Divider));
            self.push(LogicalLine::plain(
                0,
                source_label,
                Style::default().fg(theme::SUBTEXT),
            ));
        } else {
            self.push(LogicalLine::empty().surface(LineSurface::Divider));
            self.push(LogicalLine::plain(
                0,
                "No local man page was found; showing the cached tldr quick reference.",
                Style::default().fg(theme::YELLOW),
            ));
        }
    }

    fn tldr_example(&mut self, example: &mant_ast::TldrExample) {
        self.push(LogicalLine::empty().surface(LineSurface::Tldr));
        self.push(
            LogicalLine::plain(
                0,
                example.description.clone(),
                Style::default().fg(theme::GREEN),
            )
            .surface(LineSurface::Tldr),
        );
        let spans = example
            .command_parts
            .iter()
            .map(|part| match part {
                TldrCommandPart::Text { value } => {
                    Span::styled(value.clone(), Style::default().fg(theme::TEXT))
                }
                TldrCommandPart::Placeholder { value } => {
                    Span::styled(value.clone(), Style::default().fg(theme::YELLOW))
                }
            })
            .collect();
        self.push(LogicalLine {
            indent: 2,
            continuation_indent: 2,
            spans,
            surface: LineSurface::Tldr,
            wrap_mode: WrapMode::Word,
            table_cells: None,
            links: Vec::new(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn anchor(
        &mut self,
        id: &str,
        title: &str,
        depth: usize,
        kind: NavKind,
        has_children: bool,
        is_last: bool,
        parent_id: Option<&str>,
    ) {
        self.anchors.insert(id.to_owned(), self.lines.len());
        self.navigation(id, id, title, depth, kind, has_children, is_last, parent_id);
    }

    #[allow(clippy::too_many_arguments)]
    fn navigation(
        &mut self,
        id: &str,
        target_id: &str,
        title: &str,
        depth: usize,
        kind: NavKind,
        has_children: bool,
        is_last: bool,
        parent_id: Option<&str>,
    ) {
        self.navigation.push(NavItem {
            id: id.to_owned(),
            target_id: target_id.to_owned(),
            title: title.to_owned(),
            depth,
            kind,
            has_children,
            is_last,
            parent_id: parent_id.map(str::to_owned),
        });
    }

    fn section_with_position(
        &mut self,
        section: &Section,
        depth: usize,
        is_last: bool,
        parent_id: Option<&str>,
    ) {
        self.spacing(section.spacing_before_lines);
        let options = section_option_entries(&section.blocks);
        let has_children = !options.is_empty() || !section.children.is_empty();
        self.anchor(
            &section.id,
            &section.title,
            depth,
            NavKind::Section,
            has_children,
            is_last,
            parent_id,
        );
        if !options.is_empty() {
            let group_id = format!("__mant-options__{}", section.id);
            self.navigation(
                &group_id,
                &section.id,
                &format!("OPTIONS ({})", options.len()),
                depth + 1,
                NavKind::EntryGroup,
                true,
                section.children.is_empty(),
                Some(&section.id),
            );
            let option_count = options.len();
            for (index, identity) in options.into_iter().enumerate() {
                self.navigation(
                    &identity.id,
                    &identity.id,
                    &identity.names.join(", "),
                    depth + 2,
                    NavKind::Option,
                    false,
                    index + 1 == option_count,
                    Some(&group_id),
                );
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
                        ListKind::Ordered => format!("{}. ", start.unwrap_or(1) + index as u64),
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
                        let mut inline_lines =
                            styled_inline_lines(children, Style::default().fg(theme::TEXT));
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
                        self.anchors.insert(identity.id.clone(), self.lines.len());
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
                for row in rows {
                    let cells = row
                        .cells
                        .iter()
                        .map(|cell| {
                            let mut builder = Self::new(String::new());
                            builder.blocks(&cell.blocks, 0);
                            builder.lines
                        })
                        .collect();
                    self.push(LogicalLine::table(indent, cells));
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

    fn inline_definition(&mut self, item: &mant_ast::DefinitionItem, indent: usize) {
        let mut term_spans = Vec::new();
        let mut term_links = Vec::new();
        for (index, term) in item.terms.iter().enumerate() {
            for line in styled_inline_lines(term, Style::default().fg(theme::SUBTEXT_BRIGHT)) {
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
            let mut description_lines =
                styled_inline_lines(children, Style::default().fg(theme::TEXT));
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
        for line in styled_inline_lines(nodes, base_style) {
            let spans = if surface == LineSurface::Code {
                crate::code::highlight(line.spans)
            } else {
                line.spans
            };
            self.push(LogicalLine {
                indent,
                continuation_indent: indent,
                spans,
                surface,
                wrap_mode: if surface == LineSurface::Code {
                    WrapMode::Character
                } else {
                    WrapMode::Word
                },
                table_cells: None,
                links: line.links,
            });
        }
    }
}

fn inline_anchor_ids(nodes: &[Inline]) -> Vec<String> {
    let mut ids = Vec::new();
    for node in nodes {
        match node {
            Inline::Anchor { id } => ids.push(id.clone()),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => ids.extend(inline_anchor_ids(children)),
            Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => {}
        }
    }
    ids
}

fn styled_inline_lines(nodes: &[Inline], style: Style) -> Vec<StyledInlineLine> {
    let mut lines = vec![StyledInlineLine::default()];
    append_inline(nodes, style, &mut lines);
    lines
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn shifted_links(links: Vec<LogicalLinkRange>, columns: usize) -> Vec<LogicalLinkRange> {
    links
        .into_iter()
        .map(|mut link| {
            link.start_column += columns;
            link.end_column += columns;
            link
        })
        .collect()
}

fn count_sections(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|section| 1 + count_sections(&section.children))
        .sum()
}

fn section_option_entries(blocks: &[Block]) -> Vec<&DefinitionIdentity> {
    let mut entries = Vec::new();
    collect_option_entries(blocks, &mut entries);
    entries
}

fn collect_option_entries<'a>(blocks: &'a [Block], entries: &mut Vec<&'a DefinitionIdentity>) {
    for block in blocks {
        match block {
            Block::DefinitionList { items, .. } => {
                for item in items {
                    if let Some(identity) = &item.identity
                        && identity.role == DefinitionRole::Option
                    {
                        entries.push(identity);
                    }
                    collect_option_entries(&item.description, entries);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_option_entries(&item.blocks, entries);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_option_entries(&cell.blocks, entries);
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn append_inline(nodes: &[Inline], style: Style, lines: &mut Vec<StyledInlineLine>) {
    for node in nodes {
        match node {
            Inline::Text { value } => append_text(value, style, lines),
            Inline::Strong { children } => {
                append_inline(children, style.add_modifier(Modifier::BOLD), lines);
            }
            Inline::Emphasis { children } => {
                append_inline(children, style.add_modifier(Modifier::ITALIC), lines);
            }
            Inline::Code { value } => {
                append_text(value, Style::default().fg(theme::HEADING), lines);
            }
            Inline::ExternalLink { children, .. } | Inline::EmailLink { children, .. } => {
                append_inline(
                    children,
                    Style::default()
                        .fg(theme::BLUE)
                        .add_modifier(Modifier::UNDERLINED),
                    lines,
                );
            }
            Inline::ManualReference { children, .. } => {
                append_inline(children, Style::default().fg(theme::LINK), lines);
            }
            Inline::SectionReference { target, children } => {
                let first_line = lines.len() - 1;
                let first_column = spans_width(&lines[first_line].spans);
                append_inline(
                    children,
                    Style::default()
                        .fg(theme::LINK)
                        .add_modifier(Modifier::UNDERLINED),
                    lines,
                );
                let last_line = lines.len() - 1;
                for (line_index, line) in lines
                    .iter_mut()
                    .enumerate()
                    .take(last_line + 1)
                    .skip(first_line)
                {
                    let start_column = if line_index == first_line {
                        first_column
                    } else {
                        0
                    };
                    let end_column = spans_width(&line.spans);
                    if end_column > start_column {
                        line.links.push(LogicalLinkRange {
                            target: target.clone(),
                            start_column,
                            end_column,
                        });
                    }
                }
            }
            Inline::Anchor { .. } => {}
            Inline::LineBreak => lines.push(StyledInlineLine::default()),
        }
    }
}

fn append_text(value: &str, style: Style, lines: &mut Vec<StyledInlineLine>) {
    for (index, part) in value.split('\n').enumerate() {
        if index > 0 {
            lines.push(StyledInlineLine::default());
        }
        if !part.is_empty() {
            lines
                .last_mut()
                .expect("inline builder always owns one line")
                .spans
                .push(Span::styled(part.to_owned(), style));
        }
    }
}

#[derive(Clone, Copy)]
struct StyledCell {
    character: char,
    width: usize,
    style: Style,
    link_index: Option<usize>,
}

struct WrappedLine {
    line: Line<'static>,
    links: Vec<WrappedLink>,
    search_cells: Vec<WrappedSearchCell>,
}

#[derive(Clone, Copy)]
struct WrappedSearchCell {
    group: usize,
    join_before: bool,
    character: char,
    start_column: usize,
    end_column: usize,
}

struct WrappedLink {
    target: String,
    start_column: usize,
    end_column: usize,
}

#[cfg(test)]
fn wrap_line(line: &LogicalLine, width: usize) -> Vec<Line<'static>> {
    wrap_line_with_links(line, width)
        .into_iter()
        .map(|wrapped| wrapped.line)
        .collect()
}

#[allow(clippy::too_many_lines)]
fn wrap_line_with_links(line: &LogicalLine, width: usize) -> Vec<WrappedLine> {
    if let Some(cells) = &line.table_cells {
        return render_table_row_with_links(line.indent, cells, width);
    }
    match line.surface {
        LineSurface::TldrTop => {
            return vec![WrappedLine {
                line: panel_border(width, '┌', '┐'),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::TldrBottom => {
            return vec![WrappedLine {
                line: panel_border(width, '└', '┘'),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Divider => {
            return vec![WrappedLine {
                line: Line::from(Span::styled(
                    "─".repeat(width),
                    Style::default().fg(theme::OVERLAY),
                )),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Rule => {
            let indent = line.indent.min(width.saturating_sub(1));
            return vec![WrappedLine {
                line: Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(
                        "─".repeat(width.saturating_sub(indent)),
                        Style::default().fg(theme::OVERLAY),
                    ),
                ]),
                links: Vec::new(),
                search_cells: Vec::new(),
            }];
        }
        LineSurface::Normal | LineSurface::Code | LineSurface::Tldr => {}
    }

    let decoration_width = usize::from(line.surface == LineSurface::Tldr) * 4;
    let mut cells = styled_cells(line);

    if cells.is_empty() {
        return vec![wrapped_cells_to_line(
            line,
            width,
            line.indent.min(width.saturating_sub(1)),
            &[],
            false,
        )];
    }

    let mut result = Vec::new();
    let mut first_row = true;
    let mut join_with_space = false;
    while !cells.is_empty() {
        let indent = if first_row {
            line.indent
        } else {
            line.continuation_indent
        }
        .min(width.saturating_sub(1));
        let available = width
            .saturating_sub(indent)
            .saturating_sub(decoration_width)
            .max(1);
        let fit = fitting_prefix(&cells, available);
        if fit == cells.len() {
            result.push(wrapped_cells_to_line(
                line,
                width,
                indent,
                &cells,
                join_with_space,
            ));
            break;
        }

        if line.wrap_mode == WrapMode::Character {
            result.push(wrapped_cells_to_line(
                line,
                width,
                indent,
                &cells[..fit],
                join_with_space,
            ));
            cells.drain(..fit);
            join_with_space = false;
        } else {
            let split = cells[..fit]
                .iter()
                .rposition(|cell| cell.character.is_whitespace())
                .filter(|position| *position > 0)
                .unwrap_or(fit);
            let row_end = trim_trailing_whitespace(&cells, split);
            let emitted_row = row_end != 0;
            let mut removed_separator = if row_end == 0 {
                cells.drain(..fit.max(1));
                true
            } else {
                result.push(wrapped_cells_to_line(
                    line,
                    width,
                    indent,
                    &cells[..row_end],
                    join_with_space,
                ));
                let removed_separator = split < fit;
                let consumed = if removed_separator { split + 1 } else { split };
                cells.drain(..consumed.max(1));
                removed_separator
            };
            while cells
                .first()
                .is_some_and(|cell| cell.character.is_whitespace())
            {
                cells.remove(0);
                removed_separator = true;
            }
            join_with_space = if emitted_row {
                removed_separator
            } else {
                join_with_space || removed_separator
            };
        }
        first_row = false;
    }
    result
}

fn styled_cells(line: &LogicalLine) -> Vec<StyledCell> {
    const TAB_STOP: usize = 8;

    let mut cells = Vec::new();
    let mut column = line.indent;
    let mut source_column = 0;
    for span in &line.spans {
        for character in span.content.chars() {
            let source_width = character.width().unwrap_or(0);
            let link_index = line.links.iter().position(|link| {
                link.start_column < source_column + source_width && link.end_column > source_column
            });
            if character == '\t' {
                let spaces = TAB_STOP - column % TAB_STOP;
                cells.extend((0..spaces).map(|_| StyledCell {
                    character: ' ',
                    width: 1,
                    style: span.style,
                    link_index,
                }));
                column += spaces;
                source_column += spaces;
                continue;
            }
            let character = if character.is_control() {
                '\u{fffd}'
            } else {
                character
            };
            let cell_width = character.width().unwrap_or(0);
            cells.push(StyledCell {
                character,
                width: cell_width,
                style: span.style,
                link_index,
            });
            column += cell_width;
            source_column += source_width;
        }
    }
    cells
}

fn render_table_row_with_links(
    indent: usize,
    cells: &[Vec<LogicalLine>],
    width: usize,
) -> Vec<WrappedLine> {
    if cells.is_empty() {
        return vec![WrappedLine {
            line: Line::default(),
            links: Vec::new(),
            search_cells: Vec::new(),
        }];
    }
    let indent = indent.min(width.saturating_sub(1));
    let available = width.saturating_sub(indent).max(1);
    let base_width = available / cells.len();
    let remainder = available % cells.len();
    let column_widths = (0..cells.len())
        .map(|index| base_width + usize::from(index < remainder))
        .collect::<Vec<_>>();
    let mut next_search_group = 0;
    let rendered_cells = cells
        .iter()
        .zip(&column_widths)
        .map(|(lines, column_width)| {
            if *column_width == 0 {
                return Vec::new();
            }
            let mut rendered = Vec::new();
            for line in lines {
                let group = next_search_group;
                next_search_group += 1;
                let mut wrapped = wrap_line_with_links(line, *column_width);
                for row in &mut wrapped {
                    for cell in &mut row.search_cells {
                        cell.group = group;
                    }
                }
                rendered.extend(wrapped);
            }
            rendered
        })
        .collect::<Vec<_>>();
    let row_count = rendered_cells.iter().map(Vec::len).max().unwrap_or(1);

    (0..row_count)
        .map(|row_index| {
            let mut spans = Vec::new();
            let mut links = Vec::new();
            let mut search_cells = Vec::new();
            let mut column_offset = indent;
            if indent > 0 {
                spans.push(Span::raw(" ".repeat(indent)));
            }
            for (cell_rows, column_width) in rendered_cells.iter().zip(&column_widths) {
                let mut used = 0;
                if let Some(row) = cell_rows.get(row_index) {
                    used = UnicodeWidthStr::width(row.line.to_string().as_str());
                    spans.extend(row.line.spans.clone());
                    links.extend(row.links.iter().map(|link| WrappedLink {
                        target: link.target.clone(),
                        start_column: column_offset + link.start_column,
                        end_column: column_offset + link.end_column,
                    }));
                    search_cells.extend(row.search_cells.iter().map(|cell| WrappedSearchCell {
                        group: cell.group,
                        join_before: cell.join_before,
                        character: cell.character,
                        start_column: column_offset + cell.start_column,
                        end_column: column_offset + cell.end_column,
                    }));
                }
                spans.push(Span::raw(" ".repeat(column_width.saturating_sub(used))));
                column_offset += column_width;
            }
            WrappedLine {
                line: Line::from(spans),
                links,
                search_cells,
            }
        })
        .collect()
}

fn fitting_prefix(cells: &[StyledCell], available: usize) -> usize {
    let mut width = 0;
    for (index, cell) in cells.iter().enumerate() {
        if width + cell.width > available {
            return index.max(1);
        }
        width += cell.width;
    }
    cells.len()
}

fn trim_trailing_whitespace(cells: &[StyledCell], end: usize) -> usize {
    let mut result = end;
    while result > 0 && cells[result - 1].character.is_whitespace() {
        result -= 1;
    }
    result
}

fn cells_to_line(
    line: &LogicalLine,
    width: usize,
    indent: usize,
    cells: &[StyledCell],
) -> Line<'static> {
    let mut spans = Vec::new();
    let background = match line.surface {
        LineSurface::Code => Some(theme::SURFACE),
        LineSurface::Tldr => Some(theme::TLDR_SURFACE),
        LineSurface::Normal
        | LineSurface::TldrTop
        | LineSurface::TldrBottom
        | LineSurface::Divider
        | LineSurface::Rule => None,
    };

    if line.surface == LineSurface::Tldr {
        spans.push(Span::styled(
            "│ ",
            Style::default().fg(theme::MAUVE).bg(theme::TLDR_SURFACE),
        ));
    }
    if indent > 0 {
        let style = background.map_or_else(Style::default, |color| Style::default().bg(color));
        spans.push(Span::styled(" ".repeat(indent), style));
    }

    if let Some(first) = cells.first() {
        let mut current_style = with_background(first.style, background);
        let mut value = String::new();
        for cell in cells {
            let style = with_background(cell.style, background);
            if style != current_style {
                spans.push(Span::styled(std::mem::take(&mut value), current_style));
                current_style = style;
            }
            value.push(cell.character);
        }
        spans.push(Span::styled(value, current_style));
    }

    if let Some(color) = background {
        let used = indent + cells.iter().map(|cell| cell.width).sum::<usize>();
        let reserved = if line.surface == LineSurface::Tldr {
            4
        } else {
            0
        };
        let fill = width.saturating_sub(used + reserved);
        spans.push(Span::styled(" ".repeat(fill), Style::default().bg(color)));
    }
    if line.surface == LineSurface::Tldr {
        spans.push(Span::styled(
            " │",
            Style::default().fg(theme::MAUVE).bg(theme::TLDR_SURFACE),
        ));
    }
    Line::from(spans)
}

fn wrapped_cells_to_line(
    line: &LogicalLine,
    width: usize,
    indent: usize,
    cells: &[StyledCell],
    join_with_space: bool,
) -> WrappedLine {
    let mut links = Vec::new();
    let mut search_cells = Vec::with_capacity(cells.len());
    let mut column = indent + usize::from(line.surface == LineSurface::Tldr) * 2;
    let mut active: Option<(usize, usize, usize)> = None;
    for (index, cell) in cells.iter().enumerate() {
        let next_column = column + cell.width;
        search_cells.push(WrappedSearchCell {
            group: 0,
            join_before: index == 0 && join_with_space,
            character: cell.character,
            start_column: column,
            end_column: next_column,
        });
        match (active, cell.link_index) {
            (Some((index, start, _)), Some(next)) if index == next => {
                active = Some((index, start, next_column));
            }
            (Some((index, start, end)), next) => {
                links.push(WrappedLink {
                    target: line.links[index].target.clone(),
                    start_column: start,
                    end_column: end,
                });
                active = next.map(|index| (index, column, next_column));
            }
            (None, Some(index)) => active = Some((index, column, next_column)),
            (None, None) => {}
        }
        column = next_column;
    }
    if let Some((index, start, end)) = active {
        links.push(WrappedLink {
            target: line.links[index].target.clone(),
            start_column: start,
            end_column: end,
        });
    }
    WrappedLine {
        line: cells_to_line(line, width, indent, cells),
        links,
        search_cells,
    }
}

fn with_background(style: Style, background: Option<ratatui::style::Color>) -> Style {
    background.map_or(style, |color| style.bg(color))
}

fn panel_border(width: usize, left: char, right: char) -> Line<'static> {
    let style = Style::default().fg(theme::MAUVE).bg(theme::TLDR_SURFACE);
    if width == 1 {
        return Line::from(Span::styled(left.to_string(), style));
    }
    Line::from(Span::styled(
        format!("{left}{}{right}", "─".repeat(width.saturating_sub(2))),
        style,
    ))
}

#[cfg(test)]
mod tests {
    use mant_ast::{
        DefinitionIdentity, DefinitionItem, DefinitionRole, DocumentMeta, DocumentSchema,
        DocumentSource, LayoutHint, ListItem, MantDocument, Producer, QuerySchema, SourceFormat,
        TableCell, TableRow, TldrDocument, TldrExample,
    };
    use unicode_width::UnicodeWidthStr;

    use super::*;

    fn bundle() -> QueryBundle {
        QueryBundle {
            schema: QuerySchema::V3,
            label: "demo".to_owned(),
            document: Some(MantDocument {
                schema: DocumentSchema::V3,
                producer: Producer {
                    name: "mant".to_owned(),
                    version: "test".to_owned(),
                    engine: None,
                },
                source: DocumentSource {
                    format: SourceFormat::Markdown,
                    path: None,
                    renderer: None,
                },
                meta: DocumentMeta::default(),
                diagnostics: Vec::new(),
                blocks: Vec::new(),
                sections: vec![Section {
                    id: "description".to_owned(),
                    title: "Description".to_owned(),
                    spacing_before_lines: 0,
                    blocks: vec![Block::Paragraph {
                        children: vec![Inline::Text {
                            value: "a deliberately long sentence".to_owned(),
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    children: Vec::new(),
                    source: None,
                }],
            }),
            tldr: None,
        }
    }

    #[test]
    fn records_section_rows_after_wrapping() {
        let view = DocumentView::new(&bundle());
        let rendered = view.render(12);

        assert_eq!(rendered.anchor_row("description"), Some(0));
        assert!(rendered.row_count >= 4);
        assert_eq!(view.navigation()[0].title, "Description");
    }

    #[test]
    fn terminal_chrome_keeps_the_manual_section_out_of_the_sidebar_label() {
        let mut bundle = bundle();
        let document = bundle.document.as_mut().expect("document");
        document.meta.section = Some("1".to_owned());
        document.blocks.push(Block::Paragraph {
            children: vec![Inline::Text {
                value: "overview".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        });

        let view = DocumentView::new(&bundle);

        assert_eq!(view.label(), "demo");
        assert_eq!(view.terminal_label(), "demo(1)");
        assert_eq!(view.top_level_count(), 1);
    }

    #[test]
    fn section_spacing_is_not_coalesced_with_existing_blank_rows() {
        let mut bundle = bundle();
        let document = bundle.document.as_mut().expect("document");
        document.blocks = vec![Block::VerticalSpace {
            lines: 1,
            source: None,
        }];
        document.sections[0].spacing_before_lines = 2;

        let rendered = DocumentView::new(&bundle).render(80);

        assert_eq!(rendered.anchor_row("description"), Some(3));
    }

    #[test]
    fn a_tldr_only_result_explains_why_no_manual_body_follows() {
        let mut bundle = bundle();
        bundle.document = None;
        bundle.tldr = Some(TldrDocument {
            title: "demo".to_owned(),
            description: vec!["Quick reference".to_owned()],
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "demo.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        });

        let rendered = DocumentView::new(&bundle).render(80);
        let output = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(output.contains("No local man page was found"));
    }

    #[test]
    fn manual_references_are_distinct_without_implying_page_local_clickability() {
        let lines = styled_inline_lines(
            &[Inline::ManualReference {
                name: "printf".to_owned(),
                section: Some("3".to_owned()),
                children: vec![Inline::Text {
                    value: "printf(3)".to_owned(),
                }],
            }],
            Style::default(),
        );

        assert_eq!(lines[0].spans[0].style.fg, Some(theme::LINK));
        assert!(
            !lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn inline_styles_preserve_the_renderer_neutral_ast_semantics() {
        let lines = styled_inline_lines(
            &[
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "strong".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: "emphasis".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::Code {
                    value: "--option".to_owned(),
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::ExternalLink {
                    uri: "https://example.test".to_owned(),
                    title: None,
                    children: vec![Inline::Text {
                        value: "link".to_owned(),
                    }],
                },
            ],
            Style::default().fg(theme::TEXT),
        );
        let spans = &lines[0].spans;

        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert!(spans[2].style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(spans[4].style.fg, Some(theme::HEADING));
        assert_eq!(spans[6].style.fg, Some(theme::BLUE));
        assert!(spans[6].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn wrapped_rows_preserve_their_indent() {
        let line = LogicalLine::plain(3, "abcdefgh", Style::default());
        let rows = wrap_line(&line, 7);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].to_string(), "   abcd");
        assert_eq!(rows[1].to_string(), "   efgh");
    }

    #[test]
    fn wrapping_prefers_word_boundaries() {
        let line = LogicalLine::plain(2, "alpha beta", Style::default());
        let rows = wrap_line(&line, 8);

        assert_eq!(rows[0].to_string(), "  alpha");
        assert_eq!(rows[1].to_string(), "  beta");
    }

    #[test]
    fn code_surfaces_fill_the_available_row_after_the_body_indent() {
        let line = LogicalLine::plain(3, "code", Style::default()).surface(LineSurface::Code);
        let rows = wrap_line(&line, 12);

        assert_eq!(UnicodeWidthStr::width(rows[0].to_string().as_str()), 12);
        assert_eq!(rows[0].spans[0].content, "   ");
        assert_eq!(rows[0].spans[1].style.bg, Some(theme::SURFACE));
        assert_eq!(
            rows[0].spans.last().and_then(|span| span.style.bg),
            Some(theme::SURFACE)
        );
    }

    #[test]
    fn preformatted_character_wrapping_preserves_significant_spaces() {
        let line = LogicalLine::plain(2, "ab  cd", Style::default())
            .surface(LineSurface::Code)
            .wrap_mode(WrapMode::Character);
        let rows = wrap_line(&line, 7);

        assert_eq!(&rows[0].to_string()[..7], "  ab  c");
        assert!(rows[1].to_string().starts_with("  d"));
    }

    #[test]
    fn tldr_is_rendered_as_a_bordered_full_width_panel() {
        let mut bundle = bundle();
        bundle.tldr = Some(TldrDocument {
            title: "demo".to_owned(),
            description: vec!["Quick reference".to_owned()],
            more_information: None,
            examples: vec![TldrExample {
                description: "Run the command".to_owned(),
                command: "demo file".to_owned(),
                command_parts: vec![TldrCommandPart::Text {
                    value: "demo file".to_owned(),
                }],
            }],
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "demo.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        });

        let rendered = DocumentView::new(&bundle).render(32);

        assert!(rendered.text.lines[0].to_string().starts_with('┌'));
        assert_eq!(
            UnicodeWidthStr::width(rendered.text.lines[0].to_string().as_str()),
            32
        );
        assert!(rendered.text.lines.iter().any(|line| {
            line.to_string().contains("Quick reference")
                && line
                    .spans
                    .iter()
                    .all(|span| span.style.bg == Some(theme::TLDR_SURFACE))
        }));
        assert!(
            rendered
                .text
                .lines
                .iter()
                .any(|line| line.to_string() == "─".repeat(32))
        );
    }

    #[test]
    fn bullet_lists_share_the_first_row_and_use_a_hanging_indent() {
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: true,
            items: vec![ListItem {
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "alpha beta gamma".to_owned(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                }],
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        let rendered = DocumentView::new(&bundle).render(16);
        let rows = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        assert_eq!(rows[1], "   • alpha beta");
        assert_eq!(rows[2], "     gamma");
    }

    #[test]
    fn inline_definitions_hang_the_description_and_expose_their_anchor() {
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks =
            vec![Block::DefinitionList {
                items: vec![DefinitionItem {
                    identity: Some(DefinitionIdentity {
                        id: "help-option".to_owned(),
                        role: DefinitionRole::Option,
                        names: vec!["-h".to_owned()],
                    }),
                    terms: vec![vec![Inline::Strong {
                        children: vec![Inline::Text {
                            value: "-h".to_owned(),
                        }],
                    }]],
                    description: vec![Block::Paragraph {
                        children: vec![Inline::Text {
                            value: "Show detailed command help".to_owned(),
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    inline_term: true,
                    spacing_before_lines: None,
                }],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }];

        let rendered = DocumentView::new(&bundle).render(18);
        let rows = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        assert_eq!(rendered.anchor_row("help-option"), Some(1));
        assert_eq!(rows[1], "   -h Show");
        assert!(rows[2].starts_with("      detailed"));
    }

    #[test]
    fn definition_lists_honour_compact_and_per_item_spacing() {
        let definition = |term: &str, description: &str, spacing_before_lines| DefinitionItem {
            identity: None,
            terms: vec![vec![Inline::Text {
                value: term.to_owned(),
            }]],
            description: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: description.to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            inline_term: false,
            spacing_before_lines,
        };
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks =
            vec![Block::DefinitionList {
                items: vec![
                    definition("-E", "Run the preprocessor.", None),
                    definition("-S", "Run the compiler.", Some(2)),
                ],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }];

        let rows = DocumentView::new(&bundle)
            .render(80)
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let first_description = rows
            .iter()
            .position(|row| row.contains("Run the preprocessor."))
            .expect("first description");
        let second_term = rows
            .iter()
            .position(|row| row.contains("-S"))
            .expect("second term");

        assert_eq!(second_term, first_description + 3);
        assert!(rows[first_description + 1].trim().is_empty());
        assert!(rows[first_description + 2].trim().is_empty());
    }

    #[test]
    fn adjacent_blocks_add_only_explicit_vertical_space() {
        let paragraph = |value: &str| Block::Paragraph {
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks = vec![
            paragraph("before"),
            Block::Preformatted {
                children: vec![Inline::Text {
                    value: "display".to_owned(),
                }],
                language: None,
                layout: LayoutHint::default(),
                source: None,
            },
            paragraph("after"),
            Block::VerticalSpace {
                lines: 1,
                source: None,
            },
            paragraph("spaced"),
        ];

        let rows = DocumentView::new(&bundle)
            .render(80)
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let before = rows.iter().position(|row| row.contains("before")).unwrap();
        let display = rows.iter().position(|row| row.contains("display")).unwrap();
        let after = rows.iter().position(|row| row.contains("after")).unwrap();
        let spaced = rows.iter().position(|row| row.contains("spaced")).unwrap();

        assert_eq!(display, before + 1);
        assert_eq!(after, display + 1);
        assert_eq!(spaced, after + 2);
    }

    #[test]
    fn table_cells_keep_equal_columns_and_independent_wrapping() {
        let mut bundle = bundle();
        let paragraph = |value: &str| Block::Paragraph {
            children: vec![Inline::Text {
                value: value.to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
            rows: vec![TableRow {
                cells: vec![
                    TableCell {
                        blocks: vec![paragraph("alpha beta gamma")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                    TableCell {
                        blocks: vec![paragraph("right hand")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        let rendered = DocumentView::new(&bundle).render(24);
        let rows = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        assert!(rows[1].starts_with("   alpha beta "));
        assert!(rows[1].contains("right hand"));
        assert!(rows[2].starts_with("   gamma"));
        assert_eq!(UnicodeWidthStr::width(rows[1].as_str()), 24);
        let left_match = rendered.search("alpha beta gamma");
        assert_eq!(left_match.len(), 1);
        assert_eq!(left_match[0].row, 1);
        assert_eq!(left_match[0].additional_fragments[0].row, 2);
        assert_eq!(rendered.search("right hand").len(), 1);
    }

    #[test]
    fn thematic_breaks_fill_the_remaining_content_width() {
        let rows = wrap_line(&LogicalLine::rule(3), 12);

        assert_eq!(rows[0].to_string(), "   ─────────");
    }

    #[test]
    fn rendered_search_finds_literal_options_and_decorates_every_match() {
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: "Use --acls, then repeat --acls.".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }];
        let rendered = DocumentView::new(&bundle).render(42);

        let matches = rendered.search("--ACLS");
        let highlighted = rendered.highlighted_text(&matches, Some(1));

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].row, 1);
        assert!(
            highlighted.lines[1]
                .spans
                .iter()
                .any(|span| span.style.bg == Some(theme::SEARCH_MATCH))
        );
        assert!(
            highlighted.lines[1]
                .spans
                .iter()
                .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
        );
    }

    #[test]
    fn case_folding_maps_expanding_unicode_back_to_the_source_character() {
        let rendered = RenderedDocument {
            text: Text::from(Line::from("İstanbul")),
            row_count: 1,
            anchor_rows: HashMap::new(),
            links: Vec::new(),
            search_records: vec![RenderedSearchRecord {
                text: "İstanbul".to_owned(),
                cells: "İstanbul"
                    .char_indices()
                    .scan(0, |column, (source_start, character)| {
                        let start_column = *column;
                        *column += character.width().unwrap_or(0);
                        Some(RenderedSearchSourceCell {
                            source_start,
                            source_end: source_start + character.len_utf8(),
                            fragment: RenderedSearchFragment {
                                row: 0,
                                start_column,
                                end_column: *column,
                            },
                        })
                    })
                    .collect(),
            }],
        };

        assert_eq!(
            rendered.search("i"),
            vec![RenderedSearchMatch {
                row: 0,
                start_column: 0,
                end_column: 1,
                additional_fragments: Vec::new(),
            }]
        );
    }

    #[test]
    fn section_reference_hit_regions_follow_wrapped_link_text() {
        let mut bundle = bundle();
        let document = bundle.document.as_mut().expect("document");
        document.sections[0].blocks = vec![Block::Paragraph {
            children: vec![
                Inline::Text {
                    value: "Read ".to_owned(),
                },
                Inline::SectionReference {
                    target: "details".to_owned(),
                    children: vec![Inline::Text {
                        value: "the detailed section".to_owned(),
                    }],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];
        document.sections[0].children.push(Section {
            id: "details".to_owned(),
            title: "Details".to_owned(),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: Vec::new(),
            source: None,
        });

        let rendered = DocumentView::new(&bundle).render(12);
        let regions = rendered
            .links
            .iter()
            .filter(|link| link.target == "details")
            .collect::<Vec<_>>();

        assert!(regions.len() >= 2, "reference should wrap across rows");
        for region in regions {
            assert_eq!(
                rendered.link_target_at(region.row, region.start_column),
                Some("details")
            );
        }
    }

    #[test]
    fn search_matches_one_logical_phrase_across_soft_wrapping() {
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: "alpha searchable phrase omega".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }];
        let rendered = DocumentView::new(&bundle).render(15);

        let matches = rendered.search("searchable phrase");
        let highlighted = rendered.highlighted_text(&matches, Some(0));

        assert_eq!(matches.len(), 1);
        assert!(!matches[0].additional_fragments.is_empty());
        let highlighted_rows = highlighted
            .lines
            .iter()
            .filter(|line| {
                line.spans
                    .iter()
                    .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
            })
            .count();
        assert_eq!(highlighted_rows, 2);
    }

    #[test]
    fn search_preserves_a_space_wrapped_exactly_after_the_row_boundary() {
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: "Relative inset end".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }];
        let rendered = DocumentView::new(&bundle).render(11);

        let matches = rendered.search("Relative inset end");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].additional_fragments.len(), 2);
    }

    #[test]
    fn character_wrapped_code_remains_contiguous_for_search() {
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks =
            vec![Block::Preformatted {
                children: vec![Inline::Text {
                    value: "abcdefghijklmnop".to_owned(),
                }],
                language: None,
                layout: LayoutHint::default(),
                source: None,
            }];
        let rendered = DocumentView::new(&bundle).render(10);

        let matches = rendered.search("ghijkl");

        assert_eq!(matches.len(), 1);
        assert!(!matches[0].additional_fragments.is_empty());
    }

    #[test]
    fn forced_word_splitting_does_not_insert_a_search_space() {
        let mut bundle = bundle();
        bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: "supercalifragilistic".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }];
        let rendered = DocumentView::new(&bundle).render(10);

        assert_eq!(rendered.search("fragilistic").len(), 1);
    }
}
