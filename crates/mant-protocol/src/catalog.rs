//! Versioned contracts for discovering locally available documents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use mant_ir::{DocumentAddress, MarkdownOrigin};

use crate::{SearchCase, SearchSyntax};

/// Exact schema marker for a local document catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatalogSchema {
    /// Version 0.8 of the pre-stable document-catalog protocol.
    #[serde(rename = "mant.catalog/v0.8")]
    V0Dot8,
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
    /// Stable logical path used by tree and discovery frontends.
    pub catalog_path: String,
}

/// Deterministically ordered page of discoverable local documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:catalog:v0.8"))]
pub struct DocumentCatalog {
    /// Exact response schema discriminator.
    pub schema: CatalogSchema,
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
            schema: CatalogSchema::V0Dot8,
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
    /// No literal pattern was supplied.
    Unranked,
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
    } else {
        CatalogMatchRank::Substring
    }
}

#[must_use]
/// Return the default maximum number of catalog rows.
pub const fn default_catalog_limit() -> u32 {
    100
}
