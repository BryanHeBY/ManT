//! Checked string and column-array copying at the native lifetime boundary.

use super::budget::{MAX_OWNED_SYNTAX_BYTES, TransferBudget};
use std::{ffi::CStr, os::raw::c_char};

/// Validate native storage while it is still borrowed from the live session.
///
/// # Safety
/// A non-null pointer must name an immutable NUL-terminated native string that
/// remains valid for the chosen borrow lifetime. Keep the document handle live
/// until every returned borrow has been consumed.
pub(super) unsafe fn borrowed_string<'a>(
    pointer: *const c_char,
) -> Result<Option<&'a str>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    std::str::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "libmandoc returned a non-UTF-8 internal string".to_owned())
}

pub(super) unsafe fn checked_string(
    pointer: *const c_char,
    budget: &mut TransferBudget,
) -> Result<Option<String>, String> {
    let text = unsafe { borrowed_string(pointer) }?;
    budget.reserve_bytes(text.map_or(0, str::len))?;
    Ok(text.map(str::to_owned))
}

/// Keep the existing logical spelling charge for known static identities, and
/// reserve unknown spellings before `from_name` allocates their owned String.
pub(super) unsafe fn checked_macro(
    pointer: *const c_char,
    budget: &mut TransferBudget,
) -> Result<Option<crate::MacroToken>, String> {
    let name = unsafe { borrowed_string(pointer) }?;
    budget.reserve_bytes(name.map_or(0, str::len))?;
    Ok(name.map(crate::MacroToken::from_name))
}

include!(concat!(env!("OUT_DIR"), "/text_sentinels.rs"));

/// Reserve both stored spellings together, before either is copied. The native
/// spelling is retained only when the visible projection changes its bytes.
pub(super) unsafe fn copy_text(
    pointer: *const c_char,
    budget: &mut TransferBudget,
) -> Result<(Option<String>, Option<String>), String> {
    let Some(text) = (unsafe { borrowed_string(pointer) })? else {
        return Ok((None, None));
    };
    let (has_sentinels, visible_bytes) = visible_storage(text);
    let required = if has_sentinels {
        text.len().saturating_add(visible_bytes)
    } else {
        text.len()
    };
    budget.reserve_bytes(required)?;
    if has_sentinels {
        Ok((
            Some(normalize_visible_text(text, visible_bytes)),
            Some(text.to_owned()),
        ))
    } else {
        Ok((Some(text.to_owned()), None))
    }
}

pub(super) unsafe fn visible_string(
    pointer: *const c_char,
    budget: &mut TransferBudget,
) -> Result<Option<String>, String> {
    let Some(text) = (unsafe { borrowed_string(pointer) })? else {
        return Ok(None);
    };
    let (has_sentinels, visible_bytes) = visible_storage(text);
    budget.reserve_bytes(visible_bytes)?;
    Ok(Some(if has_sentinels {
        normalize_visible_text(text, visible_bytes)
    } else {
        text.to_owned()
    }))
}

fn visible_storage(text: &str) -> (bool, usize) {
    // mandoc.h defines all five sentinels as single ASCII bytes. UTF-8
    // continuation bytes cannot be mistaken for one of these controls.
    text.bytes().fold((false, 0), |(sentinels, bytes), byte| {
        if [ASCII_NBRZW as u8, ASCII_BREAK as u8, ASCII_TABREF as u8].contains(&byte) {
            (true, bytes)
        } else {
            (
                sentinels || [ASCII_HYPH as u8, ASCII_NBRSP as u8].contains(&byte),
                bytes + 1,
            )
        }
    })
}

fn normalize_visible_text(text: &str, bytes: usize) -> String {
    let mut visible = String::with_capacity(bytes);
    visible.extend(text.chars().filter_map(|character| match character {
        ASCII_NBRZW | ASCII_BREAK | ASCII_TABREF => None,
        ASCII_HYPH => Some('-'),
        ASCII_NBRSP => Some(' '),
        other => Some(other),
    }));
    visible
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
    if pointer_bytes > MAX_OWNED_SYNTAX_BYTES {
        return Err("owned column transfer exceeded its cumulative node/byte budget".to_owned());
    }
    // Reserve the complete pointer/count transfer before allocating or walking
    // borrowed storage. Parser-owned entries remain valid for this call only.
    transfer_budget.reserve_items(count)?;
    transfer_budget.reserve_bytes(owned_bytes)?;
    let mut columns = Vec::with_capacity(count);
    for index in 0..count {
        let value = unsafe { *pointer.add(index) };
        if value.is_null() {
            return Err("libmandoc returned a null column string".to_owned());
        }
        let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
        let text = std::str::from_utf8(bytes)
            .map_err(|_| "libmandoc returned a non-UTF-8 column string")?;
        transfer_budget.reserve_bytes(text.len())?;
        columns.push(text.to_owned());
    }
    Ok(columns)
}

