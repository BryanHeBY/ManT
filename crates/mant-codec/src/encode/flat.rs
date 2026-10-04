//! Portable table cells flatten presentation, never semantic ownership.
use super::mapped::MappedText;
use mant_ir::{
    Block, EntryOwner, InlineContentRef, TableCell, TableCellKind, TableRow, TableRowPlan,
    TableRuleCellKind, bounded_table_rows, geometry::GapPlan,
};

#[derive(Clone, Copy, Default)]
enum Tail {
    #[default]
    Shared,
    Open,
    EndRow,
    CompletedRows,
}

#[derive(Clone, Copy)]
enum ParagraphTail {
    OpenCellRow,
    BlockBoundary,
}

/// Accepted IR row facts, not formatter state. Pending boundaries remain
/// separate through transparent containers until the next physical contribution.
/// Generated separators never acquire an owner.
#[derive(Default)]
struct Projection {
    mapped: MappedText,
    tail: Tail,
    physical: bool,
    /// An empty open row has an origin, but no padding until content arrives.
    open_row_indent: i32,
    leading_gap: GapPlan,
    pending_gap: GapPlan,
}

impl Projection {
    fn text(mapped: MappedText, tail: Tail, physical: bool) -> Self {
        Self {
            mapped,
            tail,
            physical,
            ..Self::default()
        }
    }

    fn gap(&mut self, rows: u16) {
        if self.physical {
            self.pending_gap.append_resolved(rows);
        } else {
            self.leading_gap.append_resolved(rows);
        }
    }

    fn append(&mut self, mut value: Self, separator: &str) {
        self.gap(value.leading_gap.rows(0));
        if !value.physical {
            self.gap(value.pending_gap.rows(0));
            return;
        }
        // A literal empty BODY can occupy the existing shared open row without
        // adding a glyph or closing it. Keep that row's deferred HEAD origin
        // until a later piece actually receives it, including another cell.
        let retains_open_row = self.physical
            && self.pending_gap.rows(0) == 0
            && matches!(self.tail, Tail::Open)
            && matches!(value.tail, Tail::Shared)
            && separator.is_empty()
            && value.mapped.text.is_empty();
        if self.physical {
            if self.pending_gap.rows(0) > 0 {
                self.complete_gap();
            } else {
                match self.tail {
                    Tail::EndRow => self.mapped.text.push('\n'),
                    Tail::CompletedRows => {}
                    Tail::Open => {
                        if !separator.contains('\n')
                            && (!separator.is_empty()
                                || value
                                    .mapped
                                    .text
                                    .split('\n')
                                    .next()
                                    .is_some_and(|row| !row.is_empty()))
                        {
                            self.mapped.text.push_str(
                                &" ".repeat(mant_ir::geometry::padding(self.open_row_indent)),
                            );
                        }
                        self.mapped.text.push_str(separator);
                    }
                    Tail::Shared => self.mapped.text.push_str(separator),
                }
            }
        }
        self.mapped.append(std::mem::take(&mut value.mapped));
        self.physical = true;
        if !retains_open_row {
            self.tail = value.tail;
            self.open_row_indent = value.open_row_indent;
        }
        self.pending_gap = value.pending_gap;
    }

    fn complete_gap(&mut self) {
        let rows = self.pending_gap.rows(0);
        if rows > 0 {
            // A literal open tail is an existing empty row. Close it before
            // appending independent resolved empty rows (term.c::term_vspace).
            if self.physical && !matches!(self.tail, Tail::CompletedRows) {
                self.mapped.text.push('\n');
            }
            self.mapped.text.push_str(&"\n".repeat(usize::from(rows)));
            self.tail = Tail::CompletedRows;
            self.pending_gap = GapPlan::default();
        }
    }

    fn structural_row(&mut self) {
        if !self.physical {
            self.pending_gap = std::mem::take(&mut self.leading_gap);
            if self.pending_gap.rows(0) > 0 {
                // These requests already own the empty data rows. Keep their
                // budget pending until another cell or this whole row ends;
                // structural occupancy adds no second empty-row delimiter.
                self.tail = Tail::CompletedRows;
            }
            self.physical = true;
        }
    }

    fn with_owner(mut self, owner: EntryOwner<'_>, track: bool) -> Self {
        self.mapped = self.mapped.with_owner(owner, track);
        self
    }

    fn finish(mut self) -> MappedText {
        let leading = self.leading_gap.rows(0);
        if leading > 0 {
            self.mapped.insert(0, &"\n".repeat(usize::from(leading)));
        }
        self.complete_gap();
        self.mapped
    }
}

fn join(values: impl IntoIterator<Item = Projection>, separator: &str) -> Projection {
    let mut output = Projection::default();
    for value in values {
        output.append(value, separator);
    }
    output
}

pub(super) fn rows(rows: &[TableRow], track: bool) -> Vec<MappedText> {
    project_rows(rows, track).map(Projection::finish).collect()
}

