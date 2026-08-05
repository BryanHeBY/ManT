//! Lowers a `ManT` query into width-aware terminal lines and stable anchors.
//!
//! This module owns wrapping instead of delegating it to a widget. As a result,
//! section navigation, scroll synchronization, links, and future search ranges
//! can all address the exact rows that Ratatui renders.

use std::collections::HashMap;

use mant_ast::{
    Block, Inline, ListKind, QueryBundle, Section, SourceFormat, TableCell, TldrCommandPart,
    TldrDocument, TldrOrigin,
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span, Text},
};
use unicode_width::UnicodeWidthChar;

use crate::theme;

const TLDR_ID: &str = "tldr";
const ROOT_ID: &str = "document-root";

/// One addressable entry displayed in the navigation sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    pub id: String,
    pub title: String,
    pub depth: usize,
    pub kind: NavKind,
    pub has_children: bool,
    pub is_last: bool,
}

/// Semantic presentation class for a navigation entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKind {
    Tldr,
    Root,
    Section,
}

/// Renderer-independent terminal view before width-dependent wrapping.
#[derive(Debug, Clone)]
pub struct DocumentView {
    label: String,
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
}

#[derive(Debug, Clone)]
struct LogicalLine {
    indent: usize,
    spans: Vec<Span<'static>>,
    surface: LineSurface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineSurface {
    Normal,
    Code,
    Tldr,
    TldrTop,
    TldrBottom,
    Divider,
}

impl LogicalLine {
    fn empty() -> Self {
        Self {
            indent: 0,
            spans: Vec::new(),
            surface: LineSurface::Normal,
        }
    }

    fn plain(indent: usize, value: impl Into<String>, style: Style) -> Self {
        Self {
            indent,
            spans: vec![Span::styled(value.into(), style)],
            surface: LineSurface::Normal,
        }
    }

    fn surface(mut self, surface: LineSurface) -> Self {
        self.surface = surface;
        self
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
        let top_level_count = bundle.document.as_ref().map_or(0, |document| {
            document.sections.len() + usize::from(!document.blocks.is_empty())
        });
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
                );
                builder.blocks(&document.blocks, 0);
            }
            let section_count = document.sections.len();
            for (index, section) in document.sections.iter().enumerate() {
                builder.section_with_position(section, 0, index + 1 == section_count);
            }
        }

        Self {
            label: builder.label,
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
        let mut logical_rows = Vec::with_capacity(self.lines.len() + 1);

        for line in &self.lines {
            logical_rows.push(rows.len());
            rows.extend(wrap_line(line, width));
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
        }
    }
}

impl RenderedDocument {
    #[must_use]
    pub fn anchor_row(&self, id: &str) -> Option<usize> {
        self.anchor_rows.get(id).copied()
    }
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
            spans,
            surface: LineSurface::Tldr,
        });
    }

    fn blank(&mut self) {
        if self.lines.last().is_none_or(|line| !line.spans.is_empty()) {
            self.lines.push(LogicalLine::empty());
        }
    }

    fn anchor(
        &mut self,
        id: &str,
        title: &str,
        depth: usize,
        kind: NavKind,
        has_children: bool,
        is_last: bool,
    ) {
        self.anchors.insert(id.to_owned(), self.lines.len());
        self.navigation.push(NavItem {
            id: id.to_owned(),
            title: title.to_owned(),
            depth,
            kind,
            has_children,
            is_last,
        });
    }

    fn section_with_position(&mut self, section: &Section, depth: usize, is_last: bool) {
        for _ in 0..section.spacing_before_lines {
            self.blank();
        }
        self.anchor(
            &section.id,
            &section.title,
            depth,
            NavKind::Section,
            !section.children.is_empty(),
            is_last,
        );
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
            self.section_with_position(child, depth + 1, index + 1 == child_count);
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
                        self.blank();
                    }
                    let marker = match kind {
                        ListKind::Bullet => "• ".to_owned(),
                        ListKind::Ordered => format!("{}. ", start.unwrap_or(1) + index as u64),
                        ListKind::Plain => String::new(),
                    };
                    let has_marker = !marker.is_empty();
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
                    for term in &item.terms {
                        self.inline_lines(term, indent, Style::default().fg(theme::TEXT));
                    }
                    self.blocks(
                        &item.description,
                        indent + usize::from(!item.inline_term) * 4,
                    );
                }
            }
            Block::Table { rows, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                let indent = base_indent + usize::from(layout.indent_columns);
                for row in rows {
                    let value = row
                        .cells
                        .iter()
                        .map(cell_text)
                        .collect::<Vec<_>>()
                        .join("  ");
                    self.push(LogicalLine::plain(
                        indent,
                        value,
                        Style::default().fg(theme::TEXT),
                    ));
                }
            }
            Block::Equation { value, layout, .. } => {
                self.spacing(layout.spacing_before_lines);
                self.push(LogicalLine::plain(
                    base_indent + usize::from(layout.indent_columns),
                    value.clone(),
                    Style::default().fg(theme::YELLOW),
                ));
            }
            Block::VerticalSpace { lines, .. } => self.spacing(*lines),
            Block::ThematicBreak { .. } => self.push(LogicalLine::plain(
                base_indent,
                "─".repeat(12),
                Style::default().fg(theme::OVERLAY),
            )),
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
        let mut lines = vec![Vec::new()];
        append_inline(nodes, base_style, &mut lines);
        for spans in lines {
            self.push(LogicalLine {
                indent,
                spans: if surface == LineSurface::Code {
                    crate::code::highlight(spans)
                } else {
                    spans
                },
                surface,
            });
        }
    }
}