#[cfg(test)]
mod tests {
    use super::super::budget::with_remaining_budget;
    use super::*;
    use crate::{MacroToken, MdocMacro};
    use std::ffi::CString;

    #[test]
    fn strings_reserve_bytes_without_consuming_syntax_items() {
        for value in ["", "ordinary text", "café 日本 😀\t\\c"] {
            let input = CString::new(value).unwrap();
            for remaining in [value.len(), value.len().saturating_sub(1)] {
                with_remaining_budget(0, remaining, || {
                    let mut budget = TransferBudget::default();
                    let initial_items = budget.items;
                    let initial_bytes = budget.bytes;
                    let result = unsafe { checked_string(input.as_ptr(), &mut budget) };
                    assert_eq!(budget.items, initial_items);
                    if remaining == value.len() {
                        assert_eq!(result.unwrap().as_deref(), Some(value));
                        assert_eq!(budget.bytes - initial_bytes, value.len());
                    } else {
                        assert!(result.unwrap_err().contains("byte budget"));
                        assert_eq!(budget.bytes, initial_bytes);
                    }
                });
            }
        }
    }

    #[test]
    fn strings_validate_utf8_before_reserving_and_keep_null_distinct_from_empty() {
        let invalid = [0xff_u8, 0];
        with_remaining_budget(0, 0, || {
            let mut budget = TransferBudget::default();
            let initial_bytes = budget.bytes;
            assert_eq!(
                unsafe { checked_string(invalid.as_ptr().cast(), &mut budget) }.unwrap_err(),
                "libmandoc returned a non-UTF-8 internal string"
            );
            assert_eq!(
                unsafe { checked_macro(invalid.as_ptr().cast(), &mut budget) }.unwrap_err(),
                "libmandoc returned a non-UTF-8 internal string"
            );
            assert_eq!(budget.bytes, initial_bytes);
            assert_eq!(
                unsafe { checked_string(std::ptr::null(), &mut budget) }.unwrap(),
                None
            );
            assert_eq!(
                unsafe { checked_macro(std::ptr::null(), &mut budget) }.unwrap(),
                None
            );
            assert_eq!(
                unsafe { copy_text(std::ptr::null(), &mut budget) }.unwrap(),
                (None, None)
            );
            assert_eq!(
                unsafe { visible_string(std::ptr::null(), &mut budget) }.unwrap(),
                None
            );
            assert_eq!(
                unsafe { checked_string(c"".as_ptr(), &mut budget) }.unwrap(),
                Some(String::new())
            );
        });
    }

    #[test]
    fn macro_identity_reserves_unknown_storage_and_preserves_known_spelling_charge() {
        for (input, expected) in [
            (c"Fo", MacroToken::Mdoc(MdocMacro::Fo)),
            (
                c"FutureBlock",
                MacroToken::Unknown("FutureBlock".to_owned()),
            ),
        ] {
            let bytes = input.to_bytes().len();
            with_remaining_budget(0, bytes, || {
                let mut budget = TransferBudget::default();
                let initial_bytes = budget.bytes;
                assert_eq!(
                    unsafe { checked_macro(input.as_ptr(), &mut budget) }.unwrap(),
                    Some(expected)
                );
                assert_eq!(budget.bytes - initial_bytes, bytes);
            });
            with_remaining_budget(0, bytes - 1, || {
                assert!(
                    unsafe { checked_macro(input.as_ptr(), &mut TransferBudget::default()) }
                        .is_err()
                );
            });
        }
        let input = CString::new("FutureBlock").unwrap();
        let owned =
            unsafe { checked_macro(input.as_ptr(), &mut TransferBudget::default()) }.unwrap();
        drop(input);
        assert_eq!(owned, Some(MacroToken::Unknown("FutureBlock".to_owned())));
    }

