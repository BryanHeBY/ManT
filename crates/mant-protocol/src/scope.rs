//! Stable contracts for bounded queries over a linked set of documents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    DocumentAddress, SearchCase, SearchContentProjection, SearchMatch, SearchQuery, SearchRender,
    SearchScope, SearchSyntax, SourceContext, default_search_limit,
};

/// Maximum number of initial documents accepted by the native scope contract.
pub const MAX_SCOPE_DOCUMENTS: usize = 16;
/// Default maximum number of link edges followed from an initial document.
pub const DEFAULT_SCOPE_DEPTH: u16 = 8;
/// Hard maximum number of link edges accepted by the native scope contract.
pub const MAX_SCOPE_DEPTH: u16 = 32;
/// Default maximum number of distinct documents in one resolved scope.
pub const DEFAULT_SCOPE_DOCUMENT_LIMIT: u32 = 64;
/// Hard maximum number of distinct documents in one resolved scope.
pub const MAX_SCOPE_DOCUMENT_LIMIT: u32 = 256;
/// Maximum aggregate normalized-document payload retained by one scope.
///
/// Scope resolution keeps each parsed document in memory so later search,
/// explanation, and interactive navigation observe one consistent graph. This
/// independent guard prevents a small number of individually valid documents
/// from creating an unbounded aggregate allocation.
pub const MAX_SCOPE_CONTENT_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum Unicode scalar length of one logical document selector.
pub const MAX_DOCUMENT_SELECTOR_CHARS: usize = 1024;
/// Maximum Unicode scalar length of one semantic-entry selector.
pub const MAX_SEMANTIC_ENTRY_CHARS: usize = 512;
/// Maximum Unicode scalar length of one search pattern.
pub const MAX_SEARCH_PATTERN_CHARS: usize = 4096;
/// Maximum Unicode scalar length of one configured Markdown source selector.
pub const MAX_SOURCE_SELECTOR_CHARS: usize = 128;
/// Maximum Unicode scalar length of one native manual section selector.
pub const MAX_MANUAL_SECTION_CHARS: usize = 32;

/// One violated runtime constraint shared by scope-query request adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeTextError {
    /// The value was empty after trimming surrounding whitespace.
    Empty,
    /// The value contained a terminal or structural control character.
    ControlCharacter,
    /// The Unicode scalar length exceeded the declared maximum.
    TooLong {
        /// Inclusive maximum accepted Unicode scalar length.
        maximum: usize,
    },
}

/// Validate one bounded logical selector at the native request boundary.
///
/// JSON Schema advertises the same limits, but native `--request-json` callers
/// do not pass through a schema validator, so the runtime contract must check
/// them independently.
///
/// # Errors
///
/// Returns the precise empty, control-character, or scalar-length violation.
pub fn validate_scope_text(value: &str, maximum: usize) -> Result<(), ScopeTextError> {
    if value.trim().is_empty() {
        return Err(ScopeTextError::Empty);
    }
    if value.chars().any(char::is_control) {
        return Err(ScopeTextError::ControlCharacter);
    }
    if value.chars().count() > maximum {
        return Err(ScopeTextError::TooLong { maximum });
    }
    Ok(())
}

/// One logical document selector before catalog resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentSelector {
    /// Unqualified name or complete catalog path.
    #[schemars(length(min = 1, max = MAX_DOCUMENT_SELECTOR_CHARS))]
    pub selector: String,
    /// Optional configured Markdown source for an unqualified selector.
    #[schemars(length(min = 1, max = MAX_SOURCE_SELECTOR_CHARS))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Optional native manual category for an unqualified selector.
    #[schemars(length(min = 1, max = MAX_MANUAL_SECTION_CHARS))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
}

/// Bounded traversal applied after resolving the initial documents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentTraversal {
    /// Follow typed links to other registered documents.
    #[serde(default)]
    pub follow_links: bool,
    /// Optional maximum number of link edges from an initial document.
    ///
    /// Omission selects [`DEFAULT_SCOPE_DEPTH`] when [`Self::follow_links`] is
    /// true. The field is invalid when link traversal is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(max = 32))]
    pub max_depth: Option<u16>,
    /// Optional maximum number of distinct documents, including roots.
    ///
    /// Omission selects [`DEFAULT_SCOPE_DOCUMENT_LIMIT`] when
    /// [`Self::follow_links`] is true. The field is invalid when link traversal
    /// is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 256))]
    pub max_documents: Option<u32>,
}

