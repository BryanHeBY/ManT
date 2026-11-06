//! Checked, synchronous calls into the pinned table width solver.
mod input;
use super::raw;
use crate::{Node, TableColumnMetrics};
use std::ffi::{CStr, CString, c_char, c_void};

pub(crate) fn table_hline_input(argument: &str) -> Option<bool> {
    let argument = CString::new(argument).ok()?;
    match unsafe { raw::mant_mandoc_table_hline_input(argument.as_ptr()) } {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

pub(crate) fn table_hmotion_input(argument: &str, backbefore: bool) -> bool {
    CString::new(argument).is_ok_and(|argument| unsafe {
        raw::mant_mandoc_table_hmotion_input(argument.as_ptr(), i32::from(backbefore)) != 0
    })
}

pub(crate) fn table_line_length(current: usize, previous: usize, argument: Option<&str>) -> usize {
    let argument = argument.and_then(|argument| CString::new(argument).ok());
    unsafe {
        raw::mant_mandoc_table_line_length(
            current.min(24000),
            previous.min(24000),
            argument
                .as_ref()
                .map_or(std::ptr::null(), |argument| argument.as_ptr()),
        )
    }
}

struct Measurement<F> {
    callback: F,
    failed: bool,
    bytes_left: usize,
    units_left: usize,
    maximum_width: usize,
}

impl<F> Measurement<F> {
    fn new(callback: F, bytes: usize, cells: usize, maximum_width: usize) -> Self {
        Self {
            callback,
            failed: false,
            bytes_left: bytes
                .saturating_mul(8)
                .saturating_add(cells.saturating_mul(16))
                .saturating_add(16),
            units_left: input::MAX_UNITS,
            maximum_width,
        }
    }
}

extern "C" fn measure<F: FnMut(&str) -> usize>(text: *const c_char, context: *mut c_void) -> usize {
    // The shim calls synchronously, with a NUL-terminated borrowed string.
    let state = unsafe { &mut *context.cast::<Measurement<F>>() };
    if state.failed {
        return 0;
    }
    let text = unsafe { CStr::from_ptr(text) };
    let Some(bytes_left) = state
        .bytes_left
        .checked_sub(text.to_bytes().len().saturating_add(1))
    else {
        state.failed = true;
        return 0;
    };
    state.bytes_left = bytes_left;
    let Ok(text) = text.to_str() else {
        state.failed = true;
        return 0;
    };
    if let Ok(width) =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (state.callback)(text)))
        && width <= state.maximum_width
        && let Some(units_left) = state.units_left.checked_sub(width)
    {
        state.units_left = units_left;
        width
    } else {
        state.failed = true;
        0
    }
}

pub(crate) fn measure_table_columns<F: FnMut(&str) -> usize>(
    rows: &[&Node],
    offset: usize,
    margin: usize,
    callback: F,
) -> Result<Vec<TableColumnMetrics>, String> {
    let first = rows.first().ok_or("no table spans")?;
    let columns = first
        .table_column_count
        .ok_or("missing table column count")?;
    if columns == 0 || columns > 250_000 || offset > input::MAX_UNITS || margin > input::MAX_UNITS {
        return Err("invalid table column count".into());
    }
    let layout = first.table_layout.as_ref().ok_or("missing table layout")?;
    let input = input::prepare(rows, columns)?;
    let edges = u32::from(layout.width_edge_columns);
    if edges > 2 {
        return Err("invalid table width edges".into());
    }
    let mut widths = vec![0; columns as usize];
    let mut spacings = vec![0; columns as usize];
    let mut decimals = vec![0; columns as usize];
    let mut state = Measurement::new(
        callback,
        input.bytes,
        input.cells.len(),
        input::MAX_UNITS / columns as usize,
    );
    let ok = unsafe {
        raw::mant_mandoc_table_measure(
            input.cells.as_ptr(),
            input.cells.len(),
            columns,
            layout.decimal_point,
            edges,
            offset,
            margin,
            measure::<F>,
            std::ptr::from_mut(&mut state).cast(),
            widths.as_mut_ptr(),
            spacings.as_mut_ptr(),
            decimals.as_mut_ptr(),
        )
    };
    if ok == 0 || state.failed {
        return Err("native table measurement failed".into());
    }
    Ok(widths
        .into_iter()
        .zip(spacings)
        .zip(decimals)
        .map(
            |((width_basic_units, spacing_columns), decimal_basic_units)| TableColumnMetrics {
                width_basic_units,
                spacing_columns,
                decimal_basic_units,
            },
        )
        .collect())
}

pub(crate) fn number_padding<F: FnMut(&str) -> usize>(
    column: TableColumnMetrics,
    text: &str,
    decimal: u8,
    callback: F,
) -> Result<usize, String> {
    if text.len() > 128 * 1024 * 1024
        || column.width_basic_units > input::MAX_UNITS
        || column.decimal_basic_units > input::MAX_UNITS
    {
        return Err("table number measurement budget exceeded".into());
    }
    let mut state = Measurement::new(callback, text.len(), 1, input::MAX_UNITS);
    let text = CString::new(text).map_err(|_| "NUL in table number")?;
    let pad = unsafe {
        raw::mant_mandoc_table_number_padding(
            text.as_ptr(),
            decimal,
            column.width_basic_units,
            column.decimal_basic_units,
            measure::<F>,
            std::ptr::from_mut(&mut state).cast(),
        )
    };
    if state.failed {
        Err("native number measurement failed".into())
    } else {
        Ok(pad)
    }
}