fn project_rows(rows: &[TableRow], track: bool) -> impl Iterator<Item = Projection> + '_ {
    bounded_table_rows(rows)
        .into_iter()
        .zip(rows)
        .filter_map(|(plan, row)| (!mant_ir::table_row_is_navigation_only(row)).then_some(plan))
        .map(move |row| {
            let mut output = match row {
                TableRowPlan::Empty => Projection::text(MappedText::default(), Tail::EndRow, true),
                TableRowPlan::WholeRule { double } => Projection::text(
                    if double { "===" } else { "---" }.to_owned().into(),
                    Tail::Shared,
                    true,
                ),
                TableRowPlan::LayoutRule { cells } => Projection::text(
                    MappedText::join(
                        cells.iter().map(|cell| {
                            match cell {
                                TableRuleCellKind::Horizontal => "---",
                                TableRuleCellKind::DoubleHorizontal => "===",
                            }
                            .to_owned()
                            .into()
                        }),
                        " | ",
                    ),
                    Tail::Shared,
                    true,
                ),
                TableRowPlan::Dense { slots } => join(
                    slots.into_iter().map(|cell| {
                        cell.map_or_else(
                            || Projection::text(MappedText::default(), Tail::Shared, true),
                            |cell| plain_cell(cell, track),
                        )
                    }),
                    " | ",
                ),
                TableRowPlan::Sparse { cells } => join(
                    cells.into_iter().map(|positioned| {
                        let mut value = plain_cell(positioned.cell, track);
                        value.mapped.insert(
                            0,
                            &format!("column {}: ", positioned.column.saturating_add(1)),
                        );
                        value
                    }),
                    " | ",
                ),
            };
            // A whole data row consumes its pending boundary. A later row
            // begins an independent budget; navigation-only rows were filtered
            // before reaching this physical boundary.
            output.complete_gap();
            output
        })
}

fn plain_cell(cell: &TableCell, track: bool) -> Projection {
    // tbl_term.c::term_tbl/tbl_hrule distinguish rules from suppressed payload.
    let rule = match cell.kind {
        TableCellKind::HorizontalRule | TableCellKind::IsolatedHorizontalRule => Some("---"),
        TableCellKind::DoubleHorizontalRule | TableCellKind::IsolatedDoubleHorizontalRule => {
            Some("===")
        }
        TableCellKind::Text => None,
    };
    let mut output = rule.map_or_else(
        || plain_blocks(&cell.blocks, "; ", ParagraphTail::OpenCellRow, track),
        |rule| Projection::text(rule.to_owned().into(), Tail::Shared, true),
    );
    // An actual data cell retains its structural row even without glyphs.
    output.structural_row();
    if cell.break_after {
        // Explicit closure consumes this row's existing gap once. Later
        // leading requests use a new budget even if the gap is already full.
        output.complete_gap();
        if !matches!(output.tail, Tail::CompletedRows) {
            output.tail = Tail::EndRow;
        }
    }
    output
}

fn plain_blocks(blocks: &[Block], separator: &str, tail: ParagraphTail, track: bool) -> Projection {
    plain_blocks_in_row(blocks, separator, tail, track, None)
}

fn plain_blocks_in_row(
    blocks: &[Block],
    separator: &str,
    tail: ParagraphTail,
    track: bool,
    shared_first: Option<usize>,
) -> Projection {
    let mut output = Projection::default();
    for (index, block) in blocks.iter().enumerate() {
        output.gap(mant_ir::geometry::block_gap(block));
        if !matches!(block, Block::VerticalSpace { .. }) {
            output.append(
                plain_block(
                    block,
                    if index + 1 == blocks.len() {
                        tail
                    } else {
                        ParagraphTail::BlockBoundary
                    },
                    track,
                    shared_first == Some(index),
                ),
                separator,
            );
        }
    }
    output
}

fn inline_projection(
    content: InlineContentRef<'_>,
    paragraph: Option<ParagraphTail>,
    shared_first: bool,
) -> Projection {
    let mut text = String::new();
    let mut ends_in_break = false;
    let mut authored_row = false;
    mant_ir::visit_inline_plain_text(content.content, |chunk| {
        authored_row = true;
        text.push_str(chunk);
        if let Some(last) = chunk.as_bytes().last() {
            ends_in_break = *last == b'\n';
        }
    });
    let physical = if paragraph.is_some() {
        !text.is_empty()
    } else {
        authored_row
    };
    let tail = if !ends_in_break {
        Tail::Shared
    } else if matches!(paragraph, Some(ParagraphTail::BlockBoundary)) {
        // Nonfinal paragraphs retire one provisional empty row; their frame
        // closes the retained row. Literal content never retires that tail.
        text.pop();
        Tail::EndRow
    } else {
        Tail::Open
    };
    let open_row_indent = if ends_in_break && matches!(tail, Tail::Open) {
        content
            .layout
            .row_indent(mant_ir::logical_row_count(content.content) - 1)
    } else {
        0
    };
    let mut mapped = String::new();
    for (row, text) in text.split('\n').enumerate() {
        if row > 0 {
            mapped.push('\n');
        }
        if !text.is_empty() {
            if row > 0 || !shared_first {
                mapped.push_str(
                    &" ".repeat(mant_ir::geometry::padding(content.layout.row_indent(row))),
                );
            }
            mapped.push_str(text);
        }
    }
    Projection {
        open_row_indent,
        ..Projection::text(mapped.into(), tail, physical)
    }
}

