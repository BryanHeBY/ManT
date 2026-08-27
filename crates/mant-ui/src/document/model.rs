//! Width-independent logical rows produced from the document IR.

use std::sync::Arc;

use mant_ir::{DocumentAddress, TableAlignment};
use ratatui::{style::Style, text::Span};
use unicode_width::UnicodeWidthStr;

/// External URI that passed `ManT`'s host-activation policy.
///
/// Construction accepts only non-empty HTTP, HTTPS, and mailto targets of at
/// most 4096 bytes without control characters. Keeping validation in this
/// type prevents a new document producer from bypassing the activation gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalUri(String);

impl ExternalUri {
    /// Validate one untrusted external URI for host activation.
    #[must_use]
    pub fn parse(uri: &str) -> Option<Self> {
        if uri.is_empty() || uri.len() > 4096 || uri.chars().any(char::is_control) {
            return None;
        }
        let (scheme, target) = uri.split_once(':')?;
        if !["https", "http", "mailto"]
            .iter()
            .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
        {
            return None;
        }
        (!target.trim_start_matches('/').is_empty()).then(|| Self(uri.to_owned()))
    }

    /// Return the validated URI spelling supplied by the document.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkTarget {
    Section(String),
    Document {
        address: DocumentAddress,
        fragment: Option<String>,
    },
    External(ExternalUri),
}

#[derive(Debug, Clone)]
pub(super) struct LogicalLine {
    pub(super) indent: usize,
    pub(super) continuation_indent: usize,
    pub(super) spans: Vec<Span<'static>>,
    pub(super) surface: LineSurface,
    pub(super) wrap_mode: WrapMode,
    pub(super) table_row: Option<LogicalTableRow>,
    pub(super) links: Vec<LogicalLinkRange>,
}

#[derive(Debug, Clone)]
pub(super) struct LogicalLinkRange {
    pub(super) target: LinkTarget,
    pub(super) start_column: usize,
    pub(super) end_column: usize,
}

#[derive(Debug, Clone)]
pub(super) struct LogicalTableCell {
    pub(super) lines: Vec<LogicalLine>,
    pub(super) alignment: TableAlignment,
}

#[derive(Debug, Clone)]
pub(super) struct LogicalTableRow {
    pub(super) cells: Vec<LogicalTableCell>,
    pub(super) layout: Arc<LogicalTableLayout>,
}

#[derive(Debug)]
pub(super) struct LogicalTableLayout {
    pub(super) preferred_widths: Vec<usize>,
}

impl LogicalTableLayout {
    pub(super) fn for_rows(rows: &[Vec<LogicalTableCell>]) -> Self {
        let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
        let preferred_widths = (0..column_count)
            .map(|column| {
                rows.iter()
                    .filter_map(|row| row.get(column))
                    .map(LogicalTableCell::preferred_width)
                    .max()
                    .unwrap_or(1)
                    .max(1)
            })
            .collect();
        Self { preferred_widths }
    }

    fn preferred_width(&self) -> usize {
        self.preferred_widths.iter().sum::<usize>()
            + self.preferred_widths.len().saturating_sub(1) * 2
    }
}

impl LogicalTableCell {
    pub(super) fn new(lines: Vec<LogicalLine>, alignment: Option<TableAlignment>) -> Self {
        Self {
            lines,
            alignment: alignment.unwrap_or(TableAlignment::Left),
        }
    }

    fn preferred_width(&self) -> usize {
        self.lines
            .iter()
            .map(LogicalLine::preferred_width)
            .max()
            .unwrap_or(1)
            .max(1)
    }
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
            table_row: None,
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
            table_row: None,
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
            table_row: None,
            links: Vec::new(),
        }
    }

    pub(super) fn table(
        indent: usize,
        cells: Vec<LogicalTableCell>,
        layout: Arc<LogicalTableLayout>,
    ) -> Self {
        Self {
            indent,
            continuation_indent: indent,
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: Some(LogicalTableRow { cells, layout }),
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

    fn preferred_width(&self) -> usize {
        let content = self.table_row.as_ref().map_or_else(
            || {
                self.spans
                    .iter()
                    .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                    .sum()
            },
            |table| table.layout.preferred_width(),
        );
        self.indent.saturating_add(content)
    }
}
