//! Checked string and column-array copying at the native lifetime boundary.

use super::budget::{MAX_OWNED_SYNTAX_BYTES, MAX_OWNED_SYNTAX_ITEMS, TransferBudget};
use std::{ffi::CStr, os::raw::c_char};

pub(super) unsafe fn checked_string(pointer: *const c_char) -> Result<Option<String>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    let text =
        std::str::from_utf8(bytes).map_err(|_| "libmandoc returned a non-UTF-8 internal string")?;
    Ok(Some(text.to_owned()))
}

/// Recognize static names while the parser-owned C string is still borrowed.
/// Only an unknown spelling crosses this boundary as a heap allocation.
pub(super) unsafe fn checked_macro(
    pointer: *const c_char,
) -> Result<Option<crate::MacroToken>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    let name =
        std::str::from_utf8(bytes).map_err(|_| "libmandoc returned a non-UTF-8 internal string")?;
    Ok(Some(crate::MacroToken::from_name(name)))
}

#[cfg(test)]
mod string_boundary_tests {
    use super::{checked_macro, checked_string};

    #[test]
    fn macro_identity_transfer_checks_utf8_and_owns_only_unknown_names() {
        use crate::{MacroToken, MdocMacro};
        use std::ffi::CString;

        let known = CString::new("Fo").unwrap();
        assert_eq!(
            unsafe { checked_macro(known.as_ptr()) }.unwrap(),
            Some(MacroToken::Mdoc(MdocMacro::Fo))
        );
        let unknown = CString::new("FutureBlock").unwrap();
        let transferred = unsafe { checked_macro(unknown.as_ptr()) }.unwrap().unwrap();
        drop(unknown);
        assert_eq!(transferred, MacroToken::Unknown("FutureBlock".to_owned()));
        assert_eq!(unsafe { checked_macro(std::ptr::null()) }.unwrap(), None);
        let invalid = [0xff_u8, 0];
        assert_eq!(
            unsafe { checked_macro(invalid.as_ptr().cast()) }.unwrap_err(),
            "libmandoc returned a non-UTF-8 internal string"
        );
    }

    #[test]
    fn successful_internal_strings_reject_invalid_utf8() {
        let invalid = [0xff_u8, 0];
        let result = unsafe { checked_string(invalid.as_ptr().cast()) };
        assert_eq!(
            result.unwrap_err(),
            "libmandoc returned a non-UTF-8 internal string"
        );
    }
}

include!(concat!(env!("OUT_DIR"), "/text_sentinels.rs"));

pub(super) unsafe fn visible_string(pointer: *const c_char) -> Result<Option<String>, String> {
    Ok(split_visible_text(unsafe { checked_string(pointer) }?).0)
}

fn has_native_text_sentinel(text: &str) -> bool {
    // mandoc.h defines all five sentinels as single ASCII bytes. UTF-8
    // continuation bytes cannot be mistaken for one of these controls.
    text.bytes().any(|byte| {
        [
            ASCII_NBRSP as u8,
            ASCII_NBRZW as u8,
            ASCII_BREAK as u8,
            ASCII_HYPH as u8,
            ASCII_TABREF as u8,
        ]
        .contains(&byte)
    })
}

/// The checked native copy already owns its bytes. Only sentinel-bearing
/// text needs a second spelling; ordinary text keeps that same allocation.
pub(super) fn split_visible_text(text: Option<String>) -> (Option<String>, Option<String>) {
    match text {
        Some(text) if has_native_text_sentinel(&text) => {
            let visible = normalize_visible_text(&text);
            (Some(visible), Some(text))
        }
        text => (text, None),
    }
}

fn normalize_visible_text(text: &str) -> String {
    text.chars()
        .filter_map(|character| match character {
            ASCII_NBRZW | ASCII_BREAK | ASCII_TABREF => None,
            ASCII_HYPH => Some('-'),
            ASCII_NBRSP => Some(' '),
            other => Some(other),
        })
        .collect()
}