impl DocumentTraversal {
    /// Effective edge limit after applying the native default.
    #[must_use]
    pub fn effective_max_depth(self) -> u16 {
        self.max_depth.unwrap_or(DEFAULT_SCOPE_DEPTH)
    }

    /// Effective document budget after applying the native default.
    #[must_use]
    pub fn effective_max_documents(self) -> u32 {
        self.max_documents.unwrap_or(DEFAULT_SCOPE_DOCUMENT_LIMIT)
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
    /// Version 0.12 of the pre-stable scope-query request.
    #[serde(rename = "mant.scope-request/v0.12")]
    V0Dot12,
}

impl ScopeRequestSchema {
    /// Serialized identifier of the current request contract.
    pub const ID: &'static str = "mant.scope-request/v0.12";
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
    /// Collect independent evidence across loaded documents.
    Explain {
        /// Documented name, full form, exact entry ID/path, or bounded literal.
        #[schemars(length(min = 1, max = MAX_SEMANTIC_ENTRY_CHARS))]
        entry: String,
        /// Global result pagination and copied-content budget.
        #[serde(default)]
        options: crate::ExplanationOptions,
    },
    /// Search visible or generated-Markdown text over the complete scope.
    Search {
        /// Literal or regular-expression search pattern.
        #[schemars(length(min = 1, max = MAX_SEARCH_PATTERN_CHARS))]
        pattern: String,
        /// Pattern language.
        #[serde(default)]
        syntax: SearchSyntax,
        /// Case-matching policy.
        #[serde(default)]
        case: SearchCase,
        /// Search visible text or generated Markdown bytes.
        #[serde(default)]
        scope: SearchScope,
        /// Require Unicode-aware word boundaries.
        #[serde(default)]
        word: bool,
        /// Neighboring rendered lines included around a match.
        #[serde(default)]
        #[schemars(range(max = 100))]
        context_lines: u16,
        /// Global maximum number of complete occurrences returned.
        #[serde(default = "default_search_limit")]
        #[schemars(range(min = 1, max = 10000))]
        limit: u32,
        /// Global number of complete occurrences skipped.
        #[serde(default)]
        offset: u32,
    },
}

/// Native request for a bounded multi-document query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:scope-request:v0.12"))]
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
    /// Version 0.12 of the pre-stable scope-query result.
    #[serde(rename = "mant.scope-query/v0.12")]
    V0Dot12,
}

impl ScopeQuerySchema {
    /// Serialized identifier of the current result contract.
    pub const ID: &'static str = "mant.scope-query/v0.12";
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

/// Traversal bound that excluded an outbound logical link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TraversalLimit {
    /// The maximum number of followed link edges was reached.
    MaxDepth,
    /// The maximum number of distinct loaded documents was reached.
    MaxDocuments,
    /// Retaining another normalized document would exceed the aggregate
    /// semantic-content budget.
    MaxContentBytes,
}

/// One typed outbound link excluded by a traversal bound.
///
/// A frontier retains the logical selector rather than requiring a resolved
/// address: resolving a target may itself exceed the requested bound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentFrontier {
    /// Address containing the excluded link.
    pub from: DocumentAddress,
    /// Logical target that would be resolved if traversal continued.
    pub target: DocumentSelector,
    /// Semantic link family.
    pub kind: DocumentEdgeKind,
    /// Bound that prevented traversal of this link.
    pub limit: TraversalLimit,
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
    /// Typed outbound links excluded by depth, document, or content limits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frontier: Vec<DocumentFrontier>,
    /// Seeds and edges that could not resolve to a readable document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<UnresolvedDocument>,
    /// Documents whose outbound reference scan was incomplete. Missing edges
    /// are unknown, not proof that these documents have no further links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_limits: Vec<ScopeReferenceLimit>,
}

