//! Versioned contracts for discovering locally available documents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use mant_ir::{DocumentAddress, MarkdownOrigin};

use crate::{SearchCase, SearchSyntax};

/// Exact schema marker for a local document catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatalogSchema {
    #[serde(rename = "mant.catalog/v7")]
    V7,
}

/// Optional family filter for catalog discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogDocumentKind {
    Markdown,
    Manual,
}

/// Bounded filtering and pagination shared by CLI, TUI, and MCP discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default)]
    pub syntax: SearchSyntax,
    #[serde(default)]
    pub case: SearchCase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<CatalogDocumentKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
    #[serde(default = "default_catalog_limit")]
    #[schemars(range(min = 1, max = 10000))]
    pub limit: u32,
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

/// One catalog row. Paths describe local provenance but are not document IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummary {
    pub address: DocumentAddress,
    /// Stable logical path used by tree and discovery frontends.
    pub catalog_path: String,
    /// Local file provenance; never a document identifier.
    pub source_path: String,
}

/// Deterministically ordered page of discoverable local documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:catalog:v7"))]
pub struct DocumentCatalog {
    pub schema: CatalogSchema,
    pub total: u32,
    pub returned: u32,
    pub offset: u32,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    pub documents: Vec<DocumentSummary>,
}

/// Stable relevance tier for literal catalog matching.
///
/// Frontends may add deterministic tie-breakers inside one tier, but must not
/// place a prefix after a mere substring or an exact name after either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CatalogMatchRank {
    Exact,
    ComponentSuffix,
    Prefix,
    Substring,
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
pub const fn default_catalog_limit() -> u32 {
    100
}
