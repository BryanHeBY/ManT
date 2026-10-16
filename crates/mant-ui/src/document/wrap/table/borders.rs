//! Paint authored edges around one prepared table region, outside body maps.
use super::super::{Line, LogicalTableRow, RowCopyMap, Span, WrappedLine};
use mant_ir::{TableBorderStyle as Stroke, TableRuleCellKind};

pub(super) fn column_style(
    table: &LogicalTableRow,
    boundary: usize,
    count: usize,
) -> Option<Stroke> {
    if boundary > 0 && boundary < count {
        let left = table
            .cells
            .get(boundary - 1)
            .and_then(|cell| cell.occupancy.owner_column(boundary - 1));
        let right = table
            .cells
            .get(boundary)
            .and_then(|cell| cell.occupancy.owner_column(boundary));
        if left.is_some() && left == right {
            return None;
        }
    }
    let default = table.layout.borders.column_style(boundary, count);
    let local = u32::try_from(boundary).ok().and_then(|boundary| {
        table
            .column_borders
            .binary_search_by_key(&boundary, |edge| edge.column)
            .ok()
            .map(|index| table.column_borders[index].style)
    });
    match (default, local) {
        (Some(left), Some(right)) => Some(left.stronger(right)),
        (left, right) => left.or(right),
    }
}

pub(super) fn vertical_span(style: Option<Stroke>) -> Span<'static> {
    paint(
        match style {
            Some(Stroke::Single) => "│",
            Some(Stroke::Double) => "║",
            None => " ",
        }
        .into(),
    )
}

fn paint(value: String) -> Span<'static> {
    Span::styled(value, crate::theme::style(crate::theme::StyleRole::Rule))
}

fn decoration(spans: Vec<Span<'static>>) -> WrappedLine {
    WrappedLine {
        source_end: None,
        anchors: Vec::new(),
        line: Line::from(spans),
        links: Vec::new(),
        search_cells: Vec::new(),
        copy_map: RowCopyMap {
            end: Some(0),
            skip_row: true,
            ..RowCopyMap::default()
        },
    }
}

fn horizontal(stroke: Stroke) -> char {
    match stroke {
        Stroke::Single => '─',
        Stroke::Double => '═',
    }
}

fn crossing(horizontal: Stroke, vertical: Stroke, top: bool, bottom: bool) -> char {
    match (horizontal, vertical, top, bottom) {
        (Stroke::Single, Stroke::Single, true, _) => '┬',
        (Stroke::Single, Stroke::Single, _, true) => '┴',
        (Stroke::Single, Stroke::Single, _, _) => '┼',
        (Stroke::Double, Stroke::Double, true, _) => '╦',
        (Stroke::Double, Stroke::Double, _, true) => '╩',
        (Stroke::Double, Stroke::Double, _, _) => '╬',
        (Stroke::Single, Stroke::Double, true, _) => '╥',
        (Stroke::Single, Stroke::Double, _, true) => '╨',
        (Stroke::Single, Stroke::Double, _, _) => '╫',
        (Stroke::Double, Stroke::Single, true, _) => '╤',
        (Stroke::Double, Stroke::Single, _, true) => '╧',
        (Stroke::Double, Stroke::Single, _, _) => '╪',
    }
}

fn vertical_character(style: Stroke) -> char {
    match style {
        Stroke::Single => '│',
        Stroke::Double => '║',
    }
}

fn junction(
    left: Option<Stroke>,
    right: Option<Stroke>,
    vertical: Option<Stroke>,
    top: bool,
    bottom: bool,
) -> char {
    let horizontal = match (left, right) {
        (Some(left), Some(right)) => Some(left.stronger(right)),
        (left, right) => left.or(right),
    };
    let Some(vertical) = vertical else {
        return horizontal.map_or(' ', self::horizontal);
    };
    let Some(horizontal) = horizontal else {
        return vertical_character(vertical);
    };
    if left.is_some() && right.is_some() {
        return crossing(horizontal, vertical, top, bottom);
    }
    match (horizontal, vertical, right.is_some()) {
        (Stroke::Single, Stroke::Single, true) => '├',
        (Stroke::Single, Stroke::Single, false) => '┤',
        (Stroke::Double, Stroke::Double, true) => '╠',
        (Stroke::Double, Stroke::Double, false) => '╣',
        (Stroke::Single, Stroke::Double, true) => '╟',
        (Stroke::Single, Stroke::Double, false) => '╢',
        (Stroke::Double, Stroke::Single, true) => '╞',
        (Stroke::Double, Stroke::Single, false) => '╡',
    }
}

