use super::{
    DEFAULT_RENDER_WIDTH, Limits, NodeKind, NodeRef, RenderError, RenderFormat,
    TERMINAL_KEEP_SPACING_MARKER, TableAlignment, TableTerminalBorder, TableTerminalFont,
    TableTerminalRow, TerminalFont, append, append_blank_line, display_width,
    mark_terminal_table_vertical_skip, render_terminal_font, render_terminal_visible_text,
    take_terminal_table_vertical_skip, terminal_line_length_before, terminal_line_length_value,
    terminal_previous_sibling,
};

/// Render one contiguous tbl range from its normalized row nodes.
///
/// Preprocessing deliberately exposes each tbl row as a public `Table` node
/// because that is the legacy owned-AST contract.  Terminal layout must still
/// see all adjacent rows before it can choose a column width, so the first row
/// gathers its sibling range and later rows become no-ops.  This keeps the
/// public arena flat while making the renderer's table state local and
/// deterministic.
pub(super) fn render_terminal_table(
    node: NodeRef<'_>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    // tbl's ordinary terminal output leaves three cells between adjacent
    // calculated columns.  The public TableCell span records exactly which
    // columns one payload occupies; allocate any span deficit to its final
    // column, as tblcalc does after the simple single-column pass.
    const TABLE_COLUMN_GAP: usize = 3;
    if terminal_previous_sibling(node).is_some_and(|previous| {
        previous.kind() == NodeKind::Table
            && !node
                .table_terminal()
                .is_some_and(|terminal| terminal.starts_table)
    }) {
        return Ok(());
    }
    let Some(parent) = node.parent() else {
        return Ok(());
    };
    let rows = parent
        .children()
        .skip_while(|sibling| sibling.id() != node.id())
        .enumerate()
        .take_while(|(index, sibling)| {
            sibling.kind() == NodeKind::Table
                && (*index == 0
                    || !sibling
                        .table_terminal()
                        .is_some_and(|terminal| terminal.starts_table))
        })
        .map(|(_, sibling)| sibling)
        .collect::<Vec<_>>();
    if rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(table_terminal_has_device_layout)
    {
        return render_terminal_styled_table(&rows, format, limits, indentation, output, maximum);
    }
    if rows.iter().all(|row| row.table_cells().is_empty()) {
        return Ok(());
    }

    let column_count = rows
        .iter()
        .map(|row| {
            row.table_cells()
                .iter()
                .map(|cell| usize::from(cell.column_span.max(1)))
                .sum::<usize>()
        })
        .max()
        .unwrap_or_default();
    if column_count == 0 {
        return Ok(());
    }
    let mut widths = vec![0_usize; column_count];
    for row in &rows {
        let mut column = 0_usize;
        for cell in row.table_cells() {
            let span = usize::from(cell.column_span.max(1));
            let text = cell.text.as_deref().map_or_else(String::new, |text| {
                render_terminal_visible_text(text, format, limits)
            });
            let rendered_width = display_width(text.trim_end());
            if span == 1 && column < widths.len() {
                widths[column] = widths[column].max(rendered_width);
            }
            column = column.saturating_add(span);
        }
    }
    for row in &rows {
        let mut column = 0_usize;
        for cell in row.table_cells() {
            let span = usize::from(cell.column_span.max(1));
            let text = cell.text.as_deref().map_or_else(String::new, |text| {
                render_terminal_visible_text(text, format, limits)
            });
            let rendered_width = display_width(text.trim_end());
            if span > 1 && column < widths.len() {
                let end = column.saturating_add(span).min(widths.len());
                let available = widths[column..end]
                    .iter()
                    .copied()
                    .sum::<usize>()
                    .saturating_add(
                        TABLE_COLUMN_GAP.saturating_mul(end.saturating_sub(column + 1)),
                    );
                if rendered_width > available {
                    let final_column = end.saturating_sub(1);
                    widths[final_column] = widths[final_column]
                        .saturating_add(rendered_width.saturating_sub(available));
                }
            }
            column = column.saturating_add(span);
        }
    }

    if !output.is_empty() {
        if terminal_previous_sibling(node)
            .is_some_and(|previous| previous.kind() == NodeKind::Table)
        {
            // A distinct `.TS` range consumes its predecessor's local
            // vertical-skip marker, then owns an ordinary paragraph gap.
            // Without this boundary, adjacent flat compatibility rows would
            // run together even though their source tables are separate.
            let _ = take_terminal_table_vertical_skip(output);
            append_blank_line(output, maximum)?;
        } else if terminal_table_follows_mdoc_prose(node) {
            // Keep the preceding mdoc phrase on its completed physical line
            // without introducing man-style paragraph vspace.  Any
            // keep-spacing marker remains immediately before this newline so
            // the final terminal width pass consumes it as line provenance.
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else {
            append_blank_line(output, maximum)?;
        }
    }
    for (row_index, row) in rows.iter().enumerate() {
        let mut line = String::new();
        let mut column = 0_usize;
        for cell in row.table_cells() {
            let span = usize::from(cell.column_span.max(1));
            let end = column.saturating_add(span).min(widths.len());
            if end <= column {
                break;
            }
            let field_width = widths[column..end]
                .iter()
                .copied()
                .sum::<usize>()
                .saturating_add(TABLE_COLUMN_GAP.saturating_mul(end.saturating_sub(column + 1)));
            if !cell.vertical_continuation {
                let text = cell.text.as_deref().map_or_else(String::new, |text| {
                    render_terminal_visible_text(text, format, limits)
                });
                let text = text.trim_end();
                let padding = field_width.saturating_sub(display_width(text));
                let leading = match cell.alignment {
                    TableAlignment::Left => 0,
                    TableAlignment::Center => padding / 2,
                    TableAlignment::Right => padding,
                };
                let target = widths[..column]
                    .iter()
                    .copied()
                    .sum::<usize>()
                    .saturating_add(TABLE_COLUMN_GAP.saturating_mul(column));
                if display_width(&line) < target {
                    line.push_str(&" ".repeat(target.saturating_sub(display_width(&line))));
                }
                line.push_str(&" ".repeat(leading));
                line.push_str(text);
            }
            column = column.saturating_add(span);
        }
        if line.trim().is_empty() {
            // A physical empty tbl data row is a device-level blank line only
            // when another row follows it.  tbl discards trailing empty rows;
            // emitting indentation here would leave a visible whitespace-only
            // line instead of the terminal's ordinary empty line.
            if rows
                .iter()
                .skip(row_index + 1)
                .any(|later| !later.table_cells().is_empty())
            {
                append(output, "\n", maximum)?;
            }
            continue;
        }
        append(output, &TERMINAL_KEEP_SPACING_MARKER.to_string(), maximum)?;
        append(output, &" ".repeat(indentation), maximum)?;
        append(output, line.trim_end(), maximum)?;
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Whether retained tbl layout affects terminal presentation beyond the
/// canonical `TableCell` payload.  Ordinary alignment-only rows still use the
/// compact compatibility path above, keeping its already-exact output stable.
pub(super) fn table_terminal_has_device_layout(row: &TableTerminalRow) -> bool {
    row.outer_border != TableTerminalBorder::None
        || row.all_box
        || row.centered
        || row.horizontal_rule != TableTerminalBorder::None
        || row.cells.iter().any(|cell| {
            cell.before_vertical_rules != 0
                || cell.after_vertical_rules != 0
                || cell.horizontal_rule != TableTerminalBorder::None
                || cell.spacing.is_some()
                || cell.font != TableTerminalFont::Roman
                || cell.width_expanding
        })
}

/// Render tbl's device-only box, rule, font, and spacing metadata.
///
/// The parser keeps this small presentation layer separate from the public
/// owned AST.  It is enough for terminal geometry while allowing engine
/// lowering and canonical AST differential to continue consuming the stable
/// `TableCell` projection alone.
#[allow(clippy::too_many_lines)] // tbl geometry is inherently one stateful pass.
pub(super) fn render_terminal_styled_table(
    rows: &[NodeRef<'_>],
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    const DEFAULT_GAP: usize = 3;
    let column_count = rows
        .iter()
        .map(|row| {
            row.table_terminal()
                .map_or(0, |terminal| terminal.cells.len())
                .max(
                    row.table_cells()
                        .iter()
                        .map(|cell| usize::from(cell.column_span.max(1)))
                        .sum(),
                )
        })
        .max()
        .unwrap_or_default();
    if column_count == 0 {
        return Ok(());
    }
    let table_right_margin =
        terminal_line_length_value(terminal_line_length_before(rows[0]), DEFAULT_RENDER_WIDTH);
    // tblcalc gives a `T{…T}` cell a bounded default field even before it
    // knows the final table width. An explicit `w` replaces that default.
    // Keep this private to terminal layout: public TableCell text remains one
    // normalized logical value for AST compatibility.
    let default_text_block_width = table_right_margin
        .saturating_add(column_count / 2)
        .checked_div(column_count.saturating_add(1))
        .unwrap_or(1)
        .max(1);

    let mut gaps = vec![DEFAULT_GAP; column_count.saturating_sub(1)];
    for row in rows {
        if let Some(terminal) = row.table_terminal() {
            for (index, cell) in terminal.cells.iter().enumerate().take(gaps.len()) {
                if let Some(spacing) = cell.spacing {
                    gaps[index] = usize::from(spacing);
                }
            }
        }
    }
    let mut widths = vec![0_usize; column_count];
    let mut expanding_columns = vec![false; column_count];
    let mut numeric_before = vec![0_usize; column_count];
    let mut numeric_after = vec![0_usize; column_count];
    let mut numeric_decimal = vec![false; column_count];
    for row in rows {
        let starts = table_terminal_cell_starts(row, column_count);
        let terminal = row.table_terminal();
        for (index, cell) in row.table_cells().iter().enumerate() {
            let Some(&column) = starts.get(index) else {
                break;
            };
            let span = usize::from(cell.column_span.max(1));
            let text = table_terminal_visible_cell_text(cell, terminal, column, format, limits);
            let horizontal_rule = terminal
                .and_then(|terminal| terminal.cells.get(column))
                .is_some_and(|cell| cell.horizontal_rule != TableTerminalBorder::None);
            if horizontal_rule {
                if span == 1 && column < widths.len() {
                    widths[column] = widths[column].max(1);
                }
                continue;
            }
            let width_ignored = terminal
                .and_then(|terminal| terminal.cells.get(column))
                .is_some_and(|cell| cell.width_ignored);
            if width_ignored {
                continue;
            }
            if span == 1 && column < widths.len() {
                if cell.text_block {
                    let field_width = terminal
                        .and_then(|terminal| terminal.cells.get(column))
                        .and_then(|cell| cell.minimum_width)
                        .map_or(default_text_block_width, usize::from)
                        .max(1);
                    let rendered_width = terminal_table_text_block_lines(&text, field_width)
                        .iter()
                        .map(|line| display_width(line))
                        .max()
                        .unwrap_or_default();
                    widths[column] = widths[column].max(rendered_width);
                } else if terminal
                    .and_then(|terminal| terminal.cells.get(column))
                    .is_some_and(|cell| cell.numeric)
                {
                    let (before, after, decimal) = table_terminal_numeric_metrics(text.trim_end());
                    numeric_before[column] = numeric_before[column].max(before);
                    numeric_after[column] = numeric_after[column].max(after);
                    numeric_decimal[column] |= decimal;
                } else {
                    let rendered_width = display_width(text.trim_end());
                    widths[column] = widths[column].max(rendered_width);
                }
            }
        }
    }
    for column in 0..widths.len() {
        if numeric_before[column] > 0 || numeric_decimal[column] {
            widths[column] = widths[column].max(
                numeric_before[column]
                    + usize::from(numeric_decimal[column])
                    + numeric_after[column],
            );
        }
    }
    // tbl's `w` modifier establishes a physical terminal field even when
    // the cell payload is shorter.  It applies before span deficits are
    // distributed, just as the device's column calculation does.
    for row in rows {
        let Some(terminal) = row.table_terminal() else {
            continue;
        };
        for (column, cell) in terminal.cells.iter().enumerate().take(widths.len()) {
            expanding_columns[column] |= cell.width_expanding;
            if let Some(width) = cell.minimum_width {
                widths[column] = widths[column].max(usize::from(width));
            }
        }
    }
    for row in rows {
        let starts = table_terminal_cell_starts(row, column_count);
        let terminal = row.table_terminal();
        for (index, cell) in row.table_cells().iter().enumerate() {
            let Some(&column) = starts.get(index) else {
                break;
            };
            let span = usize::from(cell.column_span.max(1));
            if span <= 1 || column >= widths.len() {
                continue;
            }
            let end = column.saturating_add(span).min(widths.len());
            let text = table_terminal_visible_cell_text(cell, terminal, column, format, limits);
            let available = widths[column..end].iter().copied().sum::<usize>()
                + gaps[column..end.saturating_sub(1)]
                    .iter()
                    .copied()
                    .sum::<usize>();
            let rendered_width = display_width(text.trim_end());
            if rendered_width > available {
                let final_column = end.saturating_sub(1);
                widths[final_column] =
                    widths[final_column].saturating_add(rendered_width.saturating_sub(available));
            }
        }
    }

    let outer = rows
        .iter()
        .filter_map(|row| row.table_terminal().map(|terminal| terminal.outer_border))
        .find(|border| *border != TableTerminalBorder::None)
        .unwrap_or(TableTerminalBorder::None);
    let all_box = rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(|terminal| terminal.all_box);
    let centered = rows
        .iter()
        .filter_map(|row| row.table_terminal())
        .any(|terminal| terminal.centered);
    // A vertical rule occurring on any layout row reserves the outer device
    // column for the whole table.  Individual rows may leave that column
    // blank, but their data must not slide left into it.  `term_tbl()` sets
    // this up while calculating the shared tbl grid, before it knows which
    // physical rows will actually paint the rule.
    let has_left_vertical_frame = outer == TableTerminalBorder::None
        && rows
            .iter()
            .filter_map(|row| row.table_terminal())
            .any(|terminal| {
                terminal
                    .cells
                    .first()
                    .is_some_and(|cell| cell.before_vertical_rules != 0)
            });
    let has_right_vertical_frame = outer == TableTerminalBorder::None
        && rows
            .iter()
            .filter_map(|row| row.table_terminal())
            .any(|terminal| {
                terminal
                    .cells
                    .last()
                    .is_some_and(|cell| cell.after_vertical_rules != 0)
            });
    let boundary_layout = rows.iter().find_map(|row| row.table_terminal());
    // tbl centres the whole calculated grid once, not each physical row.  A
    // right or left layout frame contributes a single cell to that grid;
    // `box` and `doublebox` both contribute their two outer cells.  This
    // intentionally differs from the visual rule length, which may include
    // additional intersection glyphs.
    let center_offset = centered.then(|| {
        let right_margin =
            terminal_line_length_value(terminal_line_length_before(rows[0]), DEFAULT_RENDER_WIDTH);
        let outer_width = if outer == TableTerminalBorder::None {
            usize::from(has_left_vertical_frame) + usize::from(has_right_vertical_frame)
        } else {
            2
        };
        let table_width = widths
            .iter()
            .sum::<usize>()
            .saturating_add(gaps.iter().sum::<usize>())
            .saturating_add(outer_width);
        let centered_width = table_width.saturating_sub(usize::from(
            indentation.saturating_add(table_width) > right_margin,
        ));
        if indentation.saturating_add(right_margin) > centered_width {
            indentation
                .saturating_add(right_margin)
                .saturating_sub(centered_width)
                / 2
        } else {
            0
        }
    });
    let bottom_layout = rows
        .iter()
        .rev()
        .find_map(|row| row.table_terminal())
        .or(boundary_layout);
    if outer != TableTerminalBorder::None {
        for width in &mut widths {
            *width = (*width).max(1);
        }
    }
    // tblcalc treats `x` fields as an equal-width partition of the remaining
    // device width; their content-derived widths are only a lower-bound when
    // the fixed fields already overflow the right margin.  The 0.4995 bias is
    // intentional: it reproduces mandoc's historical, left-biased rounding
    // of indivisible cells (and its observable three-column geometry).
    // The geometry is renderer-private: the owned `TableCell` keeps only its
    // compatible text/alignment/span projection.
    let expanding_columns = expanding_columns
        .into_iter()
        .enumerate()
        .filter_map(|(column, expands)| expands.then_some(column))
        .collect::<Vec<_>>();
    if !expanding_columns.is_empty() {
        let frame_width = usize::from(outer != TableTerminalBorder::None) * 2;
        let fixed_width = widths
            .iter()
            .enumerate()
            .filter(|(column, _)| !expanding_columns.contains(column))
            .map(|(_, width)| *width)
            .sum::<usize>()
            .saturating_add(DEFAULT_GAP.saturating_mul(column_count.saturating_sub(1)))
            .saturating_add(frame_width);
        // Table geometry is calculated at its source position, not during
        // the later generic wrapping pass.  Reconstruct the preceding `.ll`
        // register here so `x` never expands a table that already exceeds a
        // temporarily narrowed terminal field.
        let target_width = table_right_margin.saturating_sub(indentation);
        if target_width > fixed_width {
            let available = target_width.saturating_sub(fixed_width);
            let count = expanding_columns.len();
            // Mandoc intentionally carries GNU tbl's five-column rounding
            // quirk.  The exception is observable in the upstream expand
            // fixture and also governs tables with six expandable fields.
            let quirk_position = if count == 5 {
                match available % count + 2 {
                    3 | 4 => Some(available % count + 2),
                    _ => None,
                }
            } else {
                None
            };
            let mut allocated = 0_usize;
            for (position, column) in expanding_columns.into_iter().enumerate() {
                // Equivalent to tblcalc's
                // `(double) available * position / count - allocated +
                // 0.4995`, but kept integral so even pathological source
                // dimensions cannot lose precision before the hard bounds
                // reject their rendered output.
                let numerator = available.saturating_mul(position + 1);
                let cumulative =
                    numerator / count + usize::from((numerator % count).saturating_mul(2) > count);
                let mut width = cumulative.saturating_sub(allocated);
                if quirk_position == Some(position + 1) {
                    width = width.saturating_sub(1);
                }
                widths[column] = width;
                allocated = allocated.saturating_add(width);
            }
        }
    }
    if !output.is_empty() {
        if terminal_previous_sibling(rows[0])
            .is_some_and(|previous| previous.kind() == NodeKind::Table)
        {
            let _ = take_terminal_table_vertical_skip(output);
            append_blank_line(output, maximum)?;
        } else if terminal_table_follows_mdoc_prose(rows[0]) {
            if !output.ends_with('\n') {
                append(output, "\n", maximum)?;
            }
        } else {
            append_blank_line(output, maximum)?;
        }
    }
    if outer != TableTerminalBorder::None {
        // In ASCII `doublebox`, tbl emits the heavy outer rule first. That
        // first frame ignores the first data layout's internal crossings;
        // the following ordinary box rule carries them. Reusing the layout
        // for both lines incorrectly duplicates a top `+---+` intersection.
        if outer == TableTerminalBorder::Double {
            append_terminal_table_rule(
                &widths,
                &gaps,
                None,
                outer,
                false,
                center_offset,
                indentation,
                output,
                maximum,
            )?;
        }
        append_terminal_table_rule(
            &widths,
            &gaps,
            boundary_layout,
            outer,
            all_box,
            center_offset,
            indentation,
            output,
            maximum,
        )?;
    }
    let mut wrote_content = false;
    for (row_index, row) in rows.iter().enumerate() {
        let terminal = row.table_terminal().cloned().unwrap_or_default();
        if terminal.horizontal_rule != TableTerminalBorder::None
            || (row.table_cells().is_empty()
                && terminal
                    .cells
                    .iter()
                    .any(|cell| cell.horizontal_rule != TableTerminalBorder::None))
        {
            // A full `_`/`=` span sits between physical data rows. Its
            // intersections are selected from the preceding row's layout;
            // only the opening rule has no predecessor and falls back to
            // its own retained layout. This is the same left-hand span
            // context passed as `spp` to upstream `tbl_hrule()`.
            let rule_layout = row_index
                .checked_sub(1)
                .and_then(|previous| rows.get(previous))
                .and_then(|previous| previous.table_terminal())
                .filter(|previous| {
                    previous.cells.iter().any(|cell| {
                        cell.before_vertical_rules != 0 || cell.after_vertical_rules != 0
                    })
                })
                .unwrap_or(&terminal);
            let has_global_vertical_frame =
                rows.iter()
                    .filter_map(|row| row.table_terminal())
                    .any(|layout| {
                        layout.cells.iter().any(|cell| {
                            cell.before_vertical_rules != 0 || cell.after_vertical_rules != 0
                        })
                    });
            let needs_solid_global_rule =
                has_global_vertical_frame
                    && terminal.cells.iter().all(|cell| {
                        cell.before_vertical_rules == 0 && cell.after_vertical_rules == 0
                    })
                    && rule_layout.cells.iter().all(|cell| {
                        cell.before_vertical_rules == 0 && cell.after_vertical_rules == 0
                    });
            let rule_layout = if needs_solid_global_rule {
                boundary_layout.unwrap_or(rule_layout)
            } else {
                rule_layout
            };
            let output_start = output.len();
            append_terminal_table_rule(
                &widths,
                &gaps,
                Some(rule_layout),
                outer,
                all_box,
                center_offset,
                indentation,
                output,
                maximum,
            )?;
            if needs_solid_global_rule {
                let character = if terminal.horizontal_rule == TableTerminalBorder::Double {
                    '='
                } else {
                    '-'
                };
                let rendered = output[output_start..].replace('+', &character.to_string());
                output.replace_range(output_start.., &rendered);
            }
            continue;
        }
        if row.table_cells().is_empty()
            && outer == TableTerminalBorder::None
            && !all_box
            && terminal.horizontal_rule == TableTerminalBorder::None
            && terminal.cells.iter().all(|cell| {
                cell.before_vertical_rules == 0
                    && cell.after_vertical_rules == 0
                    && cell.horizontal_rule == TableTerminalBorder::None
            })
        {
            // A format-only empty data row still advances tbl's selected
            // layout (for example `lb`, `li`, `lb`), but font state has no
            // glyph to emit.  Keep a true blank device line only between
            // content rows; tbl discards one at the end of the table.
            if rows
                .iter()
                .skip(row_index + 1)
                .any(|later| !later.table_cells().is_empty())
            {
                append(output, "\n", maximum)?;
            }
            continue;
        }
        append_terminal_table_content(
            *row,
            &widths,
            &gaps,
            &numeric_before,
            &terminal,
            row_index
                .checked_sub(1)
                .and_then(|previous| rows.get(previous))
                .and_then(|previous| previous.table_terminal()),
            rows.get(row_index + 1)
                .and_then(|next| next.table_terminal()),
            outer,
            all_box,
            has_left_vertical_frame,
            has_right_vertical_frame,
            center_offset,
            format,
            limits,
            indentation,
            output,
            maximum,
        )?;
        wrote_content = true;
        // `allbox` contributes its own boundary before every later content
        // row.  An authored `_`/`=` layout row remains an additional device
        // rule between those rows, but a terminal layout rule already shares
        // the bottom frame and therefore does not need another allbox rule.
        let has_later_content = rows
            .iter()
            .skip(row_index + 1)
            .any(|later| !later.table_cells().is_empty());
        if all_box && has_later_content {
            let next_layout = rows
                .get(row_index + 1)
                .and_then(|next| next.table_terminal());
            let next_manual_rule = next_layout
                .filter(|terminal| terminal.horizontal_rule != TableTerminalBorder::None);
            let next_double_intersection = next_layout.filter(|next| {
                next.cells.windows(2).any(|cells| {
                    cells[0].after_vertical_rules >= 2 || cells[1].before_vertical_rules >= 2
                })
            });
            // `allbox` is drawn between data spans, so an internal double
            // vertical edge on the preceding span meets that rule. Preserve
            // the current-row intersection rather than replacing it with a
            // featureless allbox line.
            let current_double_intersection = terminal.cells.windows(2).any(|cells| {
                cells[0].after_vertical_rules >= 2 || cells[1].before_vertical_rules >= 2
            });
            append_terminal_table_rule(
                &widths,
                &gaps,
                current_double_intersection
                    .then_some(&terminal)
                    .or(next_double_intersection)
                    .or(next_manual_rule),
                outer,
                all_box,
                center_offset,
                indentation,
                output,
                maximum,
            )?;
        }
    }
    if wrote_content && outer != TableTerminalBorder::None {
        append_terminal_table_rule(
            &widths,
            &gaps,
            bottom_layout,
            outer,
            all_box,
            center_offset,
            indentation,
            output,
            maximum,
        )?;
        if outer == TableTerminalBorder::Double {
            append_terminal_table_rule(
                &widths,
                &gaps,
                None,
                outer,
                false,
                center_offset,
                indentation,
                output,
                maximum,
            )?;
        }
    }
    // Ordinary paragraph and footer spacing consumes this one table-local
    // slot. A standalone leading vertical layout line instead owns the
    // following field boundary, so its paragraph keeps the normal blank row.
    // Sections and explicit `.sp` clear the ordinary table-local marker
    // before their own handling.
    let carries_leading_vertical_layout = outer == TableTerminalBorder::None
        && rows.iter().any(|row| {
            row.table_terminal()
                .and_then(|terminal| terminal.cells.first())
                .is_some_and(|cell| cell.before_vertical_rules != 0)
        });
    let carries_layout_horizontal_rule = rows.iter().any(|row| {
        row.table_terminal().is_some_and(|terminal| {
            terminal
                .cells
                .iter()
                .any(|cell| cell.horizontal_rule != TableTerminalBorder::None)
        })
    });
    // `term_tbl()` always records the trailing device slot of a boxed table,
    // including when its final layout row contains a partial horizontal rule.
    // Only borderless layout-only rows have the special ownership rules
    // below; otherwise the following `.sp` would manufacture a second blank
    // after the visible box frame.
    if outer != TableTerminalBorder::None
        || (!carries_leading_vertical_layout && !carries_layout_horizontal_rule)
    {
        let trailing_slots = match outer {
            TableTerminalBorder::None => 0,
            TableTerminalBorder::Single => 1,
            TableTerminalBorder::Double => 2,
        };
        for _ in 0..trailing_slots {
            mark_terminal_table_vertical_skip(output);
        }
    }
    Ok(())
}

/// mdoc's table preprocessor keeps a table directly below the preceding
/// Body phrase.  The man device instead gives a table its ordinary paragraph
/// separator.  The generated table row has no public macro set of its own,
/// but its enclosing section retains the package's exact macro spelling.
pub(super) fn terminal_table_follows_mdoc_prose(node: NodeRef<'_>) -> bool {
    node.ancestors()
        .any(|ancestor| matches!(ancestor.macro_name(), Some("Sh" | "Ss")))
}

pub(in crate::renderer) fn table_terminal_cell_starts(
    row: &NodeRef<'_>,
    column_count: usize,
) -> Vec<usize> {
    let mut starts = row
        .table_terminal()
        .map(|terminal| {
            if terminal.data_columns.len() >= row.table_cells().len() {
                return terminal
                    .data_columns
                    .iter()
                    .take(row.table_cells().len())
                    .map(|column| usize::from(*column))
                    .collect();
            }
            terminal
                .cells
                .iter()
                .enumerate()
                .filter_map(|(index, cell)| {
                    (!cell.span && cell.horizontal_rule == TableTerminalBorder::None)
                        .then_some(index)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut next = starts.last().copied().map_or(0, |column| column + 1);
    while starts.len() < row.table_cells().len() && next < column_count {
        starts.push(next);
        next += 1;
    }
    starts
}

pub(super) fn table_terminal_visible_cell_text(
    cell: &crate::TableCell,
    terminal: Option<&TableTerminalRow>,
    column: usize,
    format: RenderFormat,
    limits: &Limits,
) -> String {
    let text = cell.text.as_deref().map_or_else(String::new, |text| {
        render_terminal_visible_text(text, format, limits)
    });
    match terminal
        .and_then(|terminal| terminal.cells.get(column))
        .map_or(TableTerminalFont::Roman, |cell| cell.font)
    {
        TableTerminalFont::Roman => text,
        TableTerminalFont::Bold => render_terminal_font(&text, TerminalFont::Bold),
        TableTerminalFont::Italic => render_terminal_font(&text, TerminalFont::Italic),
    }
}

/// Wrap one normalized tbl `T{…T}` payload at the field selected by
/// `tblcalc_data()`. Text-block source lines have already been normalized to
/// ordinary spaces by preprocessing, and the C device likewise reflows them
/// at word boundaries without splitting an overwide word.
pub(in crate::renderer) fn terminal_table_text_block_lines(
    text: &str,
    width: usize,
) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let separator = usize::from(!line.is_empty());
        if !line.is_empty()
            && display_width(&line)
                .saturating_add(separator)
                .saturating_add(display_width(word))
                > width
        {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

#[allow(clippy::too_many_arguments)] // A rule shares the table renderer's bounded output context.
pub(super) fn append_terminal_table_rule(
    widths: &[usize],
    gaps: &[usize],
    terminal: Option<&TableTerminalRow>,
    outer: TableTerminalBorder,
    all_box: bool,
    center_offset: Option<usize>,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let rule = terminal.and_then(|row| {
        (row.horizontal_rule != TableTerminalBorder::None).then_some(row.horizontal_rule)
    });
    let line_character = if rule == Some(TableTerminalBorder::Double) {
        '='
    } else {
        '-'
    };
    let mut line = String::new();
    let leading_rules = if outer == TableTerminalBorder::None {
        terminal
            .and_then(|row| row.cells.first())
            // tbl retains at most one outer edge glyph even when the source
            // layout spells a double `||` boundary. Double rules remain
            // meaningful only between calculated columns.
            .map_or(0, |cell| usize::from(cell.before_vertical_rules.min(1)))
    } else {
        1
    };
    line.push_str(&"+".repeat(leading_rules));
    for column in 0..widths.len() {
        line.push_str(&line_character.to_string().repeat(widths[column]));
        if column + 1 == widths.len() {
            let trailing_rules = if outer == TableTerminalBorder::None {
                terminal
                    .and_then(|row| row.cells.last())
                    .map_or(0, |cell| usize::from(cell.after_vertical_rules.min(1)))
            } else {
                1
            };
            if trailing_rules > 0 {
                line.push(line_character);
                line.push_str(&"+".repeat(trailing_rules));
            }
            // A horizontal span crossing into a standalone leading vertical
            // layout row continues through that row's one-cell terminal
            // boundary.  It is not an outer box edge, hence one extra rule
            // glyph rather than a closing `+`.
            if rule.is_some()
                && outer == TableTerminalBorder::None
                && widths.len() == 1
                && leading_rules != 0
                && trailing_rules == 0
            {
                line.push(line_character);
            }
            continue;
        }
        let (after, before, rules) =
            table_terminal_boundary(terminal, None, None, column, gaps[column], all_box);
        line.push_str(&line_character.to_string().repeat(after));
        if rules != 0 {
            line.push_str(&"+".repeat(rules));
        }
        line.push_str(&line_character.to_string().repeat(before));
        // A standalone full-width tbl rule owns one final device cell at
        // each participating layout boundary. Partial horizontal layout
        // cells are handled by the data-row geometry instead; applying this
        // extension to the outer box frame would overrun that frame.
        if rule.is_some()
            && terminal.is_some_and(|row| {
                row.cells
                    .get(column)
                    .is_some_and(|cell| cell.horizontal_rule != TableTerminalBorder::None)
                    || row
                        .cells
                        .get(column + 1)
                        .is_some_and(|cell| cell.horizontal_rule != TableTerminalBorder::None)
            })
        {
            line.push(line_character);
        }
    }
    append_terminal_table_line_prefix(output, center_offset, indentation, maximum)?;
    append(output, &line, maximum)?;
    append(output, "\n", maximum)
}

#[allow(clippy::too_many_arguments)] // Content shares the table renderer's bounded output context.
pub(super) fn append_terminal_table_content(
    row: NodeRef<'_>,
    widths: &[usize],
    gaps: &[usize],
    numeric_before: &[usize],
    terminal: &TableTerminalRow,
    previous_terminal: Option<&TableTerminalRow>,
    next_terminal: Option<&TableTerminalRow>,
    outer: TableTerminalBorder,
    all_box: bool,
    has_left_vertical_frame: bool,
    has_right_vertical_frame: bool,
    center_offset: Option<usize>,
    format: RenderFormat,
    limits: &Limits,
    indentation: usize,
    output: &mut String,
    maximum: usize,
) -> Result<(), RenderError> {
    let starts = table_terminal_cell_starts(&row, widths.len());
    let mut cells = starts
        .iter()
        .copied()
        .zip(row.table_cells())
        .collect::<Vec<_>>();
    cells.sort_by_key(|(start, _)| *start);
    let text_block_lines = cells
        .iter()
        .filter(|(_, cell)| cell.text_block)
        .map(|(column, cell)| {
            let span = usize::from(cell.column_span.max(1)).min(widths.len() - *column);
            let end = column + span;
            let field_width = widths[*column..end].iter().copied().sum::<usize>()
                + gaps[*column..end.saturating_sub(1)]
                    .iter()
                    .copied()
                    .sum::<usize>();
            let text =
                table_terminal_visible_cell_text(cell, Some(terminal), *column, format, limits);
            terminal_table_text_block_lines(&text, field_width).len()
        })
        .max()
        .unwrap_or(1);
    let leading_horizontal = terminal
        .cells
        .first()
        .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule);
    let previous_leading_rules = if leading_horizontal == TableTerminalBorder::None {
        0
    } else {
        previous_terminal
            .and_then(|previous| previous.cells.first())
            .map_or(0, |cell| usize::from(cell.before_vertical_rules))
    };
    let leading_rules = if outer == TableTerminalBorder::None {
        terminal
            .cells
            .first()
            .map_or(0, |cell| usize::from(cell.before_vertical_rules))
            .max(
                next_terminal
                    .and_then(|next| next.cells.first())
                    .filter(|cell| !cell.leading_vertical_from_standalone)
                    .map_or(0, |cell| usize::from(cell.before_vertical_rules)),
            )
            .max(previous_leading_rules)
            .min(1)
    } else {
        1
    };
    for text_block_line in 0..text_block_lines {
        let mut line = String::new();
        if leading_rules != 0 {
            // An authored horizontal cell meets an outer vertical device
            // frame at a `+`; without it the frame is simply a `|`.
            line.push(if leading_horizontal == TableTerminalBorder::None {
                '|'
            } else {
                '+'
            });
        } else if has_left_vertical_frame {
            line.push(' ');
        }
        let mut column = 0_usize;
        let mut cell_index = 0_usize;
        while column < widths.len() {
            let (span, alignment, vertical, horizontal_rule, text, text_block) =
                if let Some((start, cell)) = cells.get(cell_index)
                    && *start == column
                {
                    cell_index += 1;
                    let span = usize::from(cell.column_span.max(1)).min(widths.len() - column);
                    (
                        span,
                        cell.alignment,
                        cell.vertical_continuation,
                        terminal
                            .cells
                            .get(column)
                            .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule),
                        table_terminal_visible_cell_text(
                            cell,
                            Some(terminal),
                            column,
                            format,
                            limits,
                        ),
                        cell.text_block,
                    )
                } else {
                    (
                        1,
                        TableAlignment::Left,
                        false,
                        terminal
                            .cells
                            .get(column)
                            .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule),
                        String::new(),
                        false,
                    )
                };
            let end = column + span;
            let field_width = widths[column..end].iter().copied().sum::<usize>()
                + gaps[column..end.saturating_sub(1)]
                    .iter()
                    .copied()
                    .sum::<usize>();
            let text = if text_block {
                terminal_table_text_block_lines(&text, field_width)
                    .into_iter()
                    .nth(text_block_line)
                    .unwrap_or_default()
            } else if text_block_line == 0 {
                text
            } else {
                String::new()
            };
            let text = text.trim_end();
            let numeric = !text_block
                && terminal
                    .cells
                    .get(column)
                    .is_some_and(|cell| cell.numeric && !cell.width_ignored);
            let padding = field_width.saturating_sub(display_width(text));
            let leading = if numeric {
                let (before, _, _) = table_terminal_numeric_metrics(text);
                numeric_before
                    .get(column)
                    .copied()
                    .unwrap_or_default()
                    .saturating_sub(before)
            } else {
                match alignment {
                    TableAlignment::Left => 0,
                    TableAlignment::Center => padding / 2,
                    TableAlignment::Right => padding,
                }
            };
            if horizontal_rule != TableTerminalBorder::None {
                let rule_character = if horizontal_rule == TableTerminalBorder::Double {
                    '='
                } else {
                    '-'
                };
                line.push_str(&rule_character.to_string().repeat(field_width));
            } else if vertical {
                line.push_str(&" ".repeat(field_width));
            } else {
                line.push_str(&" ".repeat(leading));
                line.push_str(text);
                line.push_str(&" ".repeat(padding.saturating_sub(leading)));
            }
            if end == widths.len() {
                let previous_trailing_rules = if horizontal_rule == TableTerminalBorder::None {
                    0
                } else {
                    previous_terminal
                        .and_then(|previous| previous.cells.last())
                        .map_or(0, |cell| usize::from(cell.after_vertical_rules))
                };
                let trailing_rules = if outer == TableTerminalBorder::None {
                    terminal
                        .cells
                        .last()
                        .map_or(0, |cell| usize::from(cell.after_vertical_rules))
                        .max(
                            next_terminal
                                .and_then(|next| next.cells.last())
                                .map_or(0, |cell| usize::from(cell.after_vertical_rules)),
                        )
                        .max(previous_trailing_rules)
                        .min(1)
                } else {
                    1
                };
                if horizontal_rule != TableTerminalBorder::None {
                    // The final horizontal layout cell reaches one device
                    // position past its calculated field.  If that position
                    // also carries the outer vertical frame it is the
                    // ordinary ASCII tbl intersection glyph.
                    line.push(
                        table_terminal_rule_character(horizontal_rule)
                            .expect("horizontal rule was checked above"),
                    );
                    if trailing_rules > 0 {
                        line.push('+');
                    }
                } else if trailing_rules > 0 {
                    line.push(' ');
                    line.push('|');
                } else if has_right_vertical_frame {
                    // Preserve the shared grid's right edge even on a row
                    // where no segment of that edge is currently painted.
                    // It is trailing whitespace and will intentionally be
                    // removed below, but makes this branch explicit beside
                    // the analogous leading-frame reservation.
                    line.push(' ');
                }
                break;
            }
            let (after, before, rules) = table_terminal_boundary(
                Some(terminal),
                previous_terminal,
                next_terminal,
                end - 1,
                gaps[end - 1],
                all_box,
            );
            let right_horizontal = terminal
                .cells
                .get(end)
                .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule);
            append_terminal_table_boundary(
                &mut line,
                after,
                before,
                rules,
                horizontal_rule,
                right_horizontal,
            );
            column = end;
        }
        append_terminal_table_line_prefix(output, center_offset, indentation, maximum)?;
        append(output, line.trim_end(), maximum)?;
        append(output, "\n", maximum)?;
    }
    Ok(())
}

/// Begin one calculated tbl device line.  Ordinary tables render in their
/// surrounding text field; `center` tables instead use that field's right
/// edge as the centering width, so their source indentation must not become a
/// visible prefix before the final device pass.
pub(super) fn append_terminal_table_line_prefix(
    output: &mut String,
    center_offset: Option<usize>,
    indentation: usize,
    maximum: usize,
) -> Result<(), RenderError> {
    append(output, &TERMINAL_KEEP_SPACING_MARKER.to_string(), maximum)?;
    append(
        output,
        &" ".repeat(center_offset.unwrap_or(indentation)),
        maximum,
    )
}

pub(super) fn table_terminal_numeric_metrics(value: &str) -> (usize, usize, bool) {
    let value = value.trim_end();
    let Some((before, after)) = value.rsplit_once('.') else {
        return (display_width(value), 0, false);
    };
    (display_width(before), display_width(after), true)
}

/// Draw one inter-column tbl device field.  A horizontal layout cell owns its
/// adjacent half of the spacing field; a vertical edge in the centre turns
/// the meeting point into `+` rather than replacing the horizontal rule with
/// a bare `|`.  Keeping this distinct from public `TableCell` state mirrors
/// the terminal-only layout graph used by upstream `tbl_term.c`.
pub(super) fn append_terminal_table_boundary(
    line: &mut String,
    mut after: usize,
    mut before: usize,
    rules: usize,
    left_horizontal: TableTerminalBorder,
    right_horizontal: TableTerminalBorder,
) {
    let left = table_terminal_rule_character(left_horizontal);
    let right = table_terminal_rule_character(right_horizontal);
    // A rule entering from the right starts at the centre of the ordinary
    // three-cell tbl gap, not after it.  Shift that one device position from
    // the left cell's blank half to the rule-owning right cell.  This is the
    // asymmetric `tbl_direct_border()` placement used by the ASCII device.
    if rules == 0 && left.is_none() && right.is_some() && after != 0 {
        after -= 1;
        before += 1;
    }
    if rules == 0 {
        line.extend(std::iter::repeat_n(left.unwrap_or(' '), after));
        line.extend(std::iter::repeat_n(right.unwrap_or(' '), before));
        return;
    }
    line.extend(std::iter::repeat_n(left.unwrap_or(' '), after));
    if left.is_some() || right.is_some() {
        // For a double vertical boundary, the horizontal line arriving from
        // the right crosses both ASCII device columns (`++`).  A line ending
        // on the left crosses only the first one (`+|`).  This directional
        // asymmetry is inherited from groff tbl's two-cell border encoding.
        if right.is_some() {
            line.extend(std::iter::repeat_n('+', rules));
        } else {
            line.push('+');
            line.extend(std::iter::repeat_n('|', rules.saturating_sub(1)));
        }
    } else {
        line.extend(std::iter::repeat_n('|', rules));
    }
    line.extend(std::iter::repeat_n(right.unwrap_or(' '), before));
}

pub(super) fn table_terminal_rule_character(border: TableTerminalBorder) -> Option<char> {
    match border {
        TableTerminalBorder::None => None,
        TableTerminalBorder::Single => Some('-'),
        TableTerminalBorder::Double => Some('='),
    }
}

pub(super) fn table_terminal_boundary(
    terminal: Option<&TableTerminalRow>,
    previous_terminal: Option<&TableTerminalRow>,
    next_terminal: Option<&TableTerminalRow>,
    column: usize,
    gap: usize,
    all_box: bool,
) -> (usize, usize, usize) {
    if terminal.is_some_and(|row| row.cells.get(column + 1).is_some_and(|cell| cell.span)) {
        return (gap, 0, 0);
    }
    let current_left_horizontal = terminal
        .and_then(|row| row.cells.get(column))
        .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule);
    let current_right_horizontal = terminal
        .and_then(|row| row.cells.get(column + 1))
        .map_or(TableTerminalBorder::None, |cell| cell.horizontal_rule);
    let previous_after = if current_left_horizontal == TableTerminalBorder::None {
        0
    } else {
        previous_terminal
            .and_then(|previous| previous.cells.get(column))
            .filter(|cell| cell.horizontal_rule == TableTerminalBorder::None)
            .map_or(0, |cell| usize::from(cell.after_vertical_rules))
    };
    let previous_before = if current_right_horizontal == TableTerminalBorder::None {
        0
    } else {
        previous_terminal.map_or(0, |previous| {
            let after_left = previous
                .cells
                .get(column)
                .filter(|cell| cell.horizontal_rule == TableTerminalBorder::None)
                .map_or(0, |cell| usize::from(cell.after_vertical_rules));
            let before_right = previous
                .cells
                .get(column + 1)
                .filter(|cell| cell.horizontal_rule == TableTerminalBorder::None)
                .map_or(0, |cell| usize::from(cell.before_vertical_rules));
            after_left.max(before_right)
        })
    };
    let after = terminal
        .and_then(|row| row.cells.get(column))
        .map_or(0, |cell| usize::from(cell.after_vertical_rules))
        .max(
            next_terminal
                .and_then(|row| row.cells.get(column))
                .map_or(0, |cell| {
                    usize::from(cell.after_vertical_rules).min(if all_box { 1 } else { usize::MAX })
                }),
        )
        .max(previous_after);
    let before = terminal
        .and_then(|row| row.cells.get(column + 1))
        .map_or(0, |cell| usize::from(cell.before_vertical_rules))
        .max(
            next_terminal
                .and_then(|row| row.cells.get(column + 1))
                .map_or(0, |cell| {
                    usize::from(cell.before_vertical_rules).min(if all_box {
                        1
                    } else {
                        usize::MAX
                    })
                }),
        )
        .max(previous_before);
    let rules = after.max(before).max(usize::from(all_box));
    // ASCII tbl has only one drawable crossing cell in a one- or two-cell
    // inter-column field. A double downward frame gets its second glyph only
    // once the authored spacing supplies the extra device position.
    let rules = if rules == 2 && gap <= 2 { 1 } else { rules };
    let spaces = gap.saturating_sub(rules);
    (spaces.div_ceil(2), spaces / 2, rules)
}
