//! Bounded borrowed inputs, kept alive for one native width calculation.
use super::super::raw::CTableMeasureCell;
use crate::{Node, TableRowKind};
use std::{collections::BTreeSet, ffi::CString};

const MAX_ITEMS: usize = 250_000;
const MAX_BYTES: usize = 128 * 1024 * 1024;
pub(super) const MAX_UNITS: usize = usize::MAX / 32;

pub(super) struct Inputs {
    pub(super) cells: Vec<CTableMeasureCell>,
    pub(super) bytes: usize,
    _words: Vec<Option<CString>>,
}

fn preflight(rows: &[&Node], columns: u32) -> Result<(usize, usize), String> {
    let mut count = columns as usize;
    let mut bytes = 0_usize;
    for row in rows {
        count = count
            .saturating_add(row.table_cells.len())
            .saturating_add(row.table_rule_data_columns.len());
        bytes = row.table_cells.iter().fold(bytes, |bytes, cell| {
            bytes.saturating_add(cell.decoder_text().map_or(0, str::len))
        });
    }
    if count > MAX_ITEMS || bytes > MAX_BYTES {
        return Err("native table measurement input budget exceeded".into());
    }
    Ok((count, bytes))
}

fn width(value: u64, columns: u32) -> Result<usize, String> {
    let value = usize::try_from(value).map_err(|_| "table width overflow")?;
    if value > MAX_UNITS / columns as usize {
        return Err("native table width budget exceeded".into());
    }
    Ok(value)
}

pub(super) fn prepare(rows: &[&Node], columns: u32) -> Result<Inputs, String> {
    let (count, bytes) = preflight(rows, columns)?;
    let mut words = Vec::with_capacity(count);
    let mut inputs = Vec::with_capacity(count);
    for node in rows {
        if !matches!(
            node.table_row_kind,
            Some(TableRowKind::Data | TableRowKind::LayoutRule { .. })
        ) {
            continue;
        }
        let layout = node.table_layout.as_ref().ok_or("missing table layout")?;
        for cell in &node.table_cells {
            let slot = layout
                .cells
                .get(cell.column as usize)
                .filter(|slot| slot.column == cell.column)
                .ok_or("missing data layout slot")?;
            words.push(
                cell.decoder_text()
                    .map(CString::new)
                    .transpose()
                    .map_err(|_| "NUL in table input")?,
            );
            inputs.push(CTableMeasureCell {
                column: cell.column,
                kind: crate::table_measure::kind_code(slot.kind),
                minimum_width: width(slot.minimum_width_basic_units, columns)?,
                spacing: slot
                    .spacing_columns
                    .map_or(usize::MAX, |width| width as usize),
                width_flags: u32::from(slot.ignore_width)
                    | (u32::from(slot.equal_width) << 1)
                    | (u32::from(slot.maximize_width) << 2),
                text: words
                    .last()
                    .and_then(Option::as_ref)
                    .map_or(std::ptr::null(), |word| word.as_ptr()),
                following_spans: cell.column_span.checked_sub(1).ok_or("zero table span")?,
                text_block: i32::from(cell.text_block),
            });
        }
        for &column in &node.table_rule_data_columns {
            let slot = layout
                .cells
                .get(column as usize)
                .filter(|slot| slot.column == column)
                .ok_or("missing rule layout slot")?;
            if !matches!(
                slot.kind,
                crate::TableLayoutKind::HorizontalRule
                    | crate::TableLayoutKind::DoubleHorizontalRule
            ) {
                return Err("invalid rule-row width input".into());
            }
            inputs.push(CTableMeasureCell {
                column,
                kind: crate::table_measure::kind_code(slot.kind),
                minimum_width: width(slot.minimum_width_basic_units, columns)?,
                spacing: slot
                    .spacing_columns
                    .map_or(usize::MAX, |width| width as usize),
                width_flags: u32::from(slot.ignore_width)
                    | (u32::from(slot.equal_width) << 1)
                    | (u32::from(slot.maximize_width) << 2),
                text: std::ptr::null(),
                following_spans: 0,
                text_block: 0,
            });
        }
    }
    if inputs
        .iter()
        .any(|cell| cell.spacing != usize::MAX && cell.spacing > MAX_UNITS / columns as usize / 24)
    {
        return Err("native table spacing budget exceeded".into());
    }
    check_span_work(&inputs, columns)?;
    Ok(Inputs {
        cells: inputs,
        _words: words,
        bytes,
    })
}

/// `out.c::tblcalc` first looks up span groups in a linked list, then widens
/// unresolved groups. A pass either removes a group or merges a column-width
/// level. Charge that structural work BEFORE calling C; callback byte fuel
/// alone cannot bound these loops. This stores identities, never cell text.
fn check_span_work(inputs: &[CTableMeasureCell], columns: u32) -> Result<(), String> {
    const MAX_SPAN_WORK: usize = 4_000_000;
    let mut groups = BTreeSet::new();
    let mut lookup = 0_usize;
    let mut extents = 0_usize;
    for cell in inputs
        .iter()
        .filter(|cell| cell.following_spans > 0 && cell.width_flags & 1 == 0)
    {
        lookup = lookup.saturating_add(groups.len());
        if groups.insert((cell.column, cell.following_spans)) {
            extents = extents.saturating_add(cell.following_spans as usize + 1);
        }
        let work = (columns as usize + groups.len())
            .saturating_mul(3_usize.saturating_mul(columns as usize + extents))
            .saturating_add(lookup);
        if work > MAX_SPAN_WORK {
            return Err("native table span-work budget exceeded".into());
        }
    }
    Ok(())
}
