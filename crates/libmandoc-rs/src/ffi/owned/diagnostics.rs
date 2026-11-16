//! Transfer typed native findings while the session still owns their display bytes.

use super::budget::TransferBudget;
use crate::ffi::{
    raw::{self, CDiagnosticView},
    session::DocumentHandle,
};
use crate::{Diagnostic, DiagnosticCode, DiagnosticLevel, SourceLocation};
use std::{mem::MaybeUninit, slice};

// Mirror diagnostics.inc limits at the ownership boundary. Count raw bytes
// independently of the public UTF-8 message's potentially larger allocation.
const MAX_FIELD_BYTES: usize = 4096;
// Two additional records are reserved for the known input-loss finding and
// the detail-omission warning even when both ordinary detail caps are full.
const MAX_COUNT: usize = 4096 + 2;
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024 + 2 * MAX_FIELD_BYTES;

pub(in crate::ffi) fn copy_diagnostics(
    handle: &DocumentHandle,
    budget: &mut TransferBudget,
) -> Result<Vec<Diagnostic>, String> {
    let document = handle.0.as_ptr();
    let count = unsafe { raw::mant_mandoc_document_diagnostic_count(document) };
    if count > MAX_COUNT {
        return Err("libmandoc returned too many diagnostic records".to_owned());
    }
    budget.reserve_items(count)?;
    budget.reserve_bytes(count * size_of::<Diagnostic>())?;
    let mut findings = Vec::with_capacity(count);
    let mut raw_message_bytes = 0;
    for index in 0..count {
        let mut view = MaybeUninit::uninit();
        if unsafe { raw::mant_mandoc_document_diagnostic(document, index, view.as_mut_ptr()) } != 1
        {
            return Err("libmandoc could not read a diagnostic record".to_owned());
        }
        // A successful view is initialized and borrows storage from handle,
        // which remains live throughout the transfer.
        let view = unsafe { view.assume_init() };
        findings.push(copy_finding(&view, budget, &mut raw_message_bytes)?);
    }
    Ok(findings)
}

fn check_field(pointer: *const u8, length: usize) -> Result<(), String> {
    if length > MAX_FIELD_BYTES || pointer.is_null() != (length == 0) {
        return Err("libmandoc returned invalid diagnostic text bounds".to_owned());
    }
    Ok(())
}

fn copy_finding(
    view: &CDiagnosticView,
    budget: &mut TransferBudget,
    raw_message_bytes: &mut usize,
) -> Result<Diagnostic, String> {
    let code = match view.kind {
        0 => None,
        1 => Some(DiagnosticCode::InputProcessingLimit),
        _ => return Err("libmandoc returned an invalid diagnostic code".to_owned()),
    };
    let level = match view.level {
        1 => DiagnosticLevel::Style,
        2 => DiagnosticLevel::Warning,
        3 => DiagnosticLevel::Error,
        4 => DiagnosticLevel::Unsupported,
        _ => return Err("libmandoc returned an invalid diagnostic level".to_owned()),
    };
    let location = match (view.line, view.column) {
        (0, 0) => None,
        (0, _) | (_, 0) => {
            return Err("libmandoc returned an invalid diagnostic location".to_owned());
        }
        (line, column) => Some(SourceLocation { line, column }),
    };
    // A source label may contain arbitrary OS bytes. The public finding
    // exposes only its typed line and column, so it needs no path copy.
    check_field(view.source, view.source_length)?;
    check_field(view.message, view.message_length)?;
    if view.message_length > MAX_MESSAGE_BYTES - *raw_message_bytes {
        return Err("libmandoc returned too many diagnostic message bytes".to_owned());
    }
    *raw_message_bytes += view.message_length;
    let message = if view.message_length == 0 {
        String::new()
    } else {
        // The live diagnostic record owns this bounded, non-null byte span.
        let bytes = unsafe { slice::from_raw_parts(view.message, view.message_length) };
        copy_message(bytes, budget)?
    };
    Ok(Diagnostic {
        code,
        level,
        message,
        location,
    })
}

fn copy_message(bytes: &[u8], budget: &mut TransferBudget) -> Result<String, String> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        let text = text.trim_end();
        budget.reserve_bytes(text.len())?;
        return Ok(text.to_owned());
    }
    // Reserve the expanded lossy UTF-8 size before allocating it. Only
    // diagnostic display reaches here; internal AST text remains strict.
    let display_length: usize = bytes
        .utf8_chunks()
        .map(|chunk| chunk.valid().len() + 3 * usize::from(!chunk.invalid().is_empty()))
        .sum();
    budget.reserve_bytes(display_length)?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{super::budget::with_remaining_budget, *};
    use std::ptr;

    #[test]
    fn malformed_native_diagnostic_views_fail_before_reading_their_payloads() {
        let mut view = CDiagnosticView {
            source: ptr::null(),
            source_length: 0,
            message: ptr::dangling(),
            message_length: MAX_FIELD_BYTES + 1,
            kind: 0,
            level: 3,
            line: 6,
            column: 2,
        };
        let mut budget = TransferBudget::default();
        let mut raw = 0;
        assert!(copy_finding(&view, &mut budget, &mut raw).is_err());
        view.message_length = 1;
        view.source_length = 1;
        assert!(copy_finding(&view, &mut budget, &mut raw).is_err());
        view.source_length = 0;
        raw = MAX_MESSAGE_BYTES;
        assert!(copy_finding(&view, &mut budget, &mut raw).is_err());
        view.line = 0;
        assert!(copy_finding(&view, &mut budget, &mut raw).is_err());
        view.column = 0;
        view.kind = u32::MAX;
        assert!(copy_finding(&view, &mut budget, &mut raw).is_err());
    }

    #[test]
    fn diagnostic_display_alone_accepts_invalid_bytes_and_charges_the_expanded_size() {
        let display = "a\u{fffd}z";
        let invalid = b"a\xffz";
        with_remaining_budget(0, display.len(), || {
            let mut budget = TransferBudget::default();
            assert_eq!(copy_message(invalid, &mut budget).unwrap(), display);
        });
        with_remaining_budget(0, display.len() - 1, || {
            let mut budget = TransferBudget::default();
            assert!(copy_message(invalid, &mut budget).is_err());
        });
    }
}
