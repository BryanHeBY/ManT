//! Query validation failures cannot carry host acquisition errors.
use mant_protocol::ScopeTextError;
use mant_query::SearchError;
use std::{error::Error, fmt};

/// Invalid query view, independent of source acquisition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryValidationError {
    /// Excerpt projection was requested without selectors.
    EmptySelection,
    /// Excerpt projection exceeded the closed selector-count bound.
    TooManySelections {
        /// Maximum selectors accepted by one focused request.
        maximum: usize,
    },
    /// An excerpt selector was empty.
    EmptySelector,
    /// An explicit local selector violated its path/ID grammar or byte limit.
    InvalidContentSelector,
    /// Reference inventory policy violated its closed bounds.
    InvalidReferenceProjection(&'static str),
    /// A role-filtered outline contained no kinds or exceeded the closed kind family.
    InvalidEntryKinds,
    /// An explanation entry name was empty.
    EmptyEntry,
    /// A node or semantic-entry selector violated the bounded request contract.
    InvalidViewSelector {
        /// User-facing field name.
        field: &'static str,
        /// Precise bound or character violation.
        error: ScopeTextError,
    },
    /// Search configuration failed validation.
    InvalidSearch(SearchError),
    /// Explanation configuration failed validation or had no readable content.
    InvalidExplanation(mant_query::ExplanationError),
}

impl fmt::Display for QueryValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySelection => formatter.write_str("at least one outline node is required"),
            Self::TooManySelections { maximum } => {
                write!(
                    formatter,
                    "outline nodes must not contain more than {maximum} values"
                )
            }
            Self::EmptySelector => formatter.write_str("outline node must not be empty"),
            Self::InvalidContentSelector => mant_protocol::InvalidContentSelector.fmt(formatter),
            Self::InvalidReferenceProjection(reason) => formatter.write_str(reason),
            Self::InvalidEntryKinds => {
                formatter.write_str("outline entry kinds must contain between 1 and 9 values")
            }
            Self::EmptyEntry => formatter.write_str("semantic entry must not be empty"),
            Self::InvalidViewSelector { field, error } => {
                write!(formatter, "{field} {}", view_selector_error_message(*error))
            }
            Self::InvalidSearch(error) => error.fmt(formatter),
            Self::InvalidExplanation(error) => error.fmt(formatter),
        }
    }
}
impl Error for QueryValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSearch(error) => Some(error),
            Self::InvalidExplanation(error) => Some(error),
            _ => None,
        }
    }
}
fn view_selector_error_message(error: ScopeTextError) -> String {
    match error {
        ScopeTextError::Empty => "must not be empty".to_owned(),
        ScopeTextError::ControlCharacter => "must not contain control characters".to_owned(),
        ScopeTextError::TooLong { maximum } => {
            format!("must not exceed {maximum} Unicode scalar values")
        }
    }
}
