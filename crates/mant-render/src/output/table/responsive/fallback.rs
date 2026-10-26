//! Source-order reuse of prepared owners after a table-wide fallback.
use super::{
    ProjectedTable, ProjectedTableRow, Row, TableOptions, TableRow, TableRowKind, TableText,
    frame::Frame, geometry, prepare::wrap,
};
pub(super) fn stack<T: TableText>(
    rows: Vec<Row<'_, T>>,
    projection: TableOptions,
    frame: Frame<'_>,
    available: Option<usize>,
    sparse: bool,
) -> ProjectedTable<T> {
    let width = available.map(|width| {
        width
            .saturating_sub(frame.outside())
            .min(geometry::MAX_COLUMN_PADDING.saturating_sub(geometry::padding(projection.origin)))
    });
    let wrap_width = available.map(|width| width.saturating_sub(frame.outside()));
    let mut projected = Vec::new();
    let mut spans = frame.stacked_spans();
    let mut rows = rows.into_iter().peekable();
    while let Some(row) = rows.next() {
        if let Some(spans) = &mut spans {
            spans.enter(&row);
        }
        if !matches!(row.source.kind, TableRowKind::Data) {
            let cells = match &row.source.kind {
                TableRowKind::DoubleHorizontalRule => {
                    vec![mant_ir::TableRuleCellKind::DoubleHorizontal]
                }
                TableRowKind::LayoutRule { cells } => cells.clone(),
                _ => vec![mant_ir::TableRuleCellKind::Horizontal],
            };
            projected.push((Some(row.source), ProjectedTableRow::Rule(cells, None)));
            continue;
        }
        let owners = row
            .owners
            .into_iter()
            .map(|mut owner| {
                if owner.wrap
                    && let Some(width) = wrap_width
                {
                    let _ = wrap(&mut owner.text, width);
                }
                (owner.column, owner.text)
            })
            .collect::<Vec<_>>();
        let (content, completed, _) = if sparse {
            super::super::rows::sparse_physical_rows(owners, projection.origin, width)
        } else {
            super::super::rows::stacked_physical_rows(
                owners.into_iter().map(|(_, text)| text).collect(),
                width,
            )
        };
        projected.push((
            Some(row.source),
            ProjectedTableRow::Content {
                lines: content,
                completed,
                stacked: true,
            },
        ));
        // Stacking changes cell placement, not original data-row boundaries.
        // Rule rows already supply their own separator; cell/visual-row ends
        // do not. Keep this receipt even when there is no outside frame.
        if let Some(next) = rows
            .peek()
            .filter(|next| matches!(next.source.kind, TableRowKind::Data))
            && let Some(rule) = frame.stacked_boundary(spans.as_ref(), next.source)
        {
            projected.push((Some(row.source), ProjectedTableRow::Rule(vec![rule], None)));
        }
    }
    frame_rows(projected, projection, frame, available)
}

/// Decorate only after every source-order row has its final physical receipt.
fn frame_rows<T: TableText>(
    projected: Vec<(Option<&TableRow>, ProjectedTableRow<T>)>,
    projection: TableOptions,
    frame: Frame<'_>,
    available: Option<usize>,
) -> ProjectedTable<T> {
    if !frame.has_edges() {
        // Unstyled tables retain the existing portable ASCII rule policy.
        return ProjectedTable::from_rows(projected.into_iter().map(|(_, row)| row).collect());
    }
    let max = projected
        .iter()
        .filter_map(|(_, row)| {
            if let ProjectedTableRow::Content { lines: rows, .. } = row {
                Some(rows.iter().map(TableText::width).max().unwrap_or(0))
            } else {
                None
            }
        })
        .max()
        .unwrap_or(1);
    if max
        .saturating_add(frame.outside())
        .saturating_add(geometry::padding(projection.origin))
        > geometry::MAX_COLUMN_PADDING
        || available.is_some_and(|available| max.saturating_add(frame.outside()) > available)
    {
        return ProjectedTable::from_rows(projected.into_iter().map(|(_, row)| row).collect());
    }
    if !frame.has_outer() {
        return ProjectedTable::from_rows(
            projected
                .into_iter()
                .map(|(source, row)| match row {
                    ProjectedTableRow::Rule(cells, _) => ProjectedTableRow::Content {
                        lines: rule_lines(&cells, max, frame, source),
                        completed: false,
                        stacked: true,
                    },
                    row @ ProjectedTableRow::Content { .. } => row,
                })
                .collect(),
        );
    }
    let mut boxed = Vec::new();
    if let Some(top) = frame.horizontal(&[max], None, true, false, None) {
        boxed.push(T::plain(&top));
    }
    for (source, row) in projected {
        match row {
            ProjectedTableRow::Content { lines: rows, .. } => {
                for mut value in rows {
                    let used = value.width();
                    value = value.prefixed(&frame.stack_left(source));
                    value.append(&T::plain(&format!(
                        "{}{}",
                        " ".repeat(max.saturating_sub(used)),
                        frame.stack_right(source)
                    )));
                    boxed.push(value);
                }
            }
            ProjectedTableRow::Rule(cells, _) => {
                // One source rule contributes once; it must not become a
                // complete table-width row for each logical rule column.
                boxed.extend(rule_lines(&cells, max, frame, source));
            }
        }
    }
    if let Some(bottom) = frame.horizontal(&[max], None, false, true, None) {
        boxed.push(T::plain(&bottom));
    }
    ProjectedTable::from_rows(vec![ProjectedTableRow::Content {
        lines: boxed,
        completed: false,
        stacked: true,
    }])
}

fn rule_lines<T: TableText>(
    cells: &[mant_ir::TableRuleCellKind],
    width: usize,
    frame: Frame<'_>,
    source: Option<&TableRow>,
) -> Vec<T> {
    cells
        .chunks(width.max(1))
        .map(|chunk| T::plain(&frame.fallback_rule(chunk, width.max(1), source)))
        .collect()
}
