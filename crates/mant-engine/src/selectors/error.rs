//! Existing public selection errors; re-exported through the crate facade.
use std::{error::Error, fmt};

/// Failure to derive an addressable view from a complete query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    /// Neither an authoritative document nor a quick reference is available.
    MissingContent {
        /// Requested document label.
        document: String,
    },
    /// Excerpt projection received no selectors.
    EmptySelection,
    /// One selector was empty after trimming.
    EmptySelector,
    /// No addressable node matched a selector.
    UnknownSelector {
        /// Requested document label.
        document: String,
        /// Unresolved selector.
        selector: String,
    },
    /// An alias matched more than one semantic entry.
    AmbiguousSelector {
        /// Requested document label.
        document: String,
        /// Ambiguous selector.
        selector: String,
        /// Stable paths and IDs that disambiguate the match.
        candidates: Vec<SelectorCandidate>,
    },
}

/// One stable qualification offered when a semantic alias is ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorCandidate {
    /// Canonical structural outline path.
    pub path: String,
    /// Stable document-local identity.
    pub id: String,
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingContent { document } => {
                write!(formatter, "document '{document}' has no available content")
            }
            Self::EmptySelection => formatter.write_str("at least one outline node is required"),
            Self::EmptySelector => formatter.write_str("outline node must not be empty"),
            Self::UnknownSelector { document, selector } => write!(
                formatter,
                "document '{document}' has no outline node '{selector}'; inspect its entries outline for available selectors and diagnostics"
            ),
            Self::AmbiguousSelector {
                document,
                selector,
                candidates,
            } => {
                write!(
                    formatter,
                    "document '{document}' has multiple semantic entries named '{selector}': "
                )?;
                for (index, candidate) in candidates.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{} ({})", candidate.path, candidate.id)?;
                }
                formatter.write_str("; select one by path or ID")
            }
        }
    }
}

impl Error for ProjectionError {}
