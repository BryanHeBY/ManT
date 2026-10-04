//! Width-independent logical rows produced from the document IR.

use std::sync::Arc;

use mant_ir::{DocumentAddress, TableAlignment, TableRuleCellKind};
use ratatui::{style::Style, text::Span};

/// External URI that passed `ManT`'s host-activation policy.
///
/// Construction accepts only structurally valid absolute HTTP/HTTPS targets
/// and mailto targets, at most 4096 bytes. Keeping the activation allowlist in
/// this type while reusing the IR validator prevents producers and consumers
/// from drifting apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalUri(String);

impl ExternalUri {
    /// Validate one untrusted external URI for host activation.
    #[must_use]
    pub fn parse(uri: &str) -> Option<Self> {
        if uri.is_empty()
            || uri.len() > 4096
            || uri
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return None;
        }
        let (scheme, _) = uri.split_once(':')?;
        if !["https", "http", "mailto"]
            .iter()
            .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
        {
            return None;
        }
        mant_ir::is_valid_external_uri(uri).then(|| Self(uri.to_owned()))
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
    Manual {
        name: String,
        manual_section: Option<String>,
    },
    External(ExternalUri),
}

#[derive(Debug, Clone)]
pub(super) struct LogicalLine {
    pub(super) indent: usize,
    pub(super) continuation_indent: usize,
    /// Generated row-hint padding, separate from structural list indentation.
    pub(super) layout_padding: usize,
    pub(super) continuation_layout_padding: usize,
    /// Hint-generated gaps inside a marker or shared HEAD/BODY visual row.
    pub(super) layout_scalars: Vec<std::ops::Range<usize>>,
    pub(super) spans: Vec<Span<'static>>,
    pub(super) surface: LineSurface,
    pub(super) wrap_mode: WrapMode,
    pub(super) table_row: Option<LogicalTableRow>,
    pub(super) links: Vec<LogicalLinkRange>,
    pub(super) reference_marks: Vec<ReferenceMark>,
}

#[derive(Debug, Clone)]
pub(super) struct ReferenceMark {
    pub(super) id: Arc<str>,
    /// Scalar offset in this logical row, before tabs expand into cells.
    pub(super) scalar_offset: usize,
}

#[derive(Debug, Clone)]
pub(super) struct LogicalLinkRange {
    pub(super) target: LinkTarget,
    /// Source scalar range, before complete-row grapheme shaping and tab expansion.
    pub(super) start_scalar: usize,
    pub(super) end_scalar: usize,
}

#[derive(Debug, Clone)]
pub(super) struct LogicalTableCell {
    pub(super) lines: Vec<LogicalLine>,
    pub(super) alignment: TableAlignment,
    pub(super) anchors: std::collections::HashMap<String, usize>,
    pub(super) completed_tail: bool,
    pub(super) break_after: bool,
    pub(super) empty_boundary: bool,
    pub(super) origin_bounds: mant_ir::geometry::CellOriginBounds,
}

#[derive(Debug, Clone)]
pub(super) struct LogicalTableRow {
    pub(super) cells: Vec<LogicalTableCell>,
    pub(super) rules: Option<Vec<TableRuleCellKind>>,
    pub(super) completed_tail: bool,
    pub(super) open_tail: bool,
    pub(super) layout: Arc<LogicalTableLayout>,
}

#[derive(Debug)]
pub(super) struct LogicalTableLayout {
    pub(super) preferred_widths: Vec<usize>,
    pub(super) force_stack: bool,
    /// Declared or actual-origin cells retain a provisional final row;
    /// generic measured projection retires that row even on bounded fallback.
    pub(super) retain_open_tail: bool,
    /// Complete source-neutral geometry, including content-derived gaps.
    pub(super) column_preferences: mant_ir::ColumnPreferences,
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
        Self {
            preferred_widths,
            force_stack: false,
            retain_open_tail: false,
            column_preferences: mant_ir::ColumnPreferences::default(),
        }
    }

    pub(super) fn requires_stack(&self, cells: &[LogicalTableCell]) -> bool {
        self.force_stack
            || (self.column_preferences.widths.is_empty()
                && cells
                    .iter()
                    .take(cells.len().saturating_sub(1))
                    .any(|cell| cell.break_after))
    }

    fn preferred_width(&self) -> usize {
        self.preferred_widths.iter().sum::<usize>()
            + self.preferred_widths.len().saturating_sub(1)
                * usize::from(self.column_preferences.gap_columns)
    }
}

impl LogicalTableCell {
    pub(super) fn has_open_tail(&self) -> bool {
        !self.completed_tail
            && self.lines.last().is_some_and(|line| {
                if let Some(row) = &line.table_row {
                    row.open_tail
                } else {
                    self.lines.len() > 1 && line.spans.iter().all(|span| span.content.is_empty())
                }
            })
    }

    fn reading_line_count(&self) -> usize {
        self.lines
            .len()
            .saturating_sub(usize::from(self.has_open_tail()))
            .max(1)
    }

    fn reading_completed_tail(&self) -> bool {
        self.completed_tail
            || self
                .lines
                .get(self.reading_line_count().saturating_sub(1))
                .is_some_and(|line| {
                    line.table_row.is_none()
                        && line.spans.iter().all(|span| span.content.is_empty())
                })
    }

