//! Width-independent logical rows produced from the document IR.

use std::sync::Arc;

use mant_ir::{DocumentAddress, TableRuleCellKind};
use ratatui::{style::Style, text::Span};

mod table;
pub(super) use table::{
    LogicalTableCell, LogicalTableLayout, LogicalTableRow, LogicalTableRowKind, TableCellOccupancy,
    TableRails,
};

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
    pub(super) alignment: mant_ir::TextAlignment,
    /// A structural marker stays at its origin while its content aligns.
    pub(super) alignment_prefix_scalars: usize,
    /// Cell preparation defers viewport padding until its final slot is known.
    pub(super) defer_alignment: bool,
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

#[derive(Debug, Clone, Default)]
pub(super) struct StyledInlineLine {
    /// Presentation origin of this hard row, relative to its inline root.
    /// Padding stays outside source scalar, link, and reference coordinates.
    pub(super) indent_columns: i32,
    pub(super) first_line_indent_columns: i32,
    pub(super) alignment: mant_ir::TextAlignment,
    pub(super) spans: Vec<Span<'static>>,
    pub(super) links: Vec<LogicalLinkRange>,
    pub(super) reference_marks: Vec<ReferenceMark>,
}

impl StyledInlineLine {
    pub(super) fn row_hint(&self) -> mant_ir::RowLayoutHint {
        mant_ir::RowLayoutHint {
            row: 0,
            indent_columns: self.indent_columns,
            first_line_indent_columns: self.first_line_indent_columns,
            alignment: self.alignment,
        }
    }
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
    pub(super) fn row_presentation(
        first: i32,
        continuation: i32,
        hint: mant_ir::RowLayoutHint,
        spans: Vec<Span<'static>>,
    ) -> Self {
        let origins = hint.resolve(first, continuation);
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
        line.alignment = hint.alignment;
        line
    }

    pub(super) fn empty() -> Self {
        Self {
            alignment: mant_ir::TextAlignment::Left,
            alignment_prefix_scalars: 0,
            defer_alignment: false,
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
            alignment: mant_ir::TextAlignment::Left,
            alignment_prefix_scalars: 0,
            defer_alignment: false,
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
            alignment: mant_ir::TextAlignment::Left,
            alignment_prefix_scalars: 0,
            defer_alignment: false,
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
            alignment: mant_ir::TextAlignment::Left,
            alignment_prefix_scalars: 0,
            defer_alignment: false,
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
                kind: LogicalTableRowKind::Data,
                layout,
                column_borders: Vec::new(),
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
            alignment: mant_ir::TextAlignment::Left,
            alignment_prefix_scalars: 0,
            defer_alignment: false,
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
                kind: LogicalTableRowKind::ColumnRules(rules),
                layout,
                column_borders: Vec::new(),
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
