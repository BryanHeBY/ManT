//! Independent bounded evidence collection over immutable content owners.
mod collect;
mod materialize;
mod relations;

use crate::selectors::{LocatedNode, collect_root_entries, collect_sections};
use mant_ir::{Block, DefinitionCase, EntryOwner, OutlinePath, ResolvedContent, SourceSpan};
use mant_protocol::{EvidenceBasis, ExplanationQuery, QueryExplanation};

/// Invalid explanation request or missing readable source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplanationError {
    /// Empty, overlong, or control-bearing literal.
    Entry(mant_protocol::ScopeTextError),
    /// Result count is outside 1..=256.
    ResultLimit,
    /// Content copy budget is outside 1..=4 MiB.
    ContentLimit,
    /// No document or quick reference was loaded.
    MissingContent,
}
impl std::fmt::Display for ExplanationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Entry(mant_protocol::ScopeTextError::Empty) => {
                f.write_str("explanation entry must not be empty")
            }
            Self::Entry(mant_protocol::ScopeTextError::ControlCharacter) => {
                f.write_str("explanation entry must not contain control characters")
            }
            Self::Entry(mant_protocol::ScopeTextError::TooLong { maximum }) => write!(
                f,
                "explanation entry must not exceed {maximum} Unicode scalar values"
            ),
            Self::ResultLimit => f.write_str("explanation limit must be between 1 and 256"),
            Self::ContentLimit => {
                f.write_str("explanation content budget must be between 1 and 4194304 bytes")
            }
            Self::MissingContent => f.write_str("explanation requires readable content"),
        }
    }
}
impl std::error::Error for ExplanationError {}

/// Validate literal and copy bounds before loading any source.
///
/// # Errors
/// Returns the first violated request bound.
pub fn validate_explanation_query(query: &ExplanationQuery) -> Result<(), ExplanationError> {
    mant_protocol::validate_scope_text(&query.entry, mant_protocol::MAX_SEMANTIC_ENTRY_CHARS)
        .map_err(ExplanationError::Entry)?;
    if !(1..=mant_protocol::MAX_EXPLANATION_RESULTS).contains(&query.options.limit) {
        return Err(ExplanationError::ResultLimit);
    }
    if !(1..=mant_protocol::MAX_EXPLANATION_CONTENT_BYTES).contains(&query.options.content_bytes) {
        return Err(ExplanationError::ContentLimit);
    }
    Ok(())
}

/// Collect independent semantic and literal evidence without unique selection.
///
/// Matching names/forms and owner coordinates, direct ordinary content, and
/// validated explicit relations are kept distinct. This never executes code,
/// performs I/O, resolves remote value domains or modifies the input tree.
///
/// # Errors
/// Returns invalid request bounds or absence of readable source. Multiple and
/// zero matching owners are normal results, not navigation errors.
pub fn explain_query(
    content: &ResolvedContent,
    query: &ExplanationQuery,
) -> Result<QueryExplanation, ExplanationError> {
    validate_explanation_query(query)?;
    if content.document.is_none() && content.tldr.is_none() {
        return Err(ExplanationError::MissingContent);
    }
    let query = ExplanationQuery {
        entry: query.entry.trim().to_owned(),
        options: query.options,
    };
    let mut located = Vec::new();
    if let Some(document) = &content.document {
        collect_root_entries(&document.blocks, &mut located);
        collect_sections(&document.sections, &[], &[], &mut located);
    }
    let (mut candidates, truncated, orders) = collect::collect(content, &query.entry, &located);
    let relations_truncated =
        relations::expand(content, &query.entry, &located, &orders, &mut candidates);
    candidates.sort_by_key(|candidate| candidate.order);
    Ok(materialize::response(
        content,
        query,
        &located,
        candidates,
        truncated,
        relations_truncated,
    ))
}

struct Candidate<'a> {
    order: usize,
    located: Option<usize>,
    ordinary: Option<&'a Block>,
    section: Option<usize>,
    block_path: Option<String>,
    source: Option<SourceSpan>,
    bases: Vec<EvidenceBasis>,
}

fn same(left: &str, right: &str, case: DefinitionCase) -> bool {
    match case {
        DefinitionCase::Sensitive => left == right,
        DefinitionCase::Insensitive => left.eq_ignore_ascii_case(right),
    }
}

fn owner<'a>(node: &LocatedNode<'a>) -> Option<EntryOwner<'a>> {
    match node {
        LocatedNode::Entry { entry, .. } => Some(entry.item),
        LocatedNode::Section { .. } => None,
    }
}

fn is_identity(node: &LocatedNode<'_>, entry: &str) -> bool {
    node.id() == entry
        || entry
            .parse::<OutlinePath>()
            .is_ok_and(|path| &path == node.path())
}
