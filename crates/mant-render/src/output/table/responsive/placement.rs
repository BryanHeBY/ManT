//! Place prepared owners in the selected bounded grid.
use super::{
    ProjectedTableRow, Row, TableProjection, TableRowKind, TableText,
    frame::Frame,
    measurement::{alignment_padding, column_start, owner_area, owner_width},
    spans,
};
pub(super) fn dense<T: TableText>(
    prepared: &[Row<'_, T>],
    columns: &[usize],
    frame: Frame<'_>,
    projection: TableProjection,
    available: Option<usize>,
) -> Vec<ProjectedTableRow<T>> {
    let count = columns.len();
    let content_width = columns.iter().sum::<usize>() + frame.gap * count.saturating_sub(1);
    let local = if projection.bounded_frame || frame.has_edges() {
        Some(content_width)
    } else {
        available
    };
    let mut output = Vec::new();
    let mut spans = spans::Spans::new(count);
    if let Some(top) = frame.horizontal(columns, None, true, false, None) {
        output.push(T::plain(&top));
    }
    for (index, row) in prepared.iter().enumerate() {
        spans.enter(row);
        let lines = render_row(row, columns, &frame, local, &spans);
        output.extend(lines);
        if let Some(next) = prepared.get(index + 1)
            && matches!(row.source.kind, TableRowKind::Data)
            && matches!(next.source.kind, TableRowKind::Data)
            && let Some(rule) = frame.between(columns, &spans, row.source)
        {
            output.push(T::plain(&rule));
        }
    }
    if let Some(bottom) = frame.bottom(columns, &spans) {
        output.push(T::plain(&bottom));
    }
    let completed = output.last().is_some_and(TableText::is_empty);
    vec![
        ProjectedTableRow::FrameWidth(content_width.saturating_add(frame.outside())),
        ProjectedTableRow::Content(output, completed, false),
    ]
}

fn render_row<T: TableText>(
    row: &Row<'_, T>,
    columns: &[usize],
    frame: &Frame<'_>,
    viewport: Option<usize>,
    spans: &spans::Spans,
) -> Vec<T> {
    if !matches!(row.source.kind, TableRowKind::Data) {
        return frame
            .rule(columns, &row.source.kind, Some(row.source))
            .map(|value| frame.mask_rule(&value, columns, &spans.rules(), spans, Some(row.source)))
            .map(|value| vec![T::plain(&value)])
            .unwrap_or_default();
    }
    let height = row
        .owners
        .iter()
        .map(|owner| owner.rows().len())
        .max()
        .unwrap_or(1);
    (0..height)
        .map(|line| {
            let mut value = T::plain(&frame.left_rail(row.source));
            let mut column = 0;
            while column < columns.len() {
                if let Some(owner) = row.owners.iter().find(|owner| owner.column == column) {
                    let slot = owner_width(columns, column, owner.span, frame.gap);
                    let start = column_start(columns, column, frame.gap);
                    let area = owner_area(owner, columns.len(), start, slot, viewport);
                    let mut text = owner
                        .rows()
                        .get(line)
                        .cloned()
                        .unwrap_or_else(|| T::plain(""));
                    if let Some(rule) = owner.text.rule {
                        text = T::plain(&rule.to_string().repeat(slot));
                    }
                    let extra = alignment_padding(&text, &owner.text, slot, Some(area));
                    let has_owner = text.has_alignment();
                    text = text.align_within(Some(area));
                    if !has_owner && extra > 0 {
                        text = text.prefixed(&" ".repeat(extra));
                    }
                    let width = text.width();
                    value.append(&text);
                    column = column.saturating_add(owner.span).min(columns.len());
                    if column < columns.len() || frame.has_outer() {
                        value.append(&T::plain(&" ".repeat(slot.saturating_sub(width))));
                    }
                } else {
                    value.append(&T::plain(&" ".repeat(columns[column])));
                    column += 1;
                }
                if column < columns.len() {
                    value.append(&T::plain(&frame.divider(
                        row.source,
                        column,
                        spans.interior(column),
                    )));
                }
            }
            value.append(&T::plain(&frame.right_rail(row.source)));
            value
        })
        .collect()
}
