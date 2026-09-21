//! Checked ownership transfer for the versioned structured-rendering ABI.
#![allow(dead_code)]

mod conversion;
mod input;
mod raw;
mod session;
mod transfer;
mod validation;

use conversion::{raw_limits, semantic_document, semantic_error};
use input::InputStorage;
use raw::{
    ATOM_BREAK_OPPORTUNITY, ATOM_HARD_BREAK, ATOM_TEXT, ATOM_WHITESPACE, AnchorView,
    BLOCK_DEFINITION_LIST, BLOCK_FIXED_DISPLAY, BLOCK_HEADING, BLOCK_INDENTED, BLOCK_LIST,
    BLOCK_PARAGRAPH, BLOCK_TABLE, BLOCK_THEMATIC_BREAK, BLOCK_VERTICAL_SPACE, BlockView, BytesView,
    COORD_NATIVE_NORMALIZED_BYTES, ContentAtomView, ContentPointView, ContentRefView,
    ContentRootView, DIAGNOSTIC_CODE_NATIVE_LAST, DIAGNOSTIC_STYLE, DIAGNOSTIC_UNSUPPORTED,
    DecorationView, DiagnosticView, FORMAT_MAN, FORMAT_MDOC, FailureView, FixedLineView, FixedView,
    FormView, HeadingEvidenceView, IDENTITY_BUNDLE_MEMBER, InputSourceView, InputView, ItemView,
    LINK_LABEL_CONTENT, LINK_LABEL_HARD_BREAK, LIST_BULLET, LIST_DEFINITION, LIST_NATIVE_MARKER,
    LIST_ORDERED, LIST_PLAIN, Limits, LinkLabelPartView, LinkView, ListView, MetadataView,
    NameHintView, OWNER_DEFINITION_ITEM, OWNER_KIND_LAST, OWNER_LIST_ITEM, OwnerView,
    PROFILE_ASCII, PROFILE_UTF8, PROVENANCE_AUTHORED, PROVENANCE_GENERATED, PROVENANCE_UNKNOWN,
    PlacementView, ProvenanceView, ROOT_BODY, ROOT_HEADING, ROOT_KIND_LAST, ROOT_TERM,
    RelationView, ResultHandleRaw, ResultView, STATUS_BUDGET, STATUS_BUILDER_ALLOC,
    STATUS_INVALID_INPUT, STATUS_NATIVE, STATUS_OK, STATUS_REENTRANT, STATUS_RELATION,
    STATUS_UNSUPPORTED, STYLE_MASK, SliceView, SourceView, SpanView, TARGET_ORIGIN_AUTHORED,
    TARGET_ORIGIN_GENERATED, TableCellView, TableRowView, TableView, mant_structured_abi_version,
    mant_structured_render, mant_structured_result_check, mant_structured_result_free,
    mant_structured_result_view,
};
use transfer::{
    OwnedAnchor, OwnedBlock, OwnedContentAtom, OwnedContentPoint, OwnedContentRef,
    OwnedContentRoot, OwnedDiagnostic, OwnedForm, OwnedHeadingEvidence, OwnedItem, OwnedLink,
    OwnedLinkLabelPart, OwnedList, OwnedMetadata, OwnedNameHint, OwnedOwner, OwnedProvenance,
    OwnedSource, OwnedSpan, OwnedStructuredDocument, ResultHandle, StructuredSlices,
    copy_structured_document,
};
use validation::{
    checked_slice, transfer_preflight, validate_metadata, validate_structured_relations,
};

#[cfg(test)]
use raw::{
    ProbeMetrics, RESOLVE_DENIED, RESOLVE_INVALID, RESOLVE_NOT_FOUND, RESOLVE_PANIC,
    mant_structured_discriminant_fingerprint, mant_structured_probe,
    mant_structured_test_fail_after, mant_structured_view_align, mant_structured_view_offset,
    mant_structured_view_size,
};

pub(crate) use session::render_structured;
#[cfg(test)]
use session::{probe_structured, probe_structured_profile, render_prelude, render_prelude_profile};
#[cfg(test)]
use transfer::copy_string;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeStructuredError {
    pub(crate) status: u32,
    pub(crate) stage: u32,
    pub(crate) limit_kind: u32,
    pub(crate) observed: u64,
    pub(crate) allowed: u64,
}

fn relation_error() -> NativeStructuredError {
    NativeStructuredError {
        status: STATUS_RELATION,
        stage: 6,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    }
}

fn alloc_error(_: std::collections::TryReserveError) -> NativeStructuredError {
    NativeStructuredError {
        status: STATUS_BUILDER_ALLOC,
        stage: 6,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    }
}

#[cfg(test)]
mod tests;
