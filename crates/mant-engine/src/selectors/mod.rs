//! Document-local selector policy and producer diagnostics, independent of DTO projection.
mod diagnostics;
mod error;
mod index;
mod located;
use crate::definitions::environment_variable_body;
pub(crate) use diagnostics::semantic_selector_diagnostics;
pub use error::{ProjectionError, SelectorCandidate};
pub(crate) use index::DocumentSelectorIndex;
pub(crate) use located::{LocatedBreadcrumb, LocatedNode, collect_root_entries, collect_sections};
use mant_ir::{DOCUMENT_ROOT_ID, EntryKind, OutlinePath};
pub(crate) const TLDR_ID: &str = "tldr";
pub(crate) const DOCUMENT_ROOT_TITLE: &str = "OVERVIEW";

/// Whether an identifier belongs to the selector namespace rather than a
/// document-defined node.
///
/// Section paths use dotted positive indices (`2.1`), while semantic entries
/// append a semantic-entry index (`2.1/e3`). The parser reserves the complete grammar,
/// not only selectors present in one particular document, so source-defined
/// IDs can never make excerpt lookup ambiguous.
pub(crate) fn is_reserved_selector(value: &str) -> bool {
    matches!(value, TLDR_ID | DOCUMENT_ROOT_ID)
        || value.parse::<OutlinePath>().is_ok()
        || [
            "option-",
            "marker-",
            "operand-",
            "command-",
            "configuration-",
            "environment-",
            "variable-",
            "value-",
            "term-",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

fn semantic_name_shorthand(role: EntryKind, name: &str) -> Option<&str> {
    match role {
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        } => {
            let shorthand = name.trim_start_matches('-');
            (shorthand != name && !shorthand.is_empty()).then_some(shorthand)
        }
        EntryKind::EnvironmentVariable => {
            environment_variable_body(name).filter(|body| *body != name)
        }
        EntryKind::Command
        | EntryKind::ConfigurationKey
        | EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Marker | mant_ir::ParameterKind::Operand,
        }
        | EntryKind::Variable
        | EntryKind::Value
        | EntryKind::Term => None,
    }
}

fn ambiguous_selector(
    document: &str,
    selector: &str,
    matches: Vec<&LocatedNode<'_>>,
) -> ProjectionError {
    ProjectionError::AmbiguousSelector {
        document: document.to_owned(),
        selector: selector.to_owned(),
        candidates: matches
            .into_iter()
            .map(|candidate| SelectorCandidate {
                path: candidate.path().to_string(),
                id: candidate.id().into(),
            })
            .collect(),
    }
}
