//! Query envelope combining one structured input with optional tldr content.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    MantDocument, OutlineDetail, SearchCase, SearchScope, SearchSyntax, TldrDocument,
    default_search_limit,
};

/// Exact schema marker for a complete `ManT` query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum QuerySchema {
    /// Query envelope built around `mant.document/v3`.
    #[serde(rename = "mant.query/v3")]
    V3,
}

/// Exact schema marker for a native query request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RequestSchema {
    /// Query and projection request accepted through `--request-json`.
    #[serde(rename = "mant.request/v3")]
    V3,
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
    /// Locate a page through the host's configured manual database.
    Manual {
        topic: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        section: Option<String>,
    },
    /// Read and parse one local Markdown file.
    MarkdownFile { path: String },
}

/// Projection requested after loading one complete manual query.
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

/// Validated use-case input accepted by the native query boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:request:v3"))]
pub struct QueryRequest {
    pub schema: RequestSchema,
    pub input: QueryInput,
    pub view: QueryView,
}

/// Native result consumed by JSON, Markdown, and interactive frontends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:query:v3"))]
pub struct QueryBundle {
    pub schema: QuerySchema,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<MantDocument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tldr: Option<TldrDocument>,
}
