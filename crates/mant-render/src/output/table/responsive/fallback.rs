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
    for row in rows {
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
    if !frame.has_outer() {
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
                for chunk in cells.chunks(max.max(1)) {
                    boxed.push(T::plain(&frame.fallback_rule(chunk, max.max(1), source)));
                }
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
