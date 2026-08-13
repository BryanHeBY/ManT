//! Fully resolved content shared by engines and in-memory frontends.

use crate::{Document, DocumentAddress, DocumentIndex, TldrDocument};

/// One materialized document query before any versioned process projection.
///
/// This is the in-memory handoff between the document engine and trusted
/// frontends such as `mant-ui`. External consumers receive a versioned
/// `mant-protocol` representation instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContent {
    pub label: String,
    pub address: Option<DocumentAddress>,
    pub document: Option<Document>,
    pub tldr: Option<TldrDocument>,
}

impl ResolvedContent {
    /// Build the immutable node index used by navigation-oriented consumers.
    #[must_use]
    pub fn document_index(&self) -> Option<DocumentIndex> {
        self.document.as_ref().map(DocumentIndex::build)
    }
}
