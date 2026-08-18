//! Stable contracts for bounded queries over a linked set of documents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    DocumentAddress, QueryExcerpt, QuerySearch, SearchCase, SearchQuery, SearchScope, SearchSyntax,
    default_search_limit,
};

/// Maximum number of initial documents accepted by the native scope contract.
pub const MAX_SCOPE_DOCUMENTS: usize = 16;
/// Default maximum link distance from an initial document.
pub const DEFAULT_SCOPE_DEPTH: u16 = 8;
/// Hard maximum link distance accepted by the native scope contract.
pub const MAX_SCOPE_DEPTH: u16 = 32;
/// Default maximum number of distinct documents in one resolved scope.
pub const DEFAULT_SCOPE_DOCUMENT_LIMIT: u32 = 64;
/// Hard maximum number of distinct documents in one resolved scope.
pub const MAX_SCOPE_DOCUMENT_LIMIT: u32 = 256;

/// One logical document selector before catalog resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentSelector {
    /// Unqualified name or complete catalog path.
    #[schemars(length(min = 1, max = 1024))]
    pub selector: String,
    /// Optional configured Markdown source for an unqualified selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Optional native manual category for an unqualified selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
}

/// Bounded traversal applied after resolving the initial documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentTraversal {
    /// Follow typed links to other registered documents.
    #[serde(default)]
    pub follow_links: bool,
    /// Maximum number of link edges from an initial document.
    #[serde(default = "default_scope_depth")]
    #[schemars(range(max = 32))]
    pub max_depth: u16,
    /// Maximum number of distinct documents, including initial documents.
    #[serde(default = "default_scope_document_limit")]
    #[schemars(range(min = 1, max = 256))]
    pub max_documents: u32,
}

impl Default for DocumentTraversal {
    fn default() -> Self {
        Self {
            follow_links: false,
            max_depth: DEFAULT_SCOPE_DEPTH,
            max_documents: DEFAULT_SCOPE_DOCUMENT_LIMIT,
        }
    }
}

/// Return [`DEFAULT_SCOPE_DEPTH`].
#[must_use]
pub const fn default_scope_depth() -> u16 {
    DEFAULT_SCOPE_DEPTH
}

/// Return [`DEFAULT_SCOPE_DOCUMENT_LIMIT`].
#[must_use]
pub const fn default_scope_document_limit() -> u32 {
    DEFAULT_SCOPE_DOCUMENT_LIMIT
}

/// Initial documents and the link policy used to expand them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentScope {
    /// Ordered initial documents. The first one is the initial TUI page.
    #[schemars(length(min = 1, max = 16))]
    pub documents: Vec<DocumentSelector>,
    /// Deterministic outbound-link traversal policy.
    #[serde(default)]
    pub traversal: DocumentTraversal,
}

/// Exact schema marker for a scope-query request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ScopeRequestSchema {
    /// Version 0.8 of the pre-stable scope-query request.
    #[serde(rename = "mant.scope-request/v0.8")]
    V0Dot8,
}

impl ScopeRequestSchema {
    /// Serialized identifier of the current request contract.
    pub const ID: &'static str = "mant.scope-request/v0.8";
}

/// Query projection supported over a document set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ScopeQueryView {
    /// Resolve one semantic entry independently in every document.
    Explain {
        /// Exact alias, outline path, or stable ID.
        #[schemars(length(min = 1, max = 512))]
        entry: String,
    },
    /// Search visible or generated-Markdown text over the complete scope.
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
        /// Semantic representation searched.
        #[serde(default)]
        scope: SearchScope,
        /// Require Unicode-aware word boundaries.
        #[serde(default)]
        word: bool,
        /// Neighboring rendered lines included around a match.
        #[serde(default)]
        #[schemars(range(max = 100))]
        context_lines: u16,
        /// Global maximum number of matching line groups returned.
        #[serde(default = "default_search_limit")]
        #[schemars(range(min = 1, max = 10000))]
        limit: u32,
        /// Global number of matching line groups skipped.
        #[serde(default)]
        offset: u32,
    },
}

/// Native request for a bounded multi-document query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:scope-request:v0.8"))]
pub struct ScopeQueryRequest {
    /// Exact request schema discriminator.
    pub schema: ScopeRequestSchema,
    /// Initial documents and traversal limits.
    pub scope: DocumentScope,
    /// Projection applied independently to resolved documents.
    pub view: ScopeQueryView,
}

