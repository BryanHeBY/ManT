//! Assemble native tbl rows; source recovery has a separate transaction boundary.
use crate::mandoc::roff_escape::RoffFont;
use crate::mandoc::{LoweringContext, layout::layout, source_span};
use libmandoc_rs::{
    Node, TableAlignment as MandocTableAlignment, TableCellKind, TableFont,
    TableRowKind as MandocTableRowKind, TableRuleCellKind as MandocTableRuleCellKind,
};
use mant_ir::{Block, Inline, TableAlignment as AstTableAlignment, TableRow};
mod cells;
mod execution;
mod recovery;
pub(super) use recovery::{TableEmbedding, TableEmbeddingPlan};

/// A phrasing owner projects a native row's readable cells without device
/// borders. Column entry and width-dependent fills still use the table plan.
pub(in crate::mandoc) struct InlineTableSequence<'a> {
    context: LoweringContext<'static>,
    plan: TableEmbeddingPlan<'a>,
}

pub(in crate::mandoc) struct InlineTableRow {
    pub(in crate::mandoc) content: Vec<Inline>,
    pub(in crate::mandoc) data: bool,
    pub(in crate::mandoc) separates_prefix: bool,
}

/// The accepted device prefix, in basic units, before the first table cell.
#[derive(Clone, Copy)]
pub(in crate::mandoc) struct InlineTablePrefix {
    pub(in crate::mandoc) offset: usize,
    pub(in crate::mandoc) printed: usize,
    pub(in crate::mandoc) min_padding: usize,
}

impl InlineTablePrefix {
    fn left_pad(self, col_offset: usize) -> usize {
        self.offset
            .saturating_add(col_offset)
            .saturating_sub(self.printed)
            .max(self.min_padding)
    }
}

impl<'a> InlineTableSequence<'a> {
    pub(in crate::mandoc) fn new(
        siblings: &'a [Node],
        execution: &crate::mandoc::inline::InlineExecutionState,
    ) -> Self {
        let mut context = LoweringContext::with_table_recovery(
            None,
            execution.table_recovery.clone().unwrap_or_default(),
        );
        context.macro_set = execution.macro_set;
        if let Some(annotations) = &execution.zero_advance.annotations {
            context.annotations = annotations.clone();
        }
        let plan = TableEmbeddingPlan::new(siblings);
        Self { context, plan }
    }

    pub(in crate::mandoc) fn lower_row(
        &self,
        node: &Node,
        index: usize,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        prefix: Option<InlineTablePrefix>,
    ) -> Option<InlineTableRow> {
        let kind = table_row_kind(node)?;
        let embedding = self.plan.embedding(index);
        let cells = cells::lower_cells(
            node,
            &kind,
            &self.context,
            crate::mandoc::layout::SourceIndent::default(),
            embedding,
            prefix.is_some(),
            formatter,
        );
        self.context
            .annotations
            .record_diagnostics(self.context.diagnostics.take());
        finish_table_span(formatter);
        let mut output = Vec::new();
        let mut first = None;
        let mut earlier_cells = Vec::new();
        let mut native_height = 1;
        let mut projected_height = 1;
        for (index, mut cell) in cells.into_iter().enumerate() {
            for block in cell.cell.blocks {
                if let Block::Paragraph { mut children, .. } = block
                    && !children.is_empty()
                {
                    for _ in 0..cell.empty_tail {
                        crate::mandoc::inline::consume_one_row_ending(&mut children);
                    }
                    let interior = children
                        .iter()
                        .filter(|inline| matches!(inline, Inline::LineBreak { .. }))
                        .count();
                    native_height = native_height.max(1 + interior + cell.empty_tail);
                    if crate::mandoc::inline::has_rendered_formatter_glyph(&children) {
                        if first.is_some() {
                            output.push(Inline::Text { value: " ".into() });
                        }
                        first.get_or_insert_with(|| (index, cell.entry_font.take()));
                        projected_height += interior;
                    } else {
                        // term_field defers SP/TAB until a real glyph.
                        // An earlier unprinted cell adds neither padding
                        // nor the separator used between visible cells.
                        crate::mandoc::inline::retain_inline_identities(&mut children);
                    }
                    output.extend(children);
                }
            }
            if first.is_none()
                && let Some(font) = cell.entry_font
            {
                earlier_cells.push((node.table_cells[index].column as usize, font));
            }
        }
        // A subsequent cell can print on an empty follow-up pass. Keep only
        // the remaining empty rows after the phrasing projection has read
        // every cell's actual text.
        output.extend(
            std::iter::repeat_with(Inline::line_break)
                .take(native_height.saturating_sub(projected_height)),
        );
        Some(InlineTableRow {
            content: output,
            data: kind == mant_ir::TableRowKind::Data,
            separates_prefix: prefix.is_some_and(|prefix| {
                first.is_some_and(|(index, entry_font)| {
                    embedding.map_or_else(
                        || prefix.left_pad(0) > 12,
                        |plan| {
                            plan.execution.separates_prefix(
                                node,
                                &node.table_cells[index],
                                prefix,
                                &formatter.execution,
                                entry_font,
                                &earlier_cells,
                            )
                        },
                    )
                })
            }),
        })
    }
}

