//! Versioned contracts for discovering locally available documents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use mant_ir::{DocumentAddress, MarkdownOrigin};

use crate::{SearchCase, SearchSyntax};

/// Exact schema marker for a local document catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatalogSchema {
    /// Version 0.9 of the pre-stable document-catalog protocol.
    #[serde(rename = "mant.catalog/v0.9")]
    V0Dot9,
}

impl CatalogSchema {
    /// Serialized identifier of the current catalog contract.
    pub const ID: &'static str = "mant.catalog/v0.9";
}

/// Optional family filter for catalog discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogDocumentKind {
    /// Registered Markdown documents.
    Markdown,
    /// Native manual pages.
    Manual,
}

/// Bounded filtering and pagination shared by CLI, TUI, and MCP discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogQuery {
    /// Optional name or catalog-path pattern.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Pattern language used by [`Self::pattern`].
    #[serde(default)]
    pub syntax: SearchSyntax,
    /// Case-matching policy.
    #[serde(default)]
    pub case: SearchCase,
    /// Optional document-family restriction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<CatalogDocumentKind>,
    /// Optional configured Markdown source name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Optional native manual category such as `1` or `3p`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
    /// Maximum rows returned after filtering.
    #[serde(default = "default_catalog_limit")]
    #[schemars(range(min = 1, max = 10000))]
    pub limit: u32,
    /// Number of matching rows skipped before collecting results.
    #[serde(default)]
    pub offset: u32,
}

impl Default for CatalogQuery {
    fn default() -> Self {
        Self {
            pattern: None,
            syntax: SearchSyntax::Literal,
            case: SearchCase::Insensitive,
            kind: None,
            source: None,
            manual_section: None,
            limit: default_catalog_limit(),
            offset: 0,
        }
    }
}

/// One catalog row identified entirely by logical names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummary {
    /// Stable logical document identity.
    pub address: DocumentAddress,
}

/// Indexed namespaces available to one catalog query.
///
/// `scope_total` is counted after applying the document-family, source, and
/// manual-section selectors but before applying the name pattern. It lets a
/// consumer distinguish an empty match set from an unindexed query scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCoverage {
    /// Documents inside the selected scope before name matching.
    pub scope_total: u32,
    /// Exact native manual sections present anywhere in the local index.
    pub manual_sections: Vec<String>,
    /// Configured Markdown source names that currently contribute documents.
    pub markdown_sources: Vec<String>,
    /// Whether the personal documents tree currently contributes documents.
    pub personal_documents: bool,
}

impl DocumentSummary {
    /// Derive the stable logical path used by tree and discovery frontends.
    #[must_use]
    pub fn catalog_path(&self) -> String {
        self.address.catalog_path()
    }
}

/// Deterministically ordered page of discoverable local documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:catalog:v0.9"))]
pub struct DocumentCatalog {
    /// Exact response schema discriminator.
    pub schema: CatalogSchema,
    /// Normalized query used to construct this page.
    pub query: CatalogQuery,
    /// Coverage of the local catalog independently from the name pattern.
    pub coverage: CatalogCoverage,
    /// Total rows matching the filters before pagination.
    pub total: u32,
    /// Number of rows present in [`Self::documents`].
    pub returned: u32,
    /// Applied zero-based result offset.
    pub offset: u32,
    /// Whether matching rows remain after this page.
    pub truncated: bool,
    /// Offset for the next page, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    /// Deterministically ordered page of document summaries.
    pub documents: Vec<DocumentSummary>,
}

impl Default for DocumentCatalog {
    fn default() -> Self {
        Self {
            schema: CatalogSchema::V0Dot9,
            query: CatalogQuery::default(),
            coverage: CatalogCoverage::default(),
            total: 0,
            returned: 0,
            offset: 0,
            truncated: false,
            next_offset: None,
            documents: Vec::new(),
        }
    }
}

/// Stable relevance tier for literal catalog matching.
///
/// Frontends may add deterministic tie-breakers inside one tier, but must not
/// place a prefix after a mere substring or an exact name after either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CatalogMatchRank {
    /// Complete name or path equality.
    Exact,
    /// Pattern equals the final slash-delimited path component.
    ComponentSuffix,
    /// Name or path begins with the pattern.
    Prefix,
    /// Pattern occurs elsewhere in the name or path.
    Substring,
    /// A literal pattern was supplied but does not occur in this candidate.
    NoMatch,
    /// No literal pattern was supplied.
    Unranked,
}