fn count_sections(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|section| 1 + count_sections(&section.children))
        .sum()
}

fn append_inline(nodes: &[Inline], style: Style, lines: &mut Vec<Vec<Span<'static>>>) {
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
            Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => {
                append_inline(
                    children,
                    Style::default()
                        .fg(theme::LINK)
                        .add_modifier(Modifier::UNDERLINED),
                    lines,
                );
            }
            Inline::Anchor { .. } => {}
            Inline::LineBreak => lines.push(Vec::new()),
        }
    }
}

fn append_text(value: &str, style: Style, lines: &mut Vec<Vec<Span<'static>>>) {
    for (index, part) in value.split('\n').enumerate() {
        if index > 0 {
            lines.push(Vec::new());
        }
        if !part.is_empty() {
            lines
                .last_mut()
                .expect("inline builder always owns one line")
                .push(Span::styled(part.to_owned(), style));
        }
    }
}

fn cell_text(cell: &TableCell) -> String {
    cell.blocks
        .iter()
        .map(block_text)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            inline_text(children)
        }
        Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => value.clone(),
        _ => String::new(),
    }
}

fn inline_text(nodes: &[Inline]) -> String {
    let mut value = String::new();
    for node in nodes {
        match node {
            Inline::Text { value: text } | Inline::Code { value: text } => value.push_str(text),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::ExternalLink { children, .. }
            | Inline::EmailLink { children, .. }
            | Inline::ManualReference { children, .. }
            | Inline::SectionReference { children, .. } => value.push_str(&inline_text(children)),
            Inline::LineBreak => value.push(' '),
            Inline::Anchor { .. } => {}
        }
    }
    value
}

#[derive(Clone, Copy)]
struct StyledCell {
    character: char,
    width: usize,
    style: Style,
}

fn wrap_line(line: &LogicalLine, width: usize) -> Vec<Line<'static>> {
    match line.surface {
        LineSurface::TldrTop => return vec![panel_border(width, '┌', '┐')],
        LineSurface::TldrBottom => return vec![panel_border(width, '└', '┘')],
        LineSurface::Divider => {
            return vec![Line::from(Span::styled(
                "─".repeat(width),
                Style::default().fg(theme::OVERLAY),
            ))];
        }
        LineSurface::Normal | LineSurface::Code | LineSurface::Tldr => {}
    }

    let indent = line.indent.min(width.saturating_sub(1));
    let decoration_width = usize::from(line.surface == LineSurface::Tldr) * 4;
    let available = width
        .saturating_sub(indent)
        .saturating_sub(decoration_width)
        .max(1);
    let cells = line
        .spans
        .iter()
        .flat_map(|span| {
            let style = span.style;
            span.content.chars().map(move |character| StyledCell {
                character,
                width: character.width().unwrap_or(0),
                style,
            })
        })
        .collect::<Vec<_>>();

    if cells.is_empty() {
        return vec![cells_to_line(line, width, indent, &[])];
    }

    let mut result = Vec::new();
    let mut row = Vec::new();
    let mut row_width = 0;
    for cell in cells {
        while row_width + cell.width > available && !row.is_empty() {
            if let Some(space) = row
                .iter()
                .rposition(|item: &StyledCell| item.character.is_whitespace())
            {
                let mut continuation = row.split_off(space + 1);
                row.pop();
                while row
                    .last()
                    .is_some_and(|item| item.character.is_whitespace())
                {
                    row.pop();
                }
                result.push(cells_to_line(line, width, indent, &row));
                while continuation
                    .first()
                    .is_some_and(|item| item.character.is_whitespace())
                {
                    continuation.remove(0);
                }
                row = continuation;
                row_width = row.iter().map(|item| item.width).sum();
            } else {
                result.push(cells_to_line(line, width, indent, &row));
                row.clear();
                row_width = 0;
            }
        }
        row_width += cell.width;
        row.push(cell);
    }
    if !row.is_empty() {
        result.push(cells_to_line(line, width, indent, &row));
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
        | LineSurface::Divider => None,
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
        DocumentMeta, DocumentSchema, DocumentSource, LayoutHint, MantDocument, Producer,
        QuerySchema, SourceFormat, TldrDocument, TldrExample,
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
}