/// A bounded outbound scan that could not establish the complete edge set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeReferenceLimit {
    /// Loaded logical source document, never a host filesystem path.
    pub document: DocumentAddress,
    /// Shared traversal accounting and first stop condition.
    pub coverage: crate::ReferenceCoverage,
    /// Distinct reference retention cap, when it caused the stop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_limit: Option<crate::ReferencePageLimit>,
}

/// One document's logical hits inside a globally paginated scope result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopedSearchDocument {
    /// Stable logical document identity.
    pub address: DocumentAddress,
    /// Distance retained from the resolved scope.
    pub depth: u16,
    /// Source table resolving authored hit coordinates, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_context: Option<SourceContext>,
    /// Closed response-local visible units for retained occurrences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_projection: Option<SearchContentProjection>,
    /// Coordinate space for this document's hits.
    pub render: SearchRender,
    /// Complete occurrences retained from the globally paginated result set.
    /// Their ordinals are global across all documents in the scope.
    pub matches: Vec<SearchMatch>,
}

#[derive(Deserialize)]
#[serde(
    remote = "ScopedSearchDocument",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ScopedSearchDocumentWire {
    pub address: DocumentAddress,
    pub depth: u16,
    pub source_context: Option<SourceContext>,
    #[serde(default)]
    pub content_projection: Option<SearchContentProjection>,
    pub render: SearchRender,
    pub matches: Vec<SearchMatch>,
}

impl<'de> Deserialize<'de> for ScopedSearchDocument {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = ScopedSearchDocumentWire::deserialize(deserializer)?;
        crate::document::validate_optional_source_spans(
            value.source_context.as_ref(),
            value.matches.iter().filter_map(|hit| hit.node_source),
        )
        .map_err(serde::de::Error::custom)?;
        if let Some(projection) = &value.content_projection {
            projection.validate().map_err(serde::de::Error::custom)?;
        }
        Ok(value)
    }
}

/// Coverage of one scanned scope document, including zero-hit documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedSearchCoverage {
    /// Stable logical document identity.
    pub address: DocumentAddress,
    /// Minimum distance from any initial document.
    pub depth: u16,
    /// Source table closing diagnostic spans, absent for TLDR-only input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_context: Option<SourceContext>,
    /// Whether all semantic associations were verified.
    pub semantics_complete: bool,
    /// Exact count of diagnostics replaced by one summary.
    pub coverage_details_omitted: u32,
    /// Bounded producer and validation findings.
    pub diagnostics: Vec<mant_ir::Diagnostic>,
}

#[derive(Deserialize)]
#[serde(
    remote = "ScopedSearchCoverage",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ScopedSearchCoverageWire {
    address: DocumentAddress,
    depth: u16,
    source_context: Option<SourceContext>,
    semantics_complete: bool,
    coverage_details_omitted: u32,
    diagnostics: Vec<mant_ir::Diagnostic>,
}

impl<'de> Deserialize<'de> for ScopedSearchCoverage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = ScopedSearchCoverageWire::deserialize(deserializer)?;
        crate::document::validate_optional_diagnostic_sources(
            value.source_context.as_ref(),
            &value.diagnostics,
        )
        .map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

/// Exact schema marker for a scoped search response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ScopeSearchSchema {
    /// Version 0.12 of the pre-stable scoped search contract.
    #[serde(rename = "mant.scope-search/v0.12")]
    V0Dot12,
}

/// Globally paginated occurrence search over a resolved document scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeSearch {
    /// Exact scoped-search schema discriminator.
    pub schema: ScopeSearchSchema,
    /// Normalized search configuration.
    pub query: SearchQuery,
    /// Complete occurrences across all documents before pagination.
    pub total: u32,
    /// Complete occurrences present in this response.
    pub returned: u32,
    /// Applied global zero-based offset.
    pub offset: u32,
    /// Whether additional complete occurrences remain.
    pub truncated: bool,
    /// Global offset for the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    /// Conjunction of every scanned document's semantic completeness.
    pub semantics_complete: bool,
    /// Per-document coverage, including zero-hit and paginated-away documents.
    pub coverage_by_document: Vec<ScopedSearchCoverage>,
    /// Non-empty document groups in scope order.
    pub documents: Vec<ScopedSearchDocument>,
}

