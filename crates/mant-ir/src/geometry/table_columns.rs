//! Bounded responsive column allocation, independent of table placement/style.
use super::MAX_COLUMN_PADDING;
use crate::MAX_DENSE_TABLE_COLUMNS;

/// Allocate preferred table content widths inside an available content area.
///
/// Borders are excluded from `available`; `gap` is the total space between
/// adjacent content columns, including any consumer-owned divider rail.
/// Columns can shrink to eight cells, or their shorter preferred width. The
/// result does not wrap text, execute source layout or select whole-table
/// alignment. `None` means dense-column, gap or minimum-width limits failed.
/// Storage is bounded to 256 columns and allocation work does not grow with
/// viewport width, even for deliberately huge public API inputs.
#[must_use]
pub fn allocate_table_columns(
    preferred_widths: &[usize],
    available: usize,
    gap: usize,
) -> Option<Vec<usize>> {
    const SOFT_MINIMUM: usize = 8;
    if preferred_widths.len() > MAX_DENSE_TABLE_COLUMNS {
        return None;
    }
    let gaps = gap.checked_mul(preferred_widths.len().saturating_sub(1))?;
    if gaps > MAX_COLUMN_PADDING {
        return None;
    }
    let usable = available.checked_sub(gaps)?;
    let preferred = preferred_widths
        .iter()
        .map(|width| (*width).max(1))
        .collect::<Vec<_>>();
    if preferred.is_empty() {
        return Some(preferred);
    }
    let sum = |cap: usize| {
        preferred
            .iter()
            .try_fold(0_usize, |sum, width| sum.checked_add((*width).min(cap)))
    };
    if sum(usize::MAX).is_some_and(|sum| sum <= usable) {
        return Some(preferred);
    }
    if sum(SOFT_MINIMUM).is_none_or(|minimum| minimum > usable) {
        return None;
    }
    // Find the last complete round of equal growth. This is equivalent to
    // incrementing each still-short column in source order, without rescanning
    // columns once per added viewport cell.
    let mut low = SOFT_MINIMUM;
    let mut high = preferred.iter().copied().max().unwrap_or(SOFT_MINIMUM);
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if sum(middle).is_some_and(|sum| sum <= usable) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let mut widths = preferred
        .iter()
        .map(|width| (*width).min(low))
        .collect::<Vec<_>>();
    let mut remaining = usable - sum(low)?;
    for (width, preferred) in widths.iter_mut().zip(preferred) {
        if remaining == 0 {
            break;
        }
        if *width < preferred {
            *width += 1;
            remaining -= 1;
        }
    }
    Some(widths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_columns_shrink_but_short_columns_keep_their_natural_width() {
        assert_eq!(
            allocate_table_columns(&[3, 34, 34], 43, 2),
            Some(vec![3, 18, 18])
        );
        assert_eq!(
            allocate_table_columns(&[3, 34, 34], 44, 2),
            Some(vec![3, 19, 18])
        );
        assert_eq!(
            allocate_table_columns(&[3, 34, 34], 72, 2),
            Some(vec![3, 33, 32])
        );
        assert_eq!(
            allocate_table_columns(&[3, 34, 34], 75, 2),
            Some(vec![3, 34, 34])
        );
        assert_eq!(allocate_table_columns(&[3, 34, 34], 22, 2), None);
        assert_eq!(
            allocate_table_columns(&[3, 34, 34], 23, 2),
            Some(vec![3, 8, 8])
        );
    }

    #[test]
    fn empty_and_very_large_inputs_do_not_amplify_width_or_column_work() {
        assert_eq!(allocate_table_columns(&[], 0, usize::MAX), Some(vec![]));
        assert_eq!(allocate_table_columns(&[0, 0], 3, 1), Some(vec![1, 1]));
        assert_eq!(
            allocate_table_columns(&[1; 256], 256, 0),
            Some(vec![1; 256])
        );
        assert_eq!(allocate_table_columns(&[1; 257], usize::MAX, 0), None);
        assert_eq!(
            allocate_table_columns(&[1, 1], usize::MAX, usize::MAX),
            None
        );
        assert_eq!(
            allocate_table_columns(&[usize::MAX, usize::MAX], 25, 1),
            Some(vec![12, 12])
        );
        let widths = allocate_table_columns(&[usize::MAX, usize::MAX], usize::MAX, 0).unwrap();
        assert_eq!(widths, vec![usize::MAX / 2 + 1, usize::MAX / 2]);
    }
}
