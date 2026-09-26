//! Independent bounded evidence collection over immutable content owners.
mod collect;
mod literal;
mod location;
pub use location::resolve_explanation_block;
mod details;
mod fixed;
mod matches;
mod materialize;
mod page;
mod plan;
mod positions;
mod preview;
mod projection;
mod relations;
mod scoped;
mod support;
pub(crate) use scoped::explain as explain_scope;

use crate::selectors::{LocatedNode, collect_root_entries, collect_sections};
use mant_ir::{Block, EntryOwner, NameCase, OutlinePath, ResolvedContent, SourceSpan};
use mant_protocol::{EvidenceBasis, ExplanationQuery, QueryExplanation};

/// Invalid explanation request or missing readable source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplanationError {
    /// The selected Fixed document requires annotated explanation support.
    UnsupportedFixed,
    /// A Fixed display selection or hierarchy could not be read safely.
    InvalidFixed,
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
            Self::UnsupportedFixed => {
                f.write_str("Fixed document explanation is not yet supported")
            }
            Self::InvalidFixed => f.write_str("invalid Fixed explanation selection"),
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
    explain_with_usage(content, query).map(|(response, _)| response)
}

pub(crate) fn explain_with_usage(
    content: &ResolvedContent,
    query: &ExplanationQuery,
) -> Result<(QueryExplanation, u32), ExplanationError> {
    validate_explanation_query(query)?;
    if let Some(document) = &content.document
        && let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body()
    {
        return fixed::response(content, document, fixed, query);
    }
    let plan = collection_plan(content, query.entry.trim())?;
    Ok(materialize::response(plan, query))
}

fn collection_plan<'a>(
    content: &'a ResolvedContent,
    entry: &str,
) -> Result<plan::CollectionPlan<'a>, ExplanationError> {
    if content.document.is_none() && content.tldr.is_none() {
        return Err(ExplanationError::MissingContent);
    }
    if content
        .document
        .as_ref()
        .is_some_and(|document| document.flow().is_none())
    {
        return Err(ExplanationError::UnsupportedFixed);
    }
    let mut located = Vec::new();
    if let Some(document) = &content.document {
        let content = document.content();
        let flow = document.flow().expect("Fixed rejected above");
        collect_root_entries(content, &flow.blocks, &mut located);
        collect_sections(content, &flow.sections, &[], &[], &mut located);
    }
    // Locations preserve the original tree, including optional entry facts
    // whose bindings are damaged. Only complete, owner-local checked facts
    // may suppress literal HEAD support or become direct/related evidence.
    let accepted = content.document.as_ref().map_or_else(Vec::new, |document| {
        let store_view = document.content();
        located
            .iter()
            .map(|node| {
                owner(node).is_some_and(|owner| {
                    store_view
                        .entry_validated_names(owner)
                        .is_ok_and(|names| names.is_some())
                })
            })
            .collect()
    });
    let validation = content
        .document
        .as_ref()
        .map(mant_ir::DocumentValidation::new);
    let (mut candidates, orders, supports) = collect::collect(content, entry, &located, &accepted);
    let relations = relations::expand(
        validation.as_ref(),
        entry,
        &located,
        &accepted,
        &orders,
        &mut candidates,
    );
    let (candidates, truncated) = candidates.finish();
    let mut diagnostics = content
        .document
        .as_ref()
        .map(|d| d.diagnostics.clone())
        .unwrap_or_default();
    let rejected_aliases = validation
        .as_ref()
        .into_iter()
        .flat_map(mant_ir::DocumentValidation::relation_issues)
        .filter(|issue| {
            matches!(
                issue.kind,
                mant_ir::EntryRelationIssueKind::AliasOf | mant_ir::EntryRelationIssueKind::Cycle
            )
        })
        .map(|issue| issue.owner.clone())
        .collect();
    if let Some(validation) = validation {
        for diagnostic in validation.into_diagnostics() {
            if !diagnostics.contains(&diagnostic) {
                diagnostics.push(diagnostic);
            }
        }
    }
    Ok(plan::CollectionPlan {
        content,
        located,
        candidates,
        supports,
        diagnostics,
        rejected_aliases,
        truncation: mant_protocol::ExplanationTruncation {
            candidates: truncated,
            relations,
            content: false,
        },
    })
}

/// Collect semantic evidence with the documented default page/copy budgets.
/// Use [`explain_query`] for explicit pagination; use [`crate::select_excerpt`]
/// for strict node navigation. No-evidence is a normal response.
///
/// # Errors
/// Returns invalid literal input or missing readable content.
pub fn select_explanation(
    content: &ResolvedContent,
    entry: &str,
) -> Result<QueryExplanation, ExplanationError> {
    explain_query(
        content,
        &ExplanationQuery {
            entry: entry.to_owned(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
}

struct Candidate<'a> {
    order: usize,
    located: Option<usize>,
    ordinary: Option<&'a Block>,
    /// An unclassified definition term copies only its item from the
    /// ordinary list, not the potentially large containing list.
    ordinary_item: Option<usize>,
    section: Option<usize>,
    block_path: Option<String>,
    source: Option<SourceSpan>,
    bases: Vec<EvidenceBasis>,
    matched: matches::MatchPlan,
    hits: Vec<preview::LiteralHit<'a>>,
}

fn same(left: &str, right: &str, case: NameCase) -> bool {
    match case {
        NameCase::Sensitive => left == right,
        NameCase::Insensitive => left.eq_ignore_ascii_case(right),
    }
}

fn owner<'a>(node: &LocatedNode<'a>) -> Option<EntryOwner<'a>> {
    match node {
        LocatedNode::Entry { entry, .. } => Some(entry.owner()),
        LocatedNode::Section { .. } => None,
    }
}

fn is_identity(node: &LocatedNode<'_>, entry: &str) -> bool {
    node.id() == entry
        || entry
            .parse::<OutlinePath>()
            .is_ok_and(|path| &path == node.path())
}