    #[test]
    fn sentinel_text_reserves_both_spellings_but_tags_only_reserve_visible_bytes() {
        // Pinned mandoc.h defines these single-byte controls. The visible
        // projection retains the pre-existing normalization contract.
        assert_eq!(
            [
                ASCII_TABREF,
                ASCII_HYPH,
                ASCII_BREAK,
                ASCII_NBRZW,
                ASCII_NBRSP
            ],
            ['\u{1a}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}']
        );
        for (input, visible, native) in [
            ("", "", None),
            ("ordinary text", "ordinary text", None),
            ("café 日本 😀\t", "café 日本 😀\t", None),
            ("A\u{1a}B", "AB", Some("A\u{1a}B")),
            ("A\u{1c}B", "A-B", Some("A\u{1c}B")),
            ("A\u{1d}B", "AB", Some("A\u{1d}B")),
            ("A\u{1e}B", "AB", Some("A\u{1e}B")),
            ("A\u{1f}B", "A B", Some("A\u{1f}B")),
            (
                "中\u{1f}A\u{1e}\u{1d}\u{1c}\u{1a}😀",
                "中 A-😀",
                Some("中\u{1f}A\u{1e}\u{1d}\u{1c}\u{1a}😀"),
            ),
        ] {
            let input = CString::new(input).unwrap();
            let required = visible.len() + native.map_or(0, str::len);
            with_remaining_budget(0, required, || {
                let mut budget = TransferBudget::default();
                let initial_bytes = budget.bytes;
                let (actual_visible, actual_native) =
                    unsafe { copy_text(input.as_ptr(), &mut budget) }.unwrap();
                assert_eq!(actual_visible.as_deref(), Some(visible));
                assert_eq!(actual_native.as_deref(), native);
                assert_eq!(budget.bytes - initial_bytes, required);
            });
            if required > 0 {
                with_remaining_budget(0, required - 1, || {
                    let mut budget = TransferBudget::default();
                    let initial_bytes = budget.bytes;
                    assert!(unsafe { copy_text(input.as_ptr(), &mut budget) }.is_err());
                    assert_eq!(
                        budget.bytes, initial_bytes,
                        "reserve both spellings before copying either"
                    );
                });
            }
            with_remaining_budget(0, visible.len(), || {
                let mut budget = TransferBudget::default();
                let initial_bytes = budget.bytes;
                assert_eq!(
                    unsafe { visible_string(input.as_ptr(), &mut budget) }
                        .unwrap()
                        .as_deref(),
                    Some(visible)
                );
                assert_eq!(budget.bytes - initial_bytes, visible.len());
            });
        }
    }

    #[test]
    fn columns_reserve_items_and_all_owned_bytes_before_copying() {
        let strings = [
            CString::new("first").unwrap(),
            CString::new("\\(em").unwrap(),
        ];
        let pointers = strings.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();
        let bytes = 2 * std::mem::size_of::<String>() + 9;
        for (items, remaining, accepts) in
            [(2, bytes, true), (1, bytes, false), (2, bytes - 1, false)]
        {
            with_remaining_budget(items, remaining, || {
                let mut budget = TransferBudget::default();
                let initial_items = budget.items;
                let initial_bytes = budget.bytes;
                let result = unsafe { copy_column_strings(pointers.as_ptr(), 2, &mut budget) };
                if accepts {
                    assert_eq!(result.unwrap(), ["first", "\\(em"]);
                    assert_eq!(budget.items - initial_items, 2);
                    assert_eq!(budget.bytes - initial_bytes, bytes);
                } else {
                    assert!(result.is_err());
                }
            });
        }
    }

    #[test]
    fn column_pointer_count_is_validated_before_allocation_or_dereference() {
        let mut budget = TransferBudget::default();
        assert_eq!(
            unsafe { copy_column_strings(std::ptr::null(), 0, &mut budget) }.unwrap(),
            Vec::<String>::new()
        );
        assert!(unsafe { copy_column_strings(std::ptr::null(), 1, &mut budget) }.is_err());
        let invalid = std::ptr::NonNull::<*const c_char>::dangling().as_ptr();
        let misaligned = invalid.with_addr(invalid.addr().wrapping_add(1));
        assert!(unsafe { copy_column_strings(misaligned, 1, &mut budget) }.is_err());
        for count in [usize::MAX, super::super::budget::MAX_OWNED_SYNTAX_ITEMS + 1] {
            assert!(unsafe { copy_column_strings(invalid, count, &mut budget) }.is_err());
        }
        let null_entry = [std::ptr::null()];
        assert!(unsafe { copy_column_strings(null_entry.as_ptr(), 1, &mut budget) }.is_err());
        with_remaining_budget(1, 0, || {
            assert!(
                unsafe { copy_column_strings(invalid, 1, &mut TransferBudget::default()) }.is_err()
            );
        });
        let invalid_utf8 = [0xff_u8, 0];
        let pointers = [invalid_utf8.as_ptr().cast()];
        assert_eq!(
            unsafe { copy_column_strings(pointers.as_ptr(), 1, &mut budget) }.unwrap_err(),
            "libmandoc returned a non-UTF-8 column string"
        );
    }
}
