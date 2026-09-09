//! Bounded, view-independent loading of typed document scopes.
use crate::{DocumentLoader, LoadError, LoadPolicy, LoadSpec};
use mant_ir::{DocumentAddress, DocumentReference, ResolvedContent};
use mant_protocol::{
    DocumentEdge, DocumentEdgeKind, DocumentFrontier, DocumentScope, DocumentSelector,
    MAX_DOCUMENT_SELECTOR_CHARS, MAX_SCOPE_CONTENT_BYTES, MAX_SCOPE_DEPTH,
    MAX_SCOPE_DOCUMENT_LIMIT, MAX_SCOPE_DOCUMENTS, ResolvedDocumentScope, ScopeTextError,
    ScopedDocument, TraversalLimit, UnresolvedDocument, validate_scope_text,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::{error::Error, fmt, io::Write};

mod references;
mod resolve;
#[cfg(test)]
mod tests;

/// A logical scope together with the loaded documents in matching order.
#[derive(Debug, Clone)]
pub struct LoadedDocumentScope {
    /// Transport-neutral logical graph.
    scope: ResolvedDocumentScope,
    /// Loaded documents in the same order as [`ResolvedDocumentScope::documents`].
    documents: Vec<ResolvedContent>,
}

impl LoadedDocumentScope {
    /// Logical graph and source coverage in the loader's stable order.
    #[must_use]
    pub const fn scope(&self) -> &ResolvedDocumentScope {
        &self.scope
    }
    /// Immutable original content paired with the logical graph.
    #[must_use]
    pub fn documents(&self) -> &[ResolvedContent] {
        &self.documents
    }
    /// Transfer ownership together, without cloning documents.
    #[must_use]
    pub fn into_parts(self) -> (ResolvedDocumentScope, Vec<ResolvedContent>) {
        (self.scope, self.documents)
    }
}

/// Invalid loading scope or failure to acquire any initial document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeLoadError {
    /// No initial document was supplied.
    EmptyScope,
    /// The initial document count exceeded the native bound.
    TooManyDocuments,
    /// Traversal depth exceeded the native bound.
    DepthLimit,
    /// The document budget was zero, too large, or smaller than the root set.
    DocumentLimit,
    /// Traversal limits were supplied while link following was disabled.
    TraversalLimitsRequireLinks,
    /// A logical document selector violated its native bound.
    DocumentSelector(ScopeTextError),
    /// No initial document could be loaded.
    NoResolvedDocuments {
        /// Compact seed-resolution diagnostics.
        reasons: Vec<String>,
    },
}
impl fmt::Display for ScopeLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyScope => formatter.write_str("at least one document is required"),
            Self::TooManyDocuments => write!(
                formatter,
                "at most {MAX_SCOPE_DOCUMENTS} initial documents are allowed"
            ),
            Self::DepthLimit => write!(
                formatter,
                "maximum link depth must not exceed {MAX_SCOPE_DEPTH}"
            ),
            Self::DocumentLimit => write!(
                formatter,
                "document limit must include every initial document and not exceed {MAX_SCOPE_DOCUMENT_LIMIT}"
            ),
            Self::TraversalLimitsRequireLinks => {
                formatter.write_str("maxDepth and maxDocuments require followLinks=true")
            }
            Self::DocumentSelector(error) => write!(
                formatter,
                "document selector {}",
                scope_text_error_message(*error)
            ),
            Self::NoResolvedDocuments { reasons } => {
                formatter.write_str("none of the initial documents could be resolved")?;
                if !reasons.is_empty() {
                    write!(formatter, ": {}", reasons.join("; "))?;
                }
                Ok(())
            }
        }
    }
}
impl Error for ScopeLoadError {}

/// Validate source selection and traversal bounds without inspecting a query view.
///
/// # Errors
///
/// Returns the first violated selector or loading-scope bound.
pub fn validate_document_scope(scope: &DocumentScope) -> Result<(), ScopeLoadError> {
    if scope.documents.is_empty() {
        return Err(ScopeLoadError::EmptyScope);
    }
    if scope.documents.len() > MAX_SCOPE_DOCUMENTS {
        return Err(ScopeLoadError::TooManyDocuments);
    }
    for selector in &scope.documents {
        validate_scope_text(&selector.selector, MAX_DOCUMENT_SELECTOR_CHARS)
            .map_err(ScopeLoadError::DocumentSelector)?;
    }
    if !scope.traversal.follow_links
        && (scope.traversal.max_depth.is_some() || scope.traversal.max_documents.is_some())
    {
        return Err(ScopeLoadError::TraversalLimitsRequireLinks);
    }
    if scope.traversal.effective_max_depth() > MAX_SCOPE_DEPTH {
        return Err(ScopeLoadError::DepthLimit);
    }
    let root_count = u32::try_from(scope.documents.len()).unwrap_or(u32::MAX);
    if scope.traversal.effective_max_documents() < root_count
        || scope.traversal.effective_max_documents() > MAX_SCOPE_DOCUMENT_LIMIT
    {
        return Err(ScopeLoadError::DocumentLimit);
    }
    Ok(())
}

fn scope_text_error_message(error: ScopeTextError) -> String {
    match error {
        ScopeTextError::Empty => "must not be empty".to_owned(),
        ScopeTextError::ControlCharacter => "must not contain control characters".to_owned(),
        ScopeTextError::TooLong { maximum } => {
            format!("must not exceed {maximum} Unicode scalar values")
        }
    }
}