    pub(super) fn new(lines: Vec<LogicalLine>, alignment: Option<TableAlignment>) -> Self {
        Self {
            lines,
            alignment: alignment.unwrap_or(TableAlignment::Left),
            anchors: std::collections::HashMap::new(),
            completed_tail: false,
            break_after: false,
            empty_boundary: false,
            origin_bounds: mant_ir::geometry::CellOriginBounds::default(),
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
    /// Presentation origin of this hard row, relative to its inline root.
    /// Padding stays outside source scalar, link, and reference coordinates.
    pub(super) indent_columns: i32,
    pub(super) spans: Vec<Span<'static>>,
    pub(super) links: Vec<LogicalLinkRange>,
    pub(super) reference_marks: Vec<ReferenceMark>,
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
    DoubleRule,
}

impl LogicalLine {
    /// Compose signed owner origins once, then bound display padding. Retain
    /// only the additional padding supplied by a hint for visual copy mapping.
    pub(super) fn row_geometry(
        first: i32,
        continuation: i32,
        correction: i32,
        spans: Vec<Span<'static>>,
    ) -> Self {
        let origins = mant_ir::resolve_row_origins(first, continuation, correction);
        let mut line = Self::hanging(
            mant_ir::geometry::padding(origins.first_visual_origin),
            mant_ir::geometry::padding(origins.continuation_origin),
            spans,
        );
        line.layout_padding = line
            .indent
            .saturating_sub(mant_ir::geometry::padding(first));
        line.continuation_layout_padding = line
            .continuation_indent
            .saturating_sub(mant_ir::geometry::padding(continuation));
        line
    }

    pub(super) fn empty() -> Self {
        Self {
            indent: 0,
            continuation_indent: 0,
            layout_padding: 0,
            continuation_layout_padding: 0,
            layout_scalars: Vec::new(),
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: None,
            links: Vec::new(),
            reference_marks: Vec::new(),
        }
    }

    pub(super) fn plain(indent: usize, value: impl Into<String>, style: Style) -> Self {
        Self {
            indent,
            continuation_indent: indent,
            layout_padding: 0,
            continuation_layout_padding: 0,
            layout_scalars: Vec::new(),
            spans: vec![Span::styled(value.into(), style)],
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: None,
            links: Vec::new(),
            reference_marks: Vec::new(),
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

    pub(super) fn with_reference_marks(mut self, marks: Vec<ReferenceMark>) -> Self {
        self.reference_marks = marks;
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
            layout_padding: 0,
            continuation_layout_padding: 0,
            layout_scalars: Vec::new(),
            spans,
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: None,
            links: Vec::new(),
            reference_marks: Vec::new(),
        }
    }

    pub(super) fn table(
        indent: usize,
        cells: Vec<LogicalTableCell>,
        layout: Arc<LogicalTableLayout>,
    ) -> Self {
        let sequential =
            layout.requires_stack(&cells) || !layout.column_preferences.widths.is_empty();
        let completed_tail = if cells.is_empty() {
            true
        } else if sequential {
            cells.last().is_some_and(|cell| cell.completed_tail)
        } else {
            let height = cells
                .iter()
                .map(LogicalTableCell::reading_line_count)
                .max()
                .unwrap_or(0);
            cells
                .iter()
                .any(|cell| cell.reading_completed_tail() && cell.reading_line_count() == height)
        };
        let open_tail = !completed_tail
            && sequential
            && layout.retain_open_tail
            && cells
                .iter()
                .rev()
                .find(|cell| !cell.lines.is_empty())
                .is_some_and(LogicalTableCell::has_open_tail);
        Self {
            indent,
            continuation_indent: indent,
            layout_padding: 0,
            continuation_layout_padding: 0,
            layout_scalars: Vec::new(),
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: Some(LogicalTableRow {
                cells,
                completed_tail,
                open_tail,
                rules: None,
                layout,
            }),
            links: Vec::new(),
            reference_marks: Vec::new(),
        }
    }

    pub(super) fn table_rule(
        indent: usize,
        rules: Vec<TableRuleCellKind>,
        layout: Arc<LogicalTableLayout>,
    ) -> Self {
        Self {
            indent,
            continuation_indent: indent,
            layout_padding: 0,
            continuation_layout_padding: 0,
            layout_scalars: Vec::new(),
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: Some(LogicalTableRow {
                cells: Vec::new(),
                completed_tail: false,
                open_tail: false,
                rules: Some(rules),
                layout,
            }),
            links: Vec::new(),
            reference_marks: Vec::new(),
        }
    }

    pub(super) fn rule(indent: usize) -> Self {
        let mut line = Self::empty();
        line.indent = indent;
        line.continuation_indent = indent;
        line.surface = LineSurface::Rule;
        line
    }

    pub(super) fn double_rule(indent: usize) -> Self {
        let mut line = Self::empty();
        line.indent = indent;
        line.continuation_indent = indent;
        line.surface = LineSurface::DoubleRule;
        line
    }

    fn preferred_width(&self) -> usize {
        let content = self.table_row.as_ref().map_or_else(
            || super::inline::spans_width(&self.spans),
            |table| table.layout.preferred_width(),
        );
        self.indent.saturating_add(content)
    }
}
