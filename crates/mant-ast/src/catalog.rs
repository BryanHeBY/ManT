//! Versioned contracts for discovering locally available documents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{SearchCase, SearchSyntax};

/// Exact schema marker for a local document catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatalogSchema {
    #[serde(rename = "mant.catalog/v7")]
    V7,
}

/// Storage identity of one registered Markdown document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MarkdownOrigin {
    Documents,
    Source { name: String },
}

/// Stable selector for one discoverable document candidate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum DocumentAddress {
    Markdown {
        name: String,
        origin: MarkdownOrigin,
    },
    Manual {
        name: String,
        section: String,
    },
}

impl DocumentAddress {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Markdown { name, .. } | Self::Manual { name, .. } => name,
        }
    }
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
    pub section: Option<String>,
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
            section: None,
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
    pub path: String,
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

#[must_use]
pub const fn default_catalog_limit() -> u32 {
    100
}