pub(super) fn append_table_row(
    output: &mut Vec<Block>,
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    embedding: Option<&TableEmbedding>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) {
    let Some(kind) = table_row_kind(node) else {
        // Only native table spans carry a row kind; foreign or synthetic
        // nodes without one (even if they carry cells) are tolerated and
        // dropped here rather than treated as an invariant violation.
        return;
    };
    if !matches!(&kind, mant_ir::TableRowKind::Data) && !node.table_cells.is_empty() {
        // CVS whole-row rules do not own data cells. Refuse to reinterpret a
        // structurally inconsistent foreign snapshot as printable contents.
        return;
    }
    let cells = cells::lower_cells(
        node,
        &kind,
        context,
        indent_columns,
        embedding,
        false,
        formatter,
    )
    .into_iter()
    .map(cells::ExecutedCell::into_block_cell)
    .collect();
    let (column_borders, newly_reduced) = table_row_borders(node, context);
    let row = TableRow {
        kind,
        column_borders,
        cells,
    };
    if !node.flags.table_start
        && let Some(Block::Table { rows, .. }) = output.last_mut()
    {
        if newly_reduced {
            // Reduce this complete table consistently if repeated sparse
            // authored edges exceed the derived-output allowance. Defaults
            // and every data owner remain intact; earlier rows must not
            // retain a fortuitously larger edge policy than later rows.
            for previous in rows.iter_mut() {
                previous.column_borders.clear();
            }
        }
        rows.push(row);
    } else {
        output.push(Block::Table {
            borders: crate::mandoc::table_layout_budget::table_borders(node),
            alignment: node.table_alignment.map(table_alignment),
            // CVS tbl_layout.c::cell_alloc grows the shared final opts.cols,
            // even when no data cell occupies a declared trailing column.
            column_count: node.table_column_count,
            column_preferences: mant_ir::ColumnPreferences::default(),
            rows: vec![row],
            layout: layout(indent_columns),
            source: source_span(node),
        });
    }
    finish_table_span(formatter);
}

fn finish_table_span(formatter: &mut crate::mandoc::formatter::FormatterState) {
    // term_tbl() selects NOSPACE|NONOSPACE before each span and removes
    // only NONOSPACE afterwards (tbl_term.c:175,608). Cell recovery uses a
    // local inline flow; its last word cannot replace the table's outgoing
    // global NOSPACE with its own ordinary word-boundary state.
    formatter.execution.select_native_no_space();
    formatter.set_spacing_enabled(true);
}

const fn table_alignment(alignment: MandocTableAlignment) -> AstTableAlignment {
    match alignment {
        MandocTableAlignment::Left => AstTableAlignment::Left,
        MandocTableAlignment::Center => AstTableAlignment::Center,
        MandocTableAlignment::Right => AstTableAlignment::Right,
    }
}

fn table_row_borders(
    node: &Node,
    context: &LoweringContext<'_>,
) -> (Vec<mant_ir::TableColumnBorder>, bool) {
    let mut budget = context.table_layout_budget.borrow_mut();
    let was_reduced = budget.exhausted();
    let borders = budget.column_borders(node);
    let newly_reduced = !was_reduced && budget.exhausted();
    (borders, newly_reduced)
}

fn table_cell_width(node: &Node, cell: &libmandoc_rs::TableCell) -> Option<u16> {
    let slot = node
        .table_layout
        .as_ref()?
        .cells
        .get(cell.column as usize)?;
    // tbl_layout.c stores w() in basic units; out.c treats it as a minimum,
    // never as clipping. Saturating this preference does not change a logical
    // column, clip a word or discard any source contents.
    let width = slot.minimum_width_basic_units.div_ceil(24);
    (width != 0).then(|| u16::try_from(width).unwrap_or(u16::MAX))
}

fn table_cell_font(font: Option<TableFont>) -> Option<RoffFont> {
    match font {
        Some(TableFont::Bold) => Some(RoffFont::Strong),
        Some(TableFont::Italic) => Some(RoffFont::Emphasis),
        Some(TableFont::BoldItalic) => Some(RoffFont::StrongEmphasis),
        Some(TableFont::CodeBold) => Some(RoffFont::CodeStrong),
        Some(TableFont::CodeItalic) => Some(RoffFont::CodeEmphasis),
        // CVS tbl_term.c::tbl_word does not push CR or Roman: an enclosing
        // .Bf/.ft selection stays on the terminal stack. CR presentation is
        // applied separately from that execution state.
        Some(TableFont::Code | TableFont::Roman) | None => None,
    }
}

fn table_row_kind(node: &Node) -> Option<mant_ir::TableRowKind> {
    match &node.table_row_kind {
        Some(MandocTableRowKind::Data) => Some(mant_ir::TableRowKind::Data),
        Some(MandocTableRowKind::HorizontalRule) => Some(mant_ir::TableRowKind::HorizontalRule),
        Some(MandocTableRowKind::DoubleHorizontalRule) => {
            Some(mant_ir::TableRowKind::DoubleHorizontalRule)
        }
        Some(MandocTableRowKind::LayoutRule { cells }) => Some(mant_ir::TableRowKind::LayoutRule {
            cells: cells
                .iter()
                .map(|cell| match cell {
                    MandocTableRuleCellKind::Horizontal => mant_ir::TableRuleCellKind::Horizontal,
                    MandocTableRuleCellKind::DoubleHorizontal => {
                        mant_ir::TableRuleCellKind::DoubleHorizontal
                    }
                })
                .collect(),
        }),
        None => None,
    }
}

const fn table_cell_kind(kind: TableCellKind) -> mant_ir::TableCellKind {
    match kind {
        TableCellKind::HorizontalRule => mant_ir::TableCellKind::HorizontalRule,
        TableCellKind::DoubleHorizontalRule => mant_ir::TableCellKind::DoubleHorizontalRule,
        TableCellKind::IsolatedHorizontalRule => mant_ir::TableCellKind::IsolatedHorizontalRule,
        TableCellKind::IsolatedDoubleHorizontalRule => {
            mant_ir::TableCellKind::IsolatedDoubleHorizontalRule
        }
        TableCellKind::Text | TableCellKind::Empty => mant_ir::TableCellKind::Text,
    }
}
