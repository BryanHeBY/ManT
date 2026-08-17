//! Query envelope combining one structured input with optional tldr content.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use mant_ir::{ResolvedContent, TldrDocument};

use crate::{
    DocumentAddress, DocumentResponse, NodeSelector, OutlineDetail, SearchCase, SearchScope,
    SearchSyntax, default_search_limit,
};

/// Exact schema marker for a complete `ManT` query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum QuerySchema {
    /// Query envelope built around `mant.document/v0.8`.
    #[serde(rename = "mant.query/v0.8")]
    V0Dot8,
}

impl QuerySchema {
    /// Serialized identifier of the current query response contract.
    pub const ID: &'static str = "mant.query/v0.8";
}

/// Exact schema marker for a native query request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RequestSchema {
    /// Query and projection request accepted through `--request-json`.
    #[serde(rename = "mant.request/v0.8")]
    V0Dot8,
}

impl RequestSchema {
    /// Serialized identifier of the current request contract.
    pub const ID: &'static str = "mant.request/v0.8";
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
    /// Resolve personal Markdown first, then configured sources around the
    /// priority-zero native-manual baseline.
    Document {
        /// Hierarchical catalog path or unqualified component-suffix selector.
        selector: String,
        /// Optional configured Markdown source. It bypasses root documents and manuals.
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        /// Optional native manual category such as `1` or `3p`.
        #[serde(skip_serializing_if = "Option::is_none")]
        manual_section: Option<String>,
    },
    /// Read and parse one explicit local Markdown or roff file.
    File {
        /// Physical path supplied by the caller.
        path: String,
        /// Parser-selection policy for the file.
        format: InputFormat,
    },
}

/// Parser selected for an explicit physical input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InputFormat {
    /// Infer the parser from extension and content conventions.
    #[default]
    Auto,
    /// Parse the input as Markdown.
    Markdown,
    /// Parse the input as roff with libmandoc.
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
    /// Return the complete structured query bundle.
    Full {},
    /// Return a navigable structural projection.
    Outline {
        /// Amount of semantic detail included in the outline.
        detail: OutlineDetail,
    },
    /// Return content selected by one or more node paths, IDs, or aliases.
    Excerpt {
        /// Ordered selectors resolved by the engine.
        #[schemars(length(min = 1))]
        selectors: Vec<NodeSelector>,
    },
    /// Resolve exactly one semantic entry and return its complete description.
    Explain {
        /// Exact or normalized semantic entry name.
        #[schemars(length(min = 1))]
        entry: String,
    },
    /// Search visible document content with bounded pagination.
    Search {
        /// Literal or regular-expression search pattern.
        #[schemars(length(min = 1, max = 4096))]
        pattern: String,
        /// Pattern language.
        #[serde(default)]
        syntax: SearchSyntax,
        /// Case-matching policy.
        #[serde(default)]
        case: SearchCase,
        /// Semantic content included in the search.
        #[serde(default)]
        scope: SearchScope,
        /// Require matches to be bounded by word boundaries.
        #[serde(default)]
        word: bool,
        /// Neighboring rendered lines included around each match.
        #[serde(default)]
        #[schemars(range(max = 100))]
        context_lines: u16,
        /// Maximum number of matches returned.
        #[serde(default = "default_search_limit")]
        #[schemars(range(min = 1, max = 10000))]
        limit: u32,
        /// Number of matching results skipped before collection.
        #[serde(default)]
        offset: u32,
    },
}

/// Native use-case input. The engine validates semantic constraints before I/O.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:request:v0.8"))]
pub struct QueryRequest {
    /// Exact request schema discriminator.
    pub schema: RequestSchema,
    /// Document source to resolve.
    pub input: QueryInput,
    /// Projection applied after the document is loaded.
    pub view: QueryView,
}

/// Versioned full-query result emitted at CLI and request JSON boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:query:v0.8"))]
pub struct QueryBundle {
    /// Exact response schema discriminator.
    pub schema: QuerySchema,
    /// Human-readable selected-document label.
    pub label: String,
    /// Exact registered address selected for this query. Direct input paths
    /// and standard input do not belong to the registered catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<DocumentAddress>,
    /// Authoritative structured document, when found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentResponse>,
    /// Optional quick-reference page resolved alongside the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tldr: Option<TldrDocument>,
}

impl From<&ResolvedContent> for QueryBundle {
    fn from(content: &ResolvedContent) -> Self {
        Self {
            schema: QuerySchema::V0Dot8,
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
