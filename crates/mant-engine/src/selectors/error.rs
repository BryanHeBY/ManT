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
    /// Explanation lookup found no semantic entry, but the same text occurs
    /// elsewhere in the rendered document.
    SelectorFoundOnlyInText {
        /// Requested document label.
        document: String,
        /// Unresolved semantic-entry selector.
        selector: String,
        /// Canonical path of the nearest addressable node.
        path: String,
        /// Display title of the nearest addressable node.
        title: String,
        /// One-based rendered line containing the first occurrence.
        line: u32,
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
    /// Explanation lookup selected a non-entry node.
    ExplanationRequiresEntry {
        /// Requested document label.
        document: String,
        /// Selector naming the non-entry node.
        selector: String,
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
            Self::SelectorFoundOnlyInText {
                document,
                selector,
                path,
                title,
                line,
            } => write!(
                formatter,
                "document '{document}' has no semantic entry '{selector}', but that text appears in outline node {path} ({title}) at line {line}"
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
            Self::ExplanationRequiresEntry { document, selector } => write!(
                formatter,
                "document '{document}' outline node '{selector}' is not a semantic entry; select a semantic entry instead"
            ),
        }
    }
}

impl Error for ProjectionError {}