/// Exact schema marker for a resolved scope query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ScopeQuerySchema {
    /// Version 0.8 of the pre-stable scope-query result.
    #[serde(rename = "mant.scope-query/v0.8")]
    V0Dot8,
}

impl ScopeQuerySchema {
    /// Serialized identifier of the current result contract.
    pub const ID: &'static str = "mant.scope-query/v0.8";
}

/// Typed cross-document edge retained in a resolved scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentEdgeKind {
    /// A relative Markdown link inside one registered namespace.
    Document,
    /// A semantic native-manual reference.
    Manual,
}

/// One resolved edge in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentEdge {
    /// Address containing the link.
    pub from: DocumentAddress,
    /// Resolved linked address.
    pub to: DocumentAddress,
    /// Semantic link family.
    pub kind: DocumentEdgeKind,
}

/// One distinct document in breadth-first traversal order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopedDocument {
    /// Stable logical document identity.
    pub address: DocumentAddress,
    /// Minimum outbound-link distance from any initial document.
    pub depth: u16,
    /// Initial document positions that resolve to this address.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub root_indices: Vec<u16>,
    /// Distinct documents whose links reached this address.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reached_from: Vec<DocumentAddress>,
}

/// A seed or typed link that could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedDocument {
    /// Referring document, omitted for an initial selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<DocumentAddress>,
    /// Original logical selector or link target.
    pub selector: DocumentSelector,
    /// Stable, concise resolution diagnostic.
    pub reason: String,
}

/// Logical graph produced before applying a projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDocumentScope {
    /// Original normalized scope request.
    pub query: DocumentScope,
    /// Distinct documents in deterministic breadth-first order.
    pub documents: Vec<ScopedDocument>,
    /// Successfully resolved typed edges in source order.
    pub edges: Vec<DocumentEdge>,
    /// Resolved edges whose target documents were excluded by the document
    /// budget. Empty unless [`Self::truncated`] is true.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frontier: Vec<DocumentEdge>,
    /// Seeds and edges that could not resolve to a readable document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<UnresolvedDocument>,
    /// True when the document budget stopped traversal before its frontier emptied.
    pub truncated: bool,
}

/// One document's search hits inside a globally paginated scope result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopedSearchDocument {
    /// Stable logical document identity.
    pub address: DocumentAddress,
    /// Distance retained from the resolved scope.
    pub depth: u16,
    /// Matching line groups retained for this global page together with their
    /// document-local coordinate contract.
    pub search: QuerySearch,
}

/// Globally paginated search over a resolved document scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSearch {
    /// Normalized search configuration.
    pub query: SearchQuery,
    /// Matching line groups across all documents before pagination.
    pub total: u32,
    /// Matching line groups present in this response.
    pub returned: u32,
    /// Applied global zero-based offset.
    pub offset: u32,
    /// Whether additional matching line groups remain.
    pub truncated: bool,
    /// Global offset for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    /// Non-empty document groups in scope order.
    pub documents: Vec<ScopedSearchDocument>,
}

/// One successful semantic-entry selection in a document scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopedExplanation {
    /// Stable logical document identity.
    pub address: DocumentAddress,
    /// Distance retained from the resolved scope.
    pub depth: u16,
    /// Complete selected semantic entry or ambiguity candidates.
    pub excerpt: QueryExcerpt,
}

/// One per-document projection failure that does not invalidate other results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopedQueryFailure {
    /// Stable logical document identity.
    pub address: DocumentAddress,
    /// Concise projection diagnostic.
    pub reason: String,
}

/// Projection result carried by a scope-query response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ScopeQueryResult {
    /// Semantic entries found across the scope.
    Explain {
        /// Requested entry selector.
        entry: String,
        /// Documents with one or more exact candidates.
        matches: Vec<ScopedExplanation>,
        /// Ambiguity or projection failures, excluding ordinary misses.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        failures: Vec<ScopedQueryFailure>,
    },
    /// Globally paginated text search.
    Search {
        /// Search result grouped by document.
        search: ScopeSearch,
    },
}

/// Complete bounded multi-document response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:scope-query:v0.8"))]
pub struct ScopeQueryResponse {
    /// Exact response schema discriminator.
    pub schema: ScopeQuerySchema,
    /// Resolved logical graph, including missing links and truncation.
    pub scope: ResolvedDocumentScope,
    /// Requested projection over that graph.
    pub result: ScopeQueryResult,
}