fn plain_block(block: &Block, tail: ParagraphTail, track: bool, shared_first: bool) -> Projection {
    match block {
        Block::Paragraph {
            children,
            inline_layout,
            ..
        } => inline_projection(
            InlineContentRef {
                content: children,
                layout: inline_layout,
            },
            Some(tail),
            shared_first,
        ),
        Block::Preformatted {
            children,
            inline_layout,
            ..
        } => inline_projection(
            InlineContentRef {
                content: children,
                layout: inline_layout,
            },
            None,
            shared_first,
        ),
        Block::List { items, .. } => join(
            items.iter().map(|item| {
                plain_blocks(&item.blocks, ", ", ParagraphTail::BlockBoundary, track)
                    .with_owner(EntryOwner::List(item), track)
            }),
            ", ",
        ),
        Block::DefinitionList { items, .. } => join(
            items.iter().map(|item| {
                let mut terms = join(
                    item.terms.iter().map(|term| {
                        inline_projection(
                            term.inline_content(),
                            Some(ParagraphTail::OpenCellRow),
                            false,
                        )
                    }),
                    "\n",
                );
                let shared = terms.physical.then(|| first_shared_body(item)).flatten();
                let description = plain_blocks_in_row(
                    &item.description,
                    "\n",
                    ParagraphTail::BlockBoundary,
                    track,
                    shared,
                );
                // Table simplification does not authorize changing the
                // accepted HEAD/BODY row or word seam. Geometric BODY origins
                // cannot insert cells into a joined word (termp_it_pre/post).
                let separator = if shared.is_none() {
                    "\n"
                } else if item.head_body_relation.joins_without_separator() {
                    ""
                } else {
                    " "
                };
                terms.append(description, separator);
                terms.with_owner(EntryOwner::Definition(item), track)
            }),
            "; ",
        ),
        Block::Table { rows, .. } => join(project_rows(rows, track), "; "),
        Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
            let text = value.trim().to_owned();
            let physical = !text.is_empty();
            Projection::text(text.into(), Tail::Shared, physical)
        }
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => Projection::default(),
    }
}

/// Empty inline roots are transparent, but an executed leading gap or a
/// structural BODY cannot share the final HEAD row. Read accepted IR only.
fn first_shared_body(item: &mant_ir::DefinitionItem) -> Option<usize> {
    if !item.inline_term() {
        return None;
    }
    for (index, block) in item.description.iter().enumerate() {
        if mant_ir::geometry::block_gap(block) > 0 {
            return None;
        }
        match block {
            Block::Paragraph { children, .. } => {
                let mut physical = false;
                mant_ir::visit_inline_plain_text(children, |text| physical |= !text.is_empty());
                if physical {
                    return Some(index);
                }
            }
            Block::Preformatted { children, .. } => {
                if mant_ir::geometry::has_literal_rows(children) {
                    return Some(index);
                }
            }
            Block::VerticalSpace { .. } => {}
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_cell_joins_rebase_existing_unicode_owner_ranges() {
        // Opaque borrowed keys are sufficient; no facts are dereferenced.
        let key = std::ptr::null();
        let output = join(
            [
                Projection::text("α\n\n".to_owned().into(), Tail::CompletedRows, true),
                Projection::text(
                    MappedText {
                        text: "中BODY".into(),
                        owners: vec![(key, 0..7)],
                    },
                    Tail::Shared,
                    true,
                ),
            ],
            " | ",
        )
        .finish();
        assert_eq!(output.text, "α\n\n中BODY");
        assert_eq!(output.owners, [(key, 4..11)]);
        assert_eq!(&output.text[output.owners[0].1.clone()], "中BODY");
    }

    #[test]
    fn nested_literal_gap_closures_rebase_bytes_without_claiming_generated_rows() {
        let key = std::ptr::null();
        for depth in [1, 2, 4] {
            let mut first = Projection::text(
                MappedText {
                    text: "α\n".into(),
                    owners: vec![(key, 0..3)],
                },
                Tail::Open,
                true,
            );
            first.gap(1);
            for _ in 0..depth {
                first = join([first], ", ");
            }
            let output = join(
                [
                    first,
                    Projection::text(
                        MappedText {
                            text: "中BODY".into(),
                            owners: vec![(key, 0..7)],
                        },
                        Tail::Shared,
                        true,
                    ),
                ],
                " | ",
            )
            .finish();
            assert_eq!(output.text, "α\n\n\n中BODY");
            assert_eq!(output.owners, [(key, 0..3), (key, 5..12)]);
            assert_eq!(&output.text[output.owners[0].1.clone()], "α\n");
            assert_eq!(&output.text[output.owners[1].1.clone()], "中BODY");
        }
    }
}
