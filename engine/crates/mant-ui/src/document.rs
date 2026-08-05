//! Lowers a `ManT` query into width-aware terminal lines and stable anchors.
//!
//! This module owns wrapping instead of delegating it to a widget. As a result,
//! section navigation, scroll synchronization, links, and future search ranges
//! can all address the exact rows that Ratatui renders.

use std::collections::HashMap;

use mant_ast::{
    Block, Inline, ListKind, QueryBundle, Section, TableCell, TldrCommandPart, TldrOrigin,
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
    logical_line: usize,
}

/// Renderer-independent terminal view before width-dependent wrapping.
#[derive(Debug, Clone)]
pub struct DocumentView {
    label: String,
    lines: Vec<LogicalLine>,
    navigation: Vec<NavItem>,
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
}

impl LogicalLine {
    fn empty() -> Self {
        Self {
            indent: 0,
            spans: Vec::new(),
        }
    }

    fn plain(indent: usize, value: impl Into<String>, style: Style) -> Self {
        Self {
            indent,
            spans: vec![Span::styled(value.into(), style)],
        }
    }
}

impl DocumentView {
    /// Build one immutable view from the normalized query contract.
    #[must_use]
    pub fn new(bundle: &QueryBundle) -> Self {
        let mut builder = DocumentBuilder::new(bundle.label.clone());

        if let Some(tldr) = &bundle.tldr {
            builder.anchor(TLDR_ID, "TLDR QUICK REFERENCE", 0);
            builder.push(LogicalLine::plain(
                0,
                format!("TLDR QUICK REFERENCE · {}", tldr.title),
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            ));
            for description in &tldr.description {
                builder.push(LogicalLine::plain(
                    0,
                    description.clone(),
                    Style::default().fg(theme::TEXT),
                ));
            }
            for example in &tldr.examples {
                builder.blank();
                builder.push(LogicalLine::plain(
                    0,
                    example.description.clone(),
                    Style::default().fg(theme::GREEN),
                ));
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
                builder.push(LogicalLine { indent: 2, spans });
            }
            if let Some(link) = &tldr.more_information {
                builder.blank();
                builder.push(LogicalLine::plain(
                    0,
                    format!("More information: {link}"),
                    Style::default()
                        .fg(theme::BLUE)
                        .add_modifier(Modifier::UNDERLINED),
                ));
            }
            if tldr.origin == TldrOrigin::TldrPages {
                builder.push(LogicalLine::plain(
                    0,
                    format!(
                        "tldr-pages · CC BY 4.0 · {} · {}",
                        tldr.platform, tldr.language
                    ),
                    Style::default().fg(theme::SUBTEXT),
                ));
            }
            builder.blank();
        }

        if let Some(document) = &bundle.document {
            if !document.blocks.is_empty() {
                builder.anchor(ROOT_ID, "OVERVIEW", 0);
                builder.blocks(&document.blocks, 0);
            }
            for section in &document.sections {
                builder.section(section, 0);
            }
        }

        Self {
            label: builder.label,
            lines: builder.lines,
            navigation: builder.navigation,
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
            .navigation
            .iter()
            .map(|item| {
                (
                    item.id.clone(),
                    logical_rows
                        .get(item.logical_line)
                        .copied()
                        .unwrap_or_default(),
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
}

impl DocumentBuilder {
    fn new(label: String) -> Self {
        Self {
            label,
            lines: Vec::new(),
            navigation: Vec::new(),
        }
    }

    fn push(&mut self, line: LogicalLine) {
        self.lines.push(line);
    }

    fn blank(&mut self) {
        if self.lines.last().is_none_or(|line| !line.spans.is_empty()) {
            self.lines.push(LogicalLine::empty());
        }
    }

    fn anchor(&mut self, id: &str, title: &str, depth: usize) {
        self.navigation.push(NavItem {
            id: id.to_owned(),
            title: title.to_owned(),
            depth,
            logical_line: self.lines.len(),
        });
    }

    fn section(&mut self, section: &Section, depth: usize) {
        for _ in 0..section.spacing_before_lines {
            self.blank();
        }
        self.anchor(&section.id, &section.title, depth);
        self.push(LogicalLine::plain(
            depth * 2,
            section.title.clone(),
            Style::default()
                .fg(theme::HEADING)
                .add_modifier(Modifier::BOLD),
        ));
        self.blocks(&section.blocks, depth * 2 + 2);
        for child in &section.children {
            self.section(child, depth + 1);
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
                self.inline_lines(
                    children,
                    base_indent + usize::from(layout.indent_columns),
                    Style::default().fg(theme::TEXT).bg(theme::SURFACE),
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
        let mut lines = vec![Vec::new()];
        append_inline(nodes, base_style, &mut lines);
        for spans in lines {
            self.push(LogicalLine { indent, spans });
        }
    }
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
    let indent = line.indent.min(width.saturating_sub(1));
    let available = width.saturating_sub(indent).max(1);
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
        return vec![Line::default()];
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
                result.push(cells_to_line(indent, &row));
                while continuation
                    .first()
                    .is_some_and(|item| item.character.is_whitespace())
                {
                    continuation.remove(0);
                }
                row = continuation;
                row_width = row.iter().map(|item| item.width).sum();
            } else {
                result.push(cells_to_line(indent, &row));
                row.clear();
                row_width = 0;
            }
        }
        row_width += cell.width;
        row.push(cell);
    }
    if !row.is_empty() {
        result.push(cells_to_line(indent, &row));
    }
    result
}

fn cells_to_line(indent: usize, cells: &[StyledCell]) -> Line<'static> {
    if cells.is_empty() {
        return Line::from(" ".repeat(indent));
    }
    let mut spans = Vec::new();
    if indent > 0 {
        spans.push(Span::raw(" ".repeat(indent)));
    }
    let mut current_style = cells[0].style;
    let mut value = String::new();
    for cell in cells {
        if cell.style != current_style {
            spans.push(Span::styled(std::mem::take(&mut value), current_style));
            current_style = cell.style;
        }
        value.push(cell.character);
    }
    spans.push(Span::styled(value, current_style));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use mant_ast::{
        DocumentMeta, DocumentSchema, DocumentSource, LayoutHint, MantDocument, Producer,
        QuerySchema, SourceFormat,
    };

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
}