/// Spelling fidelity inside one catalog relevance tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CatalogSpellingRank {
    /// The candidate satisfies its relevance relation with the query's exact
    /// spelling, including case.
    Exact,
    /// The relation holds only after case folding.
    Folded,
    /// No literal spelling comparison applies.
    Unranked,
}

/// Complete literal relevance score shared by catalog frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CatalogMatchScore {
    /// Structural name/path relevance.
    pub relevance: CatalogMatchRank,
    /// Case fidelity within that relevance tier.
    pub spelling: CatalogSpellingRank,
}

/// Rank one document name or slash-delimited path using the catalog's
/// literal-search case policy.
#[must_use]
pub fn catalog_literal_match_rank(
    name: &str,
    pattern: Option<&str>,
    case: SearchCase,
) -> CatalogMatchRank {
    let Some(pattern) = pattern else {
        return CatalogMatchRank::Unranked;
    };
    let insensitive = case == SearchCase::Insensitive
        || case == SearchCase::Smart && !pattern.chars().any(char::is_uppercase);
    let (name, pattern) = if insensitive {
        (name.to_lowercase(), pattern.to_lowercase())
    } else {
        (name.to_owned(), pattern.to_owned())
    };
    if name == pattern {
        CatalogMatchRank::Exact
    } else if name.ends_with(&format!("/{pattern}")) {
        CatalogMatchRank::ComponentSuffix
    } else if name.starts_with(&pattern) {
        CatalogMatchRank::Prefix
    } else if name.contains(&pattern) {
        CatalogMatchRank::Substring
    } else {
        CatalogMatchRank::NoMatch
    }
}

/// Rank one literal candidate while preferring case-faithful spellings inside
/// the same exact, prefix, or substring tier.
#[must_use]
pub fn catalog_literal_match_score(
    name: &str,
    pattern: Option<&str>,
    case: SearchCase,
) -> CatalogMatchScore {
    let relevance = catalog_literal_match_rank(name, pattern, case);
    let Some(pattern) = pattern else {
        return CatalogMatchScore {
            relevance,
            spelling: CatalogSpellingRank::Unranked,
        };
    };
    let exact_relation = match relevance {
        CatalogMatchRank::Exact => name == pattern,
        CatalogMatchRank::ComponentSuffix => name.ends_with(&format!("/{pattern}")),
        CatalogMatchRank::Prefix => name.starts_with(pattern),
        CatalogMatchRank::Substring => name.contains(pattern),
        CatalogMatchRank::NoMatch | CatalogMatchRank::Unranked => {
            return CatalogMatchScore {
                relevance,
                spelling: CatalogSpellingRank::Unranked,
            };
        }
    };
    CatalogMatchScore {
        relevance,
        spelling: if exact_relation {
            CatalogSpellingRank::Exact
        } else {
            CatalogSpellingRank::Folded
        },
    }
}

#[must_use]
/// Return the default maximum number of catalog rows.
pub const fn default_catalog_limit() -> u32 {
    100
}

#[cfg(test)]
mod tests {
    use super::{
        CatalogMatchRank, CatalogSpellingRank, catalog_literal_match_rank,
        catalog_literal_match_score,
    };
    use crate::SearchCase;

    #[test]
    fn literal_rank_distinguishes_substrings_from_non_matches() {
        assert_eq!(
            catalog_literal_match_rank("woman", Some("man"), SearchCase::Insensitive),
            CatalogMatchRank::Substring
        );
        assert_eq!(
            catalog_literal_match_rank("printf", Some("man"), SearchCase::Insensitive),
            CatalogMatchRank::NoMatch
        );
        assert_eq!(
            catalog_literal_match_rank("printf", None, SearchCase::Insensitive),
            CatalogMatchRank::Unranked
        );
    }

    #[test]
    fn literal_score_prefers_case_faithful_prefixes_inside_one_tier() {
        let lower = catalog_literal_match_score("execve", Some("exec"), SearchCase::Insensitive);
        let folded = catalog_literal_match_score("EXECUTE", Some("exec"), SearchCase::Insensitive);
        assert_eq!(lower.relevance, CatalogMatchRank::Prefix);
        assert_eq!(folded.relevance, CatalogMatchRank::Prefix);
        assert_eq!(lower.spelling, CatalogSpellingRank::Exact);
        assert_eq!(folded.spelling, CatalogSpellingRank::Folded);
        assert!(lower < folded);
    }
}