fn corner(stroke: Stroke, left: bool, top: bool, bottom: bool) -> char {
    match (stroke, left, top, bottom) {
        (Stroke::Single, true, true, _) => '┌',
        (Stroke::Single, false, true, _) => '┐',
        (Stroke::Single, true, _, true) => '└',
        (Stroke::Single, false, _, true) => '┘',
        (Stroke::Single, true, _, _) => '├',
        (Stroke::Single, false, _, _) => '┤',
        (Stroke::Double, true, true, _) => '╔',
        (Stroke::Double, false, true, _) => '╗',
        (Stroke::Double, true, _, true) => '╚',
        (Stroke::Double, false, _, true) => '╝',
        (Stroke::Double, true, _, _) => '╠',
        (Stroke::Double, false, _, _) => '╣',
    }
}

fn frame_corner(horizontal: Stroke, vertical: Stroke, left: bool, top: bool, bottom: bool) -> char {
    if !top && !bottom {
        return junction(
            (!left).then_some(horizontal),
            left.then_some(horizontal),
            Some(vertical),
            false,
            false,
        );
    }
    if horizontal == vertical {
        return corner(vertical, left, top, bottom);
    }
    match (horizontal, vertical, left, top) {
        (Stroke::Single, Stroke::Double, true, true) => '╓',
        (Stroke::Single, Stroke::Double, false, true) => '╖',
        (Stroke::Single, Stroke::Double, true, false) => '╙',
        (Stroke::Single, Stroke::Double, false, false) => '╜',
        (Stroke::Double, Stroke::Single, true, true) => '╒',
        (Stroke::Double, Stroke::Single, false, true) => '╕',
        (Stroke::Double, Stroke::Single, true, false) => '╘',
        (Stroke::Double, Stroke::Single, false, false) => '╛',
        _ => unreachable!("matching strengths returned above"),
    }
}

struct RuleArea<'a> {
    indent: usize,
    extent: usize,
    widths: Option<&'a [usize]>,
    gap: usize,
    top: bool,
    bottom: bool,
    outer: bool,
}

fn rule_row(
    table: &LogicalTableRow,
    stroke: Stroke,
    styles: Option<&[TableRuleCellKind]>,
    continued: Option<&LogicalTableRow>,
    area: &RuleArea<'_>,
) -> WrappedLine {
    let rails = usize::from(area.outer) * 2;
    let mut body = vec![' '; area.extent + rails * 2];
    let span_is_covered = |column: usize| {
        continued
            .and_then(|row| row.cells.get(column))
            .is_some_and(|cell| cell.occupancy.covered())
    };
    let style_at = |column: usize| {
        if span_is_covered(column) {
            None
        } else {
            styles.map_or(Some(stroke), |styles| {
                styles.get(column).map(|style| match style {
                    TableRuleCellKind::Horizontal => Stroke::Single,
                    TableRuleCellKind::DoubleHorizontal => Stroke::Double,
                })
            })
        }
    };
    if let Some(widths) = area.widths {
        let mut at = rails;
        for (column, width) in widths.iter().enumerate() {
            let style = style_at(column);
            for target in body.iter_mut().skip(at).take(*width) {
                *target = style.map_or(' ', horizontal);
            }
            at = at.saturating_add(*width);
            if column + 1 < widths.len() {
                let left = area
                    .gap
                    .saturating_sub(usize::from(table.layout.rails.columns))
                    / 2;
                let next = style_at(column + 1);
                for (index, target) in body.iter_mut().skip(at).take(area.gap).enumerate() {
                    *target = if index < left { style } else { next }.map_or(' ', horizontal);
                }
                if table.layout.rails.columns {
                    let boundary = at + left;
                    if let Some(target) = body.get_mut(boundary) {
                        let continuation_covers = continued.is_some_and(|row| {
                            let left = row
                                .cells
                                .get(column)
                                .and_then(|cell| cell.occupancy.owner_column(column));
                            let right = row
                                .cells
                                .get(column + 1)
                                .and_then(|cell| cell.occupancy.owner_column(column + 1));
                            left.is_some() && left == right
                        });
                        *target = junction(
                            style,
                            next,
                            (!continuation_covers)
                                .then(|| column_style(table, column + 1, widths.len()))
                                .flatten(),
                            area.top,
                            area.bottom,
                        );
                    }
                }
                at = at.saturating_add(area.gap);
            }
        }
    } else {
        for target in body.iter_mut().skip(rails).take(area.extent) {
            *target = horizontal(stroke);
        }
    }
    if rails > 0 {
        let count = table.layout.logical_columns;
        let left = column_style(table, 0, count);
        let right = column_style(table, count, count);
        let last = body.len().saturating_sub(1);
        let first_style = area.widths.map_or(Some(stroke), |_| style_at(0));
        let last_style = area.widths.map_or(Some(stroke), |widths| {
            style_at(widths.len().saturating_sub(1))
        });
        if let Some(style) = left {
            body[0] = if let Some(horizontal) = first_style {
                frame_corner(horizontal, style, true, area.top, area.bottom)
            } else {
                vertical_character(style)
            };
            body[1] = first_style.map_or(' ', horizontal);
        }
        if let Some(style) = right {
            body[last] = if let Some(horizontal) = last_style {
                frame_corner(horizontal, style, false, area.top, area.bottom)
            } else {
                vertical_character(style)
            };
            body[last.saturating_sub(1)] = last_style.map_or(' ', horizontal);
        }
    }
    decoration(vec![
        Span::raw(" ".repeat(area.indent)),
        paint(body.into_iter().collect()),
    ])
}

