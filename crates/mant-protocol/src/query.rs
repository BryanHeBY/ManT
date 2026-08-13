//! Query envelope combining one structured input with optional tldr content.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use mant_ir::{ResolvedContent, TldrDocument};

use crate::{
    DocumentAddress, DocumentResponse, OutlineDetail, SearchCase, SearchScope, SearchSyntax,
    default_search_limit,
};

/// Exact schema marker for a complete `ManT` query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum QuerySchema {
    /// Query envelope built around `mant.document/v7`.
    #[serde(rename = "mant.query/v7")]
    V7,
}

/// Exact schema marker for a native query request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RequestSchema {
    /// Query and projection request accepted through `--request-json`.
    #[serde(rename = "mant.request/v7")]
    V7,
}

/// Source selected by one public query request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum QueryInput {
    /// Resolve a registered Markdown document first, then a local manual page.
    Document {
        /// Hierarchical catalog path or unqualified component-suffix selector.
        selector: String,
        /// Optional configured Markdown source. It bypasses root documents and manuals.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        section: Option<String>,
    },
    /// Read and parse one explicit local Markdown or roff file.
    File { path: String, format: InputFormat },
}

/// Parser selected for an explicit physical input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InputFormat {
    #[default]
    Auto,
    Markdown,
    Roff,
}

/// Projection requested after loading one complete structured document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum QueryView {
    Full {},
    Outline {
        detail: OutlineDetail,
    },
    Excerpt {
        #[schemars(length(min = 1))]
        nodes: Vec<String>,
    },
    /// Resolve exactly one semantic entry and return its complete description.
    Explain {
        #[schemars(length(min = 1))]
        entry: String,
    },
    Search {
        #[schemars(length(min = 1, max = 4096))]
        pattern: String,
        #[serde(default)]
        syntax: SearchSyntax,
        #[serde(default)]
        case: SearchCase,
        #[serde(default)]
        scope: SearchScope,
        #[serde(default)]
        word: bool,
        #[serde(default)]
        #[schemars(range(max = 100))]
        context_lines: u16,
        #[serde(default = "default_search_limit")]
        #[schemars(range(min = 1, max = 10000))]
        limit: u32,
        #[serde(default)]
        offset: u32,
    },
}

/// Native use-case input. The engine validates semantic constraints before I/O.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:request:v7"))]
pub struct QueryRequest {
    pub schema: RequestSchema,
    pub input: QueryInput,
    pub view: QueryView,
}

/// Versioned full-query result emitted at JSON and MCP boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:query:v7"))]
pub struct QueryBundle {
    pub schema: QuerySchema,
    pub label: String,
    /// Exact registered address selected for this query. Direct input paths
    /// and standard input do not belong to the registered catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<DocumentAddress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tldr: Option<TldrDocument>,
}

impl From<&ResolvedContent> for QueryBundle {
    fn from(content: &ResolvedContent) -> Self {
        Self {
            schema: QuerySchema::V7,
            label: content.label.clone(),
            address: content.address.clone(),
            document: content.document.as_ref().map(Into::into),
            tldr: content.tldr.clone(),
        }
    }
}

impl From<QueryBundle> for ResolvedContent {
    fn from(bundle: QueryBundle) -> Self {
        Self {
            label: bundle.label,
            address: bundle.address,
            document: bundle.document.map(Into::into),
            tldr: bundle.tldr,
        }
    }
}
