//! Ordinary list labels and the inherited boundary before an item.

use std::borrow::Cow;

use crate::ListKind;

/// Visible label, including its separating blank. Fixed labels borrow static
/// text; ordered labels use the list's saturating source ordinal.
#[must_use]
pub fn list_marker(kind: ListKind, index: usize) -> Cow<'static, str> {
    match kind {
        ListKind::Plain => Cow::Borrowed(""),
        ListKind::Bullet => Cow::Borrowed("• "),
        ListKind::Dash => Cow::Borrowed("- "),
        ListKind::Ordered { .. } => kind.ordinal(index).map_or(Cow::Borrowed(""), |ordinal| {
            Cow::Owned(format!("{ordinal}. "))
        }),
    }
}

/// Display cells in [`list_marker`], without allocating its text. Ordered
/// labels contain only decimal digits, a full stop and a separating blank.
#[must_use]
pub fn list_marker_width(kind: ListKind, index: usize) -> usize {
    let Some(mut ordinal) = kind.ordinal(index) else {
        return if matches!(kind, ListKind::Plain) {
            0
        } else {
            2
        };
    };
    let mut width = 3;
    while ordinal >= 10 {
        ordinal /= 10;
        width += 1;
    }
    width
}

/// Resolve list or definition item spacing. Explicit zero stays tight;
/// absent spacing adds one blank row only between noncompact items.
#[must_use]
pub const fn list_item_spacing(explicit: Option<u16>, index: usize, compact: bool) -> u16 {
    match explicit {
        Some(lines) => lines,
        None if index > 0 && !compact => 1,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_labels_borrow_and_ordered_labels_preserve_full_ordinals() {
        for (kind, label, width) in [
            (ListKind::Plain, "", 0),
            (ListKind::Bullet, "• ", 2),
            (ListKind::Dash, "- ", 2),
        ] {
            let marker = list_marker(kind, usize::MAX);
            assert!(matches!(marker, Cow::Borrowed(_)));
            assert_eq!(marker, label);
            assert_eq!(list_marker_width(kind, usize::MAX), width);
        }
        for (start, index, label, width) in [
            (None, 0, "1. ", 3),
            (Some(0), 0, "0. ", 3),
            (Some(9), 0, "9. ", 3),
            (Some(9), 1, "10. ", 4),
            (Some(99), 1, "100. ", 5),
            (Some(u64::MAX), 1, "18446744073709551615. ", 22),
        ] {
            let kind = ListKind::Ordered { start };
            let marker = list_marker(kind, index);
            assert!(matches!(marker, Cow::Owned(_)));
            assert_eq!(marker, label);
            assert_eq!(list_marker_width(kind, index), width);
            assert_eq!(super::super::text_width(&marker), width);
        }
    }

    #[test]
    fn explicit_spacing_overrides_compactness_and_first_item_defaults() {
        for compact in [false, true] {
            assert_eq!(list_item_spacing(None, 0, compact), 0);
            for index in [1, usize::MAX] {
                assert_eq!(list_item_spacing(None, index, compact), u16::from(!compact));
            }
            for index in [0, 1, usize::MAX] {
                for lines in [0, 2, u16::MAX] {
                    assert_eq!(list_item_spacing(Some(lines), index, compact), lines);
                }
            }
        }
    }
}