/// Copy the declared `Bl -column` width strings, if this node owns any.
pub(super) unsafe fn copy_column_strings(
    pointer: *const *const c_char,
    count: usize,
    transfer_budget: &mut TransferBudget,
) -> Result<Vec<String>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if pointer.is_null() {
        return Err("libmandoc returned null columns with a nonzero count".to_owned());
    }
    if !pointer.is_aligned() {
        return Err("libmandoc returned misaligned column pointers".to_owned());
    }
    let pointer_bytes = count
        .checked_mul(std::mem::size_of::<*const c_char>())
        .filter(|bytes| *bytes <= usize::try_from(isize::MAX).unwrap_or(usize::MAX))
        .ok_or_else(|| "libmandoc column pointer range overflowed".to_owned())?;
    let owned_bytes = count
        .checked_mul(std::mem::size_of::<String>())
        .ok_or_else(|| "libmandoc column allocation overflowed".to_owned())?;
    if count > MAX_OWNED_SYNTAX_ITEMS.saturating_sub(transfer_budget.items)
        || owned_bytes > MAX_OWNED_SYNTAX_BYTES.saturating_sub(transfer_budget.bytes)
        || pointer_bytes > MAX_OWNED_SYNTAX_BYTES
    {
        return Err("owned column transfer exceeded its cumulative node/byte budget".to_owned());
    }
    // Reserve the complete pointer/count transfer before allocating or walking
    // borrowed storage. Parser-owned entries remain valid for this call only.
    transfer_budget.items += count;
    transfer_budget.bytes += owned_bytes;
    let mut columns = Vec::with_capacity(count);
    for index in 0..count {
        let value = unsafe { *pointer.add(index) };
        if value.is_null() {
            return Err("libmandoc returned a null column string".to_owned());
        }
        let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
        if bytes.len() > MAX_OWNED_SYNTAX_BYTES.saturating_sub(transfer_budget.bytes) {
            return Err("owned column strings exceeded the cumulative byte budget".to_owned());
        }
        transfer_budget.bytes += bytes.len();
        columns.push(
            std::str::from_utf8(bytes)
                .map_err(|_| "libmandoc returned a non-UTF-8 column string")?
                .to_owned(),
        );
    }
    Ok(columns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn visible_text_reuses_checked_storage_and_retains_native_sentinels() {
        for value in ["", "ordinary text", "café 日本 😀\t\\c"] {
            let text = value.to_owned();
            let allocation = text.as_ptr();
            let (visible, native) = split_visible_text(Some(text));
            assert!(native.is_none());
            let visible = visible.unwrap();
            assert_eq!(visible, value);
            assert_eq!(visible.as_ptr(), allocation);
        }
        assert_eq!(split_visible_text(None), (None, None));
        // mandoc.h's ASCII_* controls retain independent native evidence,
        // while the visible spelling follows the existing projection.
        let text =
            format!("中{ASCII_NBRSP}A{ASCII_NBRZW}{ASCII_BREAK}{ASCII_HYPH}{ASCII_TABREF}😀");
        let allocation = text.as_ptr();
        let expected_native = text.clone();
        let (visible, native) = split_visible_text(Some(text));
        assert_eq!(visible.as_deref(), Some("中 A-😀"));
        let native = native.unwrap();
        assert_eq!(native, expected_native);
        assert_eq!(native.as_ptr(), allocation);
    }

    #[test]
    fn column_pointer_count_is_validated_before_allocation_or_dereference() {
        let mut budget = TransferBudget::default();
        assert_eq!(
            unsafe { copy_column_strings(std::ptr::null(), 0, &mut budget) }
                .unwrap()
                .len(),
            0
        );
        assert!(unsafe { copy_column_strings(std::ptr::null(), 1, &mut budget) }.is_err());
        let invalid = std::ptr::NonNull::<*const c_char>::dangling().as_ptr();
        let misaligned = invalid.with_addr(invalid.addr().wrapping_add(1));
        assert!(unsafe { copy_column_strings(misaligned, 1, &mut budget) }.is_err());
        for count in [usize::MAX, MAX_OWNED_SYNTAX_ITEMS + 1] {
            assert!(unsafe { copy_column_strings(invalid, count, &mut budget) }.is_err());
        }
        let strings = [
            CString::new("first").unwrap(),
            CString::new("\\(em").unwrap(),
        ];
        let pointers = strings.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
        assert_eq!(
            unsafe { copy_column_strings(pointers.as_ptr(), 2, &mut budget) }.unwrap(),
            ["first", "\\(em"]
        );
        drop(strings);
        let null_entry = [std::ptr::null()];
        assert!(unsafe { copy_column_strings(null_entry.as_ptr(), 1, &mut budget) }.is_err());
        budget.bytes = MAX_OWNED_SYNTAX_BYTES;
        assert!(unsafe { copy_column_strings(invalid, 1, &mut budget) }.is_err());
    }

    #[test]
    fn native_marker_values_and_visible_translation_follow_the_pinned_header() {
        assert_eq!(ASCII_TABREF, '\u{1a}');
        assert_eq!(ASCII_HYPH, '\u{1c}');
        assert_eq!(ASCII_BREAK, '\u{1d}');
        assert_eq!(ASCII_NBRZW, '\u{1e}');
        assert_eq!(ASCII_NBRSP, '\u{1f}');
        for (marker, expected) in [
            (ASCII_TABREF, "AB"),
            (ASCII_HYPH, "A-B"),
            (ASCII_BREAK, "AB"),
            (ASCII_NBRZW, "AB"),
            (ASCII_NBRSP, "A B"),
        ] {
            let input = CString::new(format!("A{marker}B")).unwrap();
            // CString owns the NUL-terminated bytes for this complete call.
            assert_eq!(
                unsafe { visible_string(input.as_ptr()) }
                    .unwrap()
                    .as_deref(),
                Some(expected)
            );
        }
        let input = CString::new("café 日本 😀\t").unwrap();
        assert_eq!(
            unsafe { visible_string(input.as_ptr()) }
                .unwrap()
                .as_deref(),
            Some("café 日本 😀\t")
        );
        assert_eq!(unsafe { visible_string(std::ptr::null()) }.unwrap(), None);
    }
}
