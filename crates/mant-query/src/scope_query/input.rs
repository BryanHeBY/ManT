//! Validate address/order alignment once before borrowing collection query input.
use mant_ir::ResolvedContent;
use mant_protocol::{
    MAX_SCOPE_DEPTH, MAX_SCOPE_DOCUMENT_LIMIT, ResolvedDocumentScope, ScopedDocument,
};
use std::{collections::BTreeSet, error::Error, fmt};

/// An immutable graph and its exact, ordered content snapshots.
///
/// Construction validates structural alignment without parsing or copying content.
/// The caller retains authority over provenance and must supply one coherent
/// snapshot; this is not verification of remote freshness or unvisited targets.
#[derive(Debug, Clone, Copy)]
pub struct QueryScopeView<'a> {
    graph: &'a ResolvedDocumentScope,
    documents: &'a [ResolvedContent],
}

/// Invalid association between a logical loading report and supplied content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeInputError {
    /// Graph records and content have different lengths.
    LengthMismatch,
    /// More documents than the supported collection-query ceiling.
    TooManyDocuments,
    /// The supplied content address is absent or does not match its graph slot.
    AddressMismatch {
        /// Zero-based content slot.
        index: usize,
    },
    /// Two slots claim the same logical document.
    DuplicateAddress {
        /// Zero-based duplicate slot.
        index: usize,
    },
    /// A source has invalid depth, root indices, or a non-BFS order.
    InvalidSource {
        /// Zero-based graph slot.
        index: usize,
    },
    /// An edge, provenance or coverage record names a document outside the set.
    UnknownGraphAddress,
}

impl fmt::Display for ScopeInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch => f.write_str("scope graph and content lengths do not match"),
            Self::TooManyDocuments => f.write_str("scope content exceeds the document ceiling"),
            Self::AddressMismatch { index } => {
                write!(f, "scope content address does not match graph slot {index}")
            }
            Self::DuplicateAddress { index } => {
                write!(f, "scope graph repeats an address at slot {index}")
            }
            Self::InvalidSource { index } => write!(
                f,
                "scope source order or coordinates are invalid at slot {index}"
            ),
            Self::UnknownGraphAddress => {
                f.write_str("scope graph refers to a document outside the loaded set")
            }
        }
    }
}
impl Error for ScopeInputError {}

impl<'a> QueryScopeView<'a> {
    /// Borrow graph and content only after validating their positional contract.
    /// No content is cloned, serialized, loaded or reinterpreted.
    ///
    /// # Errors
    /// Rejects mismatched addresses/order/counts and graph references outside the set.
    pub fn new(
        graph: &'a ResolvedDocumentScope,
        documents: &'a [ResolvedContent],
    ) -> Result<Self, ScopeInputError> {
        if graph.documents.len() != documents.len() {
            return Err(ScopeInputError::LengthMismatch);
        }
        if documents.len() > MAX_SCOPE_DOCUMENT_LIMIT as usize {
            return Err(ScopeInputError::TooManyDocuments);
        }
        let mut addresses = BTreeSet::new();
        let mut depth = 0;
        for (index, (source, content)) in graph.documents.iter().zip(documents).enumerate() {
            if content.address.as_ref() != Some(&source.address) {
                return Err(ScopeInputError::AddressMismatch { index });
            }
            if !addresses.insert(&source.address) {
                return Err(ScopeInputError::DuplicateAddress { index });
            }
            if source.depth < depth
                || source.depth > MAX_SCOPE_DEPTH
                || source
                    .root_indices
                    .iter()
                    .any(|root| usize::from(*root) >= graph.query.documents.len())
                || (source.depth != 0 && !source.root_indices.is_empty())
            {
                return Err(ScopeInputError::InvalidSource { index });
            }
            depth = source.depth;
        }
        let known = |address: &mant_ir::DocumentAddress| addresses.contains(address);
        if graph
            .edges
            .iter()
            .any(|edge| !known(&edge.from) || !known(&edge.to))
            || graph
                .documents
                .iter()
                .any(|source| source.reached_from.iter().any(|from| !known(from)))
            || graph.frontier.iter().any(|item| !known(&item.from))
            || graph
                .unresolved
                .iter()
                .any(|item| item.from.as_ref().is_some_and(|from| !known(from)))
            || graph
                .reference_limits
                .iter()
                .any(|item| !known(&item.document))
        {
            return Err(ScopeInputError::UnknownGraphAddress);
        }
        Ok(Self { graph, documents })
    }

    /// The borrowed logical graph and its loading/coverage report.
    #[must_use]
    pub const fn graph(self) -> &'a ResolvedDocumentScope {
        self.graph
    }

    /// Borrow original content in the validated graph order.
    #[must_use]
    pub const fn documents(self) -> &'a [ResolvedContent] {
        self.documents
    }

    /// Paired records cannot silently truncate or refer to another content order.
    #[must_use]
    pub fn iter(self) -> impl ExactSizeIterator<Item = (&'a ScopedDocument, &'a ResolvedContent)> {
        self.graph.documents.iter().zip(self.documents)
    }
}
