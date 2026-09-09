//! Checked, snapshot-local addresses into authoritative document content.
//! Typed roots own wire/encoding bounds, resolvers check exact containers, and
//! entry mapping translates original slices without copying document content.

mod entry;
mod resolve;
mod types;

pub use entry::EntryOwnerLocationRef;
pub(crate) use resolve::{block_children, content_blocks, inline_children};
pub use resolve::{
    resolve_block_descendant, resolve_content_block, resolve_content_section, resolve_inline_path,
};
pub use types::{
    ContentBlockStep, ContentInlineRoot, ContentLocation, ContentLocationRef, MAX_CONTENT_DEPTH,
    MAX_CONTENT_LOCATION_BYTES,
};

#[cfg(test)]
mod tests;