#[derive(Deserialize)]
#[serde(remote = "ScopeSearch", rename_all = "camelCase", deny_unknown_fields)]
struct ScopeSearchWire {
    schema: ScopeSearchSchema,
    query: SearchQuery,
    total: u32,
    returned: u32,
    offset: u32,
    truncated: bool,
    next_offset: Option<u32>,
    semantics_complete: bool,
    coverage_by_document: Vec<ScopedSearchCoverage>,
    documents: Vec<ScopedSearchDocument>,
}

impl<'de> Deserialize<'de> for ScopeSearch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = ScopeSearchWire::deserialize(deserializer)?;
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl ScopeSearch {
    /// Validate occurrence pagination and all returned-document coverage links.
    ///
    /// # Errors
    /// Returns malformed page counts, duplicate addresses or missing coverage.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.query.limit == 0
            || self.query.limit > 10_000
            || self.query.context_lines > 100
            || self.returned > self.query.limit
        {
            return Err("scope search request or page exceeds protocol bounds");
        }
        if self.coverage_by_document.len() > MAX_SCOPE_DOCUMENT_LIMIT as usize {
            return Err("scope search coverage exceeds document bound");
        }
        let mut retained = Vec::new();
        let mut last_group_index = None;
        let mut projection_bytes = 0usize;
        let mut metadata_bytes = ScopeSearchMetadataCounter::default();
        for group in &self.documents {
            if group.matches.is_empty() {
                return Err("scope search document group has no retained occurrences");
            }
            retained.extend(group.matches.iter().cloned());
            let group_index = self
                .coverage_by_document
                .iter()
                .position(|coverage| {
                    coverage.address == group.address && coverage.depth == group.depth
                })
                .ok_or("scope search document group has no coverage entry")?;
            if last_group_index.is_some_and(|last| group_index <= last) {
                return Err("scope search document groups are not in scope order");
            }
            if group.source_context != self.coverage_by_document[group_index].source_context {
                return Err("scope search hit group and coverage source tables disagree");
            }
            if let Some(context) = &group.source_context {
                metadata_bytes.add(context)?;
            }
            crate::document::validate_optional_source_spans(
                group.source_context.as_ref(),
                group.matches.iter().filter_map(|hit| hit.node_source),
            )
            .map_err(|_| "scope search hit source is not closed by its source table")?;
            last_group_index = Some(group_index);
            crate::search::validate_search_content(
                self.query.scope,
                &group.render,
                group.content_projection.as_ref(),
                &group.matches,
            )?;
            if let Some(projection) = &group.content_projection {
                add_scope_projection_bytes(projection, &mut projection_bytes)?;
            }
        }
        crate::search::validate_search_page(
            self.offset,
            self.returned,
            self.total,
            self.truncated,
            self.next_offset,
            &retained,
        )?;
        crate::search::validate_search_presentation(&retained)?;
        if self.coverage_by_document.is_empty() && !self.documents.is_empty() {
            return Err("scope search has no scanned-document coverage");
        }
        for (index, coverage) in self.coverage_by_document.iter().enumerate() {
            metadata_bytes.add(coverage)?;
            if self.coverage_by_document[..index]
                .iter()
                .any(|prior| prior.address == coverage.address)
            {
                return Err("scope search duplicates a coverage document");
            }
            crate::search::validate_search_coverage(
                coverage.semantics_complete,
                coverage.coverage_details_omitted,
                &coverage.diagnostics,
            )?;
            crate::document::validate_optional_diagnostic_sources(
                coverage.source_context.as_ref(),
                &coverage.diagnostics,
            )
            .map_err(|_| "scope search diagnostic source is not closed by its source table")?;
        }
        if self.semantics_complete
            != self
                .coverage_by_document
                .iter()
                .all(|entry| entry.semantics_complete)
        {
            return Err("scope search completeness disagrees with scanned documents");
        }
        Ok(())
    }
}

#[derive(Default)]
struct ScopeSearchMetadataCounter {
    bytes: usize,
}

impl ScopeSearchMetadataCounter {
    fn add<T: Serialize>(&mut self, value: &T) -> Result<(), &'static str> {
        serde_json::to_writer(self, value)
            .map_err(|_| "scope search metadata exceeds global byte budget")
    }
}

