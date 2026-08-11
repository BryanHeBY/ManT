//! Width-independent logical rows produced from the document AST.

use ratatui::{style::Style, text::Span};

#[derive(Debug, Clone)]
pub(super) struct LogicalLine {
    pub(super) indent: usize,
    pub(super) continuation_indent: usize,
    pub(super) spans: Vec<Span<'static>>,
    pub(super) surface: LineSurface,
    pub(super) wrap_mode: WrapMode,
    pub(super) table_cells: Option<Vec<Vec<LogicalLine>>>,
    pub(super) links: Vec<LogicalLinkRange>,
}

#[derive(Debug, Clone)]
pub(super) struct LogicalLinkRange {
    pub(super) target: String,
    pub(super) start_column: usize,
    pub(super) end_column: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct StyledInlineLine {
    pub(super) spans: Vec<Span<'static>>,
    pub(super) links: Vec<LogicalLinkRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WrapMode {
    Word,
    Character,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LineSurface {
    Normal,
    Code,
    Tldr,
    TldrTop,
    TldrBottom,
    Divider,
    Rule,
}

impl LogicalLine {
    pub(super) fn empty() -> Self {
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

    pub(super) fn plain(indent: usize, value: impl Into<String>, style: Style) -> Self {
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

    pub(super) fn surface(mut self, surface: LineSurface) -> Self {
        self.surface = surface;
        self
    }

    pub(super) fn wrap_mode(mut self, wrap_mode: WrapMode) -> Self {
        self.wrap_mode = wrap_mode;
        self
    }

    pub(super) fn with_links(mut self, links: Vec<LogicalLinkRange>) -> Self {
        self.links = links;
        self
    }

    pub(super) fn hanging(
        indent: usize,
        continuation_indent: usize,
        spans: Vec<Span<'static>>,
    ) -> Self {
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

    pub(super) fn table(indent: usize, cells: Vec<Vec<Self>>) -> Self {
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

    pub(super) fn rule(indent: usize) -> Self {
        let mut line = Self::empty();
        line.indent = indent;
        line.continuation_indent = indent;
        line.surface = LineSurface::Rule;
        line
    }
}