fn paint_sides(row: &mut WrappedLine, table: &LogicalTableRow, indent: usize, extent: usize) {
    if !table.layout.rails.outer {
        return;
    }
    let left = column_style(table, 0, table.layout.logical_columns);
    let right = column_style(
        table,
        table.layout.logical_columns,
        table.layout.logical_columns,
    );
    let prefix = indent + 2;
    let mut source = std::mem::take(&mut row.line.spans);
    if let Some(first) = source.first_mut()
        && first.content.starts_with(&" ".repeat(prefix))
    {
        first.content = first.content[prefix..].to_owned().into();
    }
    let used = super::super::super::inline::spans_width(&source);
    row.line.spans = vec![
        Span::raw(" ".repeat(indent)),
        vertical_span(left),
        Span::raw(" "),
    ];
    row.line.spans.extend(source);
    row.line
        .spans
        .push(Span::raw(" ".repeat(extent.saturating_sub(used))));
    row.line.spans.push(Span::raw(" "));
    row.line.spans.push(vertical_span(right));
    row.copy_map.omitted.push(0..prefix);
    row.copy_map
        .omitted
        .push(prefix.saturating_add(used)..prefix.saturating_add(extent).saturating_add(2));
    row.copy_map.end.get_or_insert(prefix.saturating_add(used));
}

pub(super) fn decorate(
    tables: &[&LogicalTableRow],
    rows: &mut [Vec<WrappedLine>],
    indent: usize,
    widths: Option<&[usize]>,
    gap: usize,
    extent: usize,
    show_outer: bool,
) {
    let Some(first) = tables.first() else {
        return;
    };
    let layout = &first.layout;
    let framed = layout.borders.outer.filter(|_| show_outer);
    let area = |top, bottom| RuleArea {
        indent,
        extent,
        widths,
        gap,
        top,
        bottom,
        outer: show_outer,
    };
    for (index, (table, output)) in tables.iter().zip(rows.iter_mut()).enumerate() {
        if table.rules.is_some() && widths.is_none() && table.whole_rule.is_none() {
            // The bounded fallback already retained every authored strength in
            // source order. Outside paint must not replace those segments with
            // a synthesized single-stroke rectangular rule.
            for row in output.iter_mut() {
                if show_outer {
                    paint_sides(row, table, indent, extent);
                }
                row.copy_map.skip_row = true;
                row.copy_map.end = Some(0);
            }
        } else if table.rules.is_some() && (show_outer || layout.rails.columns) {
            let stroke = if table.whole_rule == Some(true) {
                Stroke::Double
            } else {
                Stroke::Single
            };
            let styles = table
                .whole_rule
                .is_none()
                .then_some(table.rules.as_deref())
                .flatten();
            let next = tables
                .get(index + 1)
                .copied()
                .filter(|row| row.rules.is_none());
            *output = vec![rule_row(table, stroke, styles, next, &area(false, false))];
        } else if show_outer {
            for row in output.iter_mut() {
                paint_sides(row, table, indent, extent);
            }
        }
        if index > 0
            && table.rules.is_none()
            && tables[index - 1].rules.is_none()
            && let Some(stroke) = layout.borders.inner_rows
            && table.cells.iter().any(|cell| !cell.occupancy.covered())
        {
            output.insert(
                0,
                rule_row(table, stroke, None, Some(table), &area(false, false)),
            );
        }
    }
    if let Some(stroke) = framed
        && !rows.is_empty()
    {
        rows[0].insert(0, rule_row(first, stroke, None, None, &area(true, false)));
        let last = rows.len() - 1;
        rows[last].push(rule_row(
            tables[last],
            stroke,
            None,
            None,
            &area(false, true),
        ));
    }
}