impl std::io::Write for ScopeSearchMetadataCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= crate::MAX_SEARCH_PRESENTATION_BYTES)
            .ok_or_else(|| std::io::Error::other("scope search metadata byte budget"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod metadata_budget_tests {
    use super::ScopeSearchMetadataCounter;

    #[test]
    fn serialized_diagnostics_and_source_tables_share_one_response_limit() {
        let mut counter = ScopeSearchMetadataCounter {
            bytes: crate::MAX_SEARCH_PRESENTATION_BYTES - 4,
        };
        assert!(counter.add(&"long diagnostic").is_err());
        assert!(counter.bytes <= crate::MAX_SEARCH_PRESENTATION_BYTES);
    }
}

fn add_scope_projection_bytes(
    projection: &SearchContentProjection,
    total: &mut usize,
) -> Result<(), &'static str> {
    for fragment in &projection.fragments {
        *total = total
            .checked_add(fragment.text.len())
            .ok_or("scope search projection byte count overflows")?;
    }
    for unit in &projection.units {
        for join in &unit.joins {
            if let crate::SearchTextJoin::AuthoredSeparator { text }
            | crate::SearchTextJoin::GeneratedSeparator { text }
            | crate::SearchTextJoin::RenderSeparator { text } = join
            {
                *total = total
                    .checked_add(text.len())
                    .ok_or("scope search projection byte count overflows")?;
            }
        }
    }
    if *total > crate::search::MAX_SEARCH_PROJECTION_BYTES {
        return Err("scope search projection exceeds global byte budget");
    }
    Ok(())
}

/// One readable document's contribution to the evidence result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedExplanation {
    /// Declaration context pool for evidence with this document index.
    pub supports: Vec<crate::ExplanationSupport>,
    /// Closed document-local content store shared by supports and global evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_projection: Option<mant_ir::ContentProjection>,
    /// Stable logical document identity.
    pub address: DocumentAddress,
    /// Distance retained from the resolved scope.
    pub depth: u16,
    /// Selected source label, independent of catalog identity.
    pub label: String,
    /// Source table resolving authored coordinates, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_context: Option<SourceContext>,
    /// Parser and process provenance when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<crate::Producer>,
    /// Recoverable producer and shared invariant findings.
    pub diagnostics: Vec<mant_ir::Diagnostic>,
    /// Semantic validation, not evidence recall.
    pub semantics_complete: bool,
    /// Normal local evidence/no-evidence outcome before global pagination.
    pub outcome: crate::ExplanationOutcome,
    /// Local collected owners.
    pub total: u32,
    /// Local owners selected on the one global page.
    pub returned: u32,
    /// Local contributions to each global evidence category.
    pub counts: crate::EvidenceCounts,
    /// Local collection and copy truncation.
    pub truncation: crate::ExplanationTruncation,
}

#[derive(Deserialize)]
#[serde(
    remote = "ScopedExplanation",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ScopedExplanationWire {
    pub supports: Vec<crate::ExplanationSupport>,
    #[serde(default)]
    pub content_projection: Option<mant_ir::ContentProjection>,
    pub address: DocumentAddress,
    pub depth: u16,
    pub label: String,
    pub source_context: Option<SourceContext>,
    pub producer: Option<crate::Producer>,
    pub diagnostics: Vec<mant_ir::Diagnostic>,
    pub semantics_complete: bool,
    pub outcome: crate::ExplanationOutcome,
    pub total: u32,
    pub returned: u32,
    pub counts: crate::EvidenceCounts,
    pub truncation: crate::ExplanationTruncation,
}

impl<'de> Deserialize<'de> for ScopedExplanation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = ScopedExplanationWire::deserialize(deserializer)?;
        crate::explanation::validate_explanation_sources(
            value.content_projection.as_ref(),
            value.source_context.as_ref(),
            &value.diagnostics,
            &value.supports,
            std::iter::empty(),
        )
        .map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

