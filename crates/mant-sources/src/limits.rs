//! Shared resource budgets for acquired document-source snapshots.

pub(crate) const MAX_SOURCE_ENTRIES: usize = 20_000;
pub(crate) const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_SOURCE_DOCUMENTS: usize = 10_000;
pub(crate) const MAX_SOURCE_DEPTH: usize = 32;
