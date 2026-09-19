//! Width-independent logical rows produced from the document IR.

use std::sync::Arc;

use mant_ir::{DocumentAddress, TableAlignment, TableRuleCellKind};
use ratatui::{style::Style, text::Span};

use super::inline::{shifted_links, shifted_reference_marks, spans_scalars};

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
    /// Absolute terminal-column offset of this line's local coordinate space.
    pub(super) geometry_offset: usize,
    pub(super) indent: usize,
    pub(super) continuation_indent: usize,
    /// Optional scalar boundary after which wrapped rows use a second origin.
    ///
    /// Responsive definitions need the native term continuation while a row
    /// still starts inside the term, then the paragraph continuation once a
    /// wrapped row starts in the run-in description.
    pub(super) continuation_switch: Option<ContinuationSwitch>,
    pub(super) spans: Vec<Span<'static>>,
    pub(super) surface: LineSurface,
    pub(super) wrap_mode: WrapMode,
    pub(super) table_row: Option<LogicalTableRow>,
    pub(super) links: Vec<LogicalLinkRange>,
    pub(super) reference_marks: Vec<ReferenceMark>,
    /// Document-local anchors owned by the start of this exact logical row.
    pub(super) anchors: Vec<String>,
    /// Anchors inside a logical row, before tabs and wrapping are resolved.
    pub(super) positioned_anchors: Vec<PositionedAnchor>,
    pub(super) conditional_definition: Option<Box<ConditionalDefinitionLine>>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ContinuationSwitch {
    pub(super) scalar_offset: usize,
    pub(super) indent: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ConditionalDefinitionLine {
    pub(super) plan: mant_ir::geometry::DefinitionPlacementPlan,
    pub(super) term: LogicalLine,
    pub(super) description: Vec<LogicalLine>,
}

pub(super) enum ResolvedLogicalLines<'a> {
    Borrowed(&'a [LogicalLine]),
    Owned(Vec<LogicalLine>),
}

impl std::ops::Deref for ResolvedLogicalLines<'_> {
    type Target = [LogicalLine];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(lines) => lines,
            Self::Owned(lines) => lines,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ReferenceMark {
    pub(super) id: Arc<str>,
    /// Scalar offset in this logical row, before tabs expand into cells.
    pub(super) scalar_offset: usize,
}

#[derive(Debug, Clone)]
pub(super) struct PositionedAnchor {
    pub(super) id: String,
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
}

#[derive(Debug, Clone)]
pub(super) struct LogicalTableRow {
    pub(super) cells: Vec<LogicalTableCell>,
    pub(super) rules: Option<Vec<TableRuleCellKind>>,
    pub(super) layout: Arc<LogicalTableLayout>,
}

#[derive(Debug)]
pub(super) struct LogicalTableLayout {
    pub(super) preferred_widths: Vec<usize>,
    pub(super) force_stack: bool,
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
        }
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
            anchors: std::collections::HashMap::new(),
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
    pub(super) fn empty() -> Self {
        Self {
            indent: 0,
            geometry_offset: 0,
            continuation_indent: 0,
            continuation_switch: None,
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: None,
            links: Vec::new(),
            reference_marks: Vec::new(),
            anchors: Vec::new(),
            positioned_anchors: Vec::new(),
            conditional_definition: None,
        }
    }

    pub(super) fn plain(indent: usize, value: impl Into<String>, style: Style) -> Self {
        Self {
            indent,
            geometry_offset: 0,
            continuation_indent: indent,
            continuation_switch: None,
            spans: vec![Span::styled(value.into(), style)],
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: None,
            links: Vec::new(),
            reference_marks: Vec::new(),
            anchors: Vec::new(),
            positioned_anchors: Vec::new(),
            conditional_definition: None,
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

    pub(super) fn with_anchors(mut self, anchors: Vec<String>) -> Self {
        self.anchors = anchors;
        self
    }

    pub(super) fn conditional_definition(
        plan: mant_ir::geometry::DefinitionPlacementPlan,
        term: Self,
        description: Vec<Self>,
    ) -> Self {
        let mut line = Self::empty();
        line.indent = term.indent;
        line.continuation_indent = line.indent;
        line.conditional_definition = Some(Box::new(ConditionalDefinitionLine {
            plan,
            term,
            description,
        }));
        line
    }

    pub(super) fn resolved_lines(&self, width: usize) -> ResolvedLogicalLines<'_> {
        self.resolved_lines_at(width.saturating_sub(self.indent), self.geometry_offset)
    }

    pub(super) fn resolved_lines_at(
        &self,
        allocated_width: usize,
        geometry_offset: usize,
    ) -> ResolvedLogicalLines<'_> {
        let Some(conditional) = &self.conditional_definition else {
            return ResolvedLogicalLines::Borrowed(std::slice::from_ref(self));
        };
        let delta = i32::try_from(geometry_offset).unwrap_or(i32::MAX);
        let resolution = conditional
            .plan
            .translated(delta)
            .resolve(Some(allocated_width));
        let local = |origin: usize| origin.saturating_sub(geometry_offset);
        let mut term = conditional.term.clone();
        term.geometry_offset = geometry_offset;
        term.indent = self.indent;
        term.continuation_indent = local(resolution.term_continuation_origin_columns);
        let mut description = conditional.description.clone();
        for line in &mut description {
            line.geometry_offset = geometry_offset;
        }
        if resolution.run_in {
            if let Some(first) = description.first() {
                let term_width = resolution.final_label_width_columns.unwrap_or_default();
                let absolute_term_origin = self.indent.saturating_add(geometry_offset);
                let gap = resolution
                    .first_description_origin_columns
                    .saturating_sub(absolute_term_origin.saturating_add(term_width));
                term.spans.push(Span::raw(" ".repeat(gap)));
                let scalar_offset = spans_scalars(&term.spans);
                term.continuation_switch = Some(ContinuationSwitch {
                    scalar_offset,
                    indent: local(resolution.continuation_origin_columns),
                });
                term.links
                    .extend(shifted_links(first.links.clone(), scalar_offset));
                term.reference_marks.extend(shifted_reference_marks(
                    first.reference_marks.clone(),
                    scalar_offset,
                ));
                term.positioned_anchors.extend(
                    first
                        .anchors
                        .iter()
                        .cloned()
                        .map(|id| PositionedAnchor { id, scalar_offset }),
                );
                term.positioned_anchors
                    .extend(first.positioned_anchors.iter().cloned().map(|mut anchor| {
                        anchor.scalar_offset = anchor.scalar_offset.saturating_add(scalar_offset);
                        anchor
                    }));
                term.spans.extend(first.spans.clone());
            }
            for line in description.iter_mut().skip(1) {
                line.indent = local(resolution.continuation_origin_columns);
                line.continuation_indent = line.indent;
            }
            let mut lines = vec![term];
            lines.extend(description.into_iter().skip(1));
            ResolvedLogicalLines::Owned(lines)
        } else {
            for (row, line) in description.iter_mut().enumerate() {
                line.indent = local(if row == 0 {
                    resolution.stacked_description_origin_columns
                } else {
                    resolution.continuation_origin_columns
                });
                line.continuation_indent = local(resolution.continuation_origin_columns);
            }
            let mut lines = vec![term];
            lines.extend(description);
            ResolvedLogicalLines::Owned(lines)
        }
    }

    pub(super) fn shift_origin(&mut self, delta: usize) {
        self.indent = self.indent.saturating_add(delta);
        self.continuation_indent = self.continuation_indent.saturating_add(delta);
        if let Some(switch) = &mut self.continuation_switch {
            switch.indent = switch.indent.saturating_add(delta);
        }
        if let Some(conditional) = &mut self.conditional_definition {
            conditional.plan = conditional
                .plan
                .translated(i32::try_from(delta).unwrap_or(i32::MAX));
            conditional.term.shift_origin(delta);
            for line in &mut conditional.description {
                line.shift_origin(delta);
            }
        }
    }

    pub(super) fn shift_geometry_origin(&mut self, delta: usize) {
        self.geometry_offset = self.geometry_offset.saturating_add(delta);
    }

    pub(super) fn hanging(
        indent: usize,
        continuation_indent: usize,
        spans: Vec<Span<'static>>,
    ) -> Self {
        Self {
            geometry_offset: 0,
            indent,
            continuation_indent,
            continuation_switch: None,
            spans,
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: None,
            links: Vec::new(),
            reference_marks: Vec::new(),
            anchors: Vec::new(),
            positioned_anchors: Vec::new(),
            conditional_definition: None,
        }
    }

    pub(super) fn table(
        indent: usize,
        cells: Vec<LogicalTableCell>,
        layout: Arc<LogicalTableLayout>,
    ) -> Self {
        Self {
            geometry_offset: 0,
            indent,
            continuation_indent: indent,
            continuation_switch: None,
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: Some(LogicalTableRow {
                cells,
                rules: None,
                layout,
            }),
            links: Vec::new(),
            reference_marks: Vec::new(),
            anchors: Vec::new(),
            positioned_anchors: Vec::new(),
            conditional_definition: None,
        }
    }

    pub(super) fn table_rule(
        indent: usize,
        rules: Vec<TableRuleCellKind>,
        layout: Arc<LogicalTableLayout>,
    ) -> Self {
        Self {
            geometry_offset: 0,
            indent,
            continuation_indent: indent,
            continuation_switch: None,
            spans: Vec::new(),
            surface: LineSurface::Normal,
            wrap_mode: WrapMode::Word,
            table_row: Some(LogicalTableRow {
                cells: Vec::new(),
                rules: Some(rules),
                layout,
            }),
            links: Vec::new(),
            reference_marks: Vec::new(),
            anchors: Vec::new(),
            positioned_anchors: Vec::new(),
            conditional_definition: None,
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
        if self.conditional_definition.is_some() {
            return self
                .resolved_lines(usize::MAX)
                .iter()
                .map(Self::preferred_width)
                .max()
                .unwrap_or(1);
        }
        let content = self.table_row.as_ref().map_or_else(
            || super::inline::spans_width(&self.spans),
            |table| table.layout.preferred_width(),
        );
        self.indent.saturating_add(content)
    }
}