/// One global evidence record with an explicit source-report reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedExplanationEvidence {
    /// Zero-based index into this explanation's documents, not the scope graph.
    pub document_index: usize,
    /// The unique record; bodies are never duplicated in the document reports.
    pub evidence: crate::ExplanationEvidence,
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
        /// Globally bounded evidence with coverage separate from the scope graph.
        explanation: ScopeExplanation,
    },
    /// Globally paginated text search.
    Search {
        /// Search result grouped by document.
        search: ScopeSearch,
    },
}

/// Global evidence page over a resolved scope. Source loading failures/frontier
/// remain in `ScopeQueryResponse.scope`, independently of this normal outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeExplanation {
    /// Normative global category/BFS/source ordering.
    pub order: crate::EvidenceOrder,
    /// Global per-class totals and page counts.
    pub counts: crate::EvidenceCounts,
    /// Original global pagination/content request.
    pub query: crate::ExplanationQuery,
    /// Evidence/no-evidence before pagination, never uniqueness or recall proof.
    pub outcome: crate::ExplanationOutcome,
    /// Sum of collected owner counts (a lower bound when collection is truncated).
    pub total: u32,
    /// Owners present on this global page.
    pub returned: u32,
    /// Next global result offset when more collected evidence remains.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    /// Independent bounds, combined across readable sources.
    pub truncation: crate::ExplanationTruncation,
    /// Readable documents, including normal zero-evidence contributions.
    pub documents: Vec<ScopedExplanation>,
    /// The only materialized evidence list, in global classification order.
    pub evidence: Vec<ScopedExplanationEvidence>,
    /// Unexpected unreadable loaded content, never normal multiple/zero hits.
    pub failures: Vec<ScopedQueryFailure>,
}

/// Complete bounded multi-document response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:scope-query:v0.12"))]
pub struct ScopeQueryResponse {
    /// Exact response schema discriminator.
    pub schema: ScopeQuerySchema,
    /// Resolved logical graph, including missing links and truncation.
    pub scope: ResolvedDocumentScope,
    /// Requested projection over that graph.
    pub result: ScopeQueryResult,
}

#[derive(Deserialize)]
#[serde(
    remote = "ScopeQueryResponse",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ScopeQueryResponseWire {
    schema: ScopeQuerySchema,
    scope: ResolvedDocumentScope,
    result: ScopeQueryResult,
}

impl<'de> Deserialize<'de> for ScopeQueryResponse {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = ScopeQueryResponseWire::deserialize(deserializer)?;
        if let ScopeQueryResult::Search { search } = &value.result
            && (search.coverage_by_document.len() != value.scope.documents.len()
                || !search
                    .coverage_by_document
                    .iter()
                    .zip(&value.scope.documents)
                    .all(|(coverage, scoped)| {
                        coverage.address == scoped.address && coverage.depth == scoped.depth
                    }))
        {
            return Err(serde::de::Error::custom(
                "scope search coverage must include every scanned document in scope order",
            ));
        }
        Ok(value)
    }
}

// Remote derive keeps the public schema closed while validating cross-field
// references after structural decoding, without a JSON intermediate tree.
#[derive(Deserialize)]
#[serde(
    remote = "ScopeExplanation",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct ScopeExplanationWire {
    pub order: crate::EvidenceOrder,
    pub counts: crate::EvidenceCounts,
    pub query: crate::ExplanationQuery,
    pub outcome: crate::ExplanationOutcome,
    pub total: u32,
    pub returned: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    pub truncation: crate::ExplanationTruncation,
    pub documents: Vec<ScopedExplanation>,
    pub evidence: Vec<ScopedExplanationEvidence>,
    pub failures: Vec<ScopedQueryFailure>,
}
impl<'de> Deserialize<'de> for ScopeExplanation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = ScopeExplanationWire::deserialize(deserializer)?;
        value
            .validate_references()
            .map_err(serde::de::Error::custom)?;
        for (index, document) in value.documents.iter().enumerate() {
            crate::explanation::validate_explanation_sources(
                document.content_projection.as_ref(),
                document.source_context.as_ref(),
                &document.diagnostics,
                &document.supports,
                value
                    .evidence
                    .iter()
                    .filter(|evidence| evidence.document_index == index)
                    .map(|evidence| &evidence.evidence),
            )
            .map_err(serde::de::Error::custom)?;
        }
        Ok(value)
    }
}
