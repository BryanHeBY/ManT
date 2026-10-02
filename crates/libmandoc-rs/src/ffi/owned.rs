//! Immediate borrowed snapshot to owned Rust AST transfer.
mod budget;
mod equations;
mod nodes;
mod strings;
mod tables;

use super::{
    raw::{self, CDocument},
    session::DocumentHandle,
};
use crate::{Document, Metadata, RawDocument};
use budget::{EquationBudget, TransferBudget};
use nodes::{IdentityState, copy_node, macro_set};
use std::{ffi::CStr, os::raw::c_char, ptr::NonNull};
use strings::checked_string;

pub(super) fn copy_document(pointer: *mut CDocument) -> Result<RawDocument, String> {
    let handle = DocumentHandle(
        NonNull::new(pointer)
            .ok_or_else(|| "libmandoc could not allocate a document".to_owned())?,
    );
    let document = handle.0.as_ptr();
    if unsafe { raw::mant_mandoc_document_ok(document) } == 0 {
        return Err(
            unsafe { optional_string(raw::mant_mandoc_document_error(document)) }
                .unwrap_or_else(|| "libmandoc could not parse the source".to_owned()),
        );
    }

    let root = unsafe { raw::mant_mandoc_document_root(document) };
    if root.is_null() {
        return Err("libmandoc produced no syntax tree".to_owned());
    }

    let mut node_truncated = false;
    let mut equation_budget = EquationBudget::default();
    let mut transfer_budget = TransferBudget::default();
    let mut identities = IdentityState::default();
    let root = unsafe {
        copy_node(
            document,
            root,
            0,
            &mut node_truncated,
            &mut equation_budget,
            &mut transfer_budget,
            &mut identities,
        )
    }?
    .0;
    Ok(RawDocument {
        document: Document {
            macro_set: macro_set(unsafe { raw::mant_mandoc_document_macroset(document) })?,
            metadata: Metadata {
                title: unsafe { checked_string(raw::mant_mandoc_document_title(document)) }?,
                section: unsafe { checked_string(raw::mant_mandoc_document_section(document)) }?,
                volume: unsafe { checked_string(raw::mant_mandoc_document_volume(document)) }?,
                os: unsafe { checked_string(raw::mant_mandoc_document_os(document)) }?,
                arch: unsafe { checked_string(raw::mant_mandoc_document_arch(document)) }?,
                name: unsafe { checked_string(raw::mant_mandoc_document_name(document)) }?,
                date: unsafe { checked_string(raw::mant_mandoc_document_date(document)) }?,
                alias_target: unsafe {
                    checked_string(raw::mant_mandoc_document_alias_target(document))
                }?,
                has_body: unsafe { raw::mant_mandoc_document_has_body(document) } != 0,
            },
            root,
        },
        diagnostics: unsafe { checked_string(raw::mant_mandoc_document_diagnostics(document)) }?
            .unwrap_or_default(),
        node_truncated,
        equation_truncated: equation_budget.truncated,
        escape_truncated: unsafe { raw::mant_mandoc_document_escape_depth_truncated(document) }
            != 0,
    })
}

// Native failure messages and locale probes are outside the successful owned
// AST contract. Preserve a readable status even when their raw bytes are bad.
pub(super) unsafe fn optional_string(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}
