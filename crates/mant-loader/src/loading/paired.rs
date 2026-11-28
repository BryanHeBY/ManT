//! Immutable source snapshot and its optional native facts.
pub use mant_codec::SourceBoundLinkLayout as ResolvedWithLinkLayout;

/// Keep the decoded document in its own carrier until final metadata is known.
pub(super) fn finish(
    loaded: ResolvedContent,
    native: Option<mant_codec::DecodedLinkDocument>,
) -> ResolvedWithLinkLayout {
    if let Some(native) = native {
        debug_assert!(loaded.document.is_none());
        native.into_resolved(loaded.label, loaded.address, loaded.tldr)
    } else {
        ResolvedWithLinkLayout::without_native_links(loaded)
    }
}

use mant_ir::ResolvedContent;
