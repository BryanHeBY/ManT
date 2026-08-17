//! Closed, compact input schemas exposed by the agent-facing MCP tools.

use mant_protocol::{
    CatalogDocumentKind, CatalogQuery, NodeSelector, OutlineDetail, QueryInput, QueryRequest,
    QueryView, SearchCase, SearchSyntax,
};
use schemars::JsonSchema;
use serde::Deserialize;

/// Maximum accepted logical document selector length.
pub(super) const MAX_DOCUMENT_BYTES: usize = 1024;
/// Maximum accepted continuation token length.
pub(super) const MAX_CURSOR_BYTES: usize = 256;
/// Maximum selectors accepted by one focused read.
pub(super) const MAX_SELECTORS: usize = 16;
/// Fixed catalog page size for agent discovery.
pub(super) const FIND_PAGE_SIZE: u32 = 50;
/// Default match-line page size for in-document search.
pub(super) const DEFAULT_SEARCH_PAGE_SIZE: u32 = 20;
/// Maximum match-line page size exposed to an MCP client.
pub(super) const MAX_SEARCH_PAGE_SIZE: u32 = 100;

/// Discover logical document identities in the local catalog.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FindParams {
    /// Optional case-insensitive literal matched against names and catalog paths.
    #[schemars(length(max = 1024))]
    pub(super) query: Option<String>,
    /// Restrict results to registered Markdown or native manuals.
    pub(super) kind: Option<CatalogDocumentKind>,
    /// Restrict Markdown results to one configured source.
    #[schemars(length(min = 1, max = 128))]
    pub(super) source: Option<String>,
    /// Restrict native manuals to one exact manual section.
    #[schemars(length(min = 1, max = 32))]
    pub(super) manual_section: Option<String>,
    /// Opaque continuation token returned by an earlier identical call.
    #[schemars(length(min = 1, max = 256))]
    pub(super) cursor: Option<String>,
}

/// Parameters shared by focused document tools.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OutlineParams {
    /// Unqualified name or canonical catalog path returned by `mant_find`.
    #[schemars(length(min = 1, max = 1024))]
    pub(super) document: String,
    /// Include sections only (the default), or semantic entries as well.
    pub(super) detail: Option<OutlineDetail>,
    /// Opaque continuation token returned by an earlier identical call.
    #[schemars(length(min = 1, max = 256))]
    pub(super) cursor: Option<String>,
}

/// Retrieve one or more nodes selected from a document outline.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReadParams {
    /// Unqualified name or canonical catalog path returned by `mant_find`.
    #[schemars(length(min = 1, max = 1024))]
    pub(super) document: String,
    /// Outline paths, stable IDs, or semantic aliases.
    #[schemars(length(min = 1, max = 16))]
    pub(super) selectors: Vec<NodeSelector>,
    /// Opaque continuation token returned by an earlier identical call.
    #[schemars(length(min = 1, max = 256))]
    pub(super) cursor: Option<String>,
}

/// Resolve one semantic command, option, variable, or environment entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ExplainParams {
    /// Unqualified name or canonical catalog path returned by `mant_find`.
    #[schemars(length(min = 1, max = 1024))]
    pub(super) document: String,
    /// Exact alias, outline path, or stable ID of the entry.
    #[schemars(length(min = 1, max = 512))]
    pub(super) entry: String,
    /// Opaque continuation token returned by an earlier identical call.
    #[schemars(length(min = 1, max = 256))]
    pub(super) cursor: Option<String>,
}

/// Search visible document text with bounded result pages.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SearchParams {
    /// Unqualified name or canonical catalog path returned by `mant_find`.
    #[schemars(length(min = 1, max = 1024))]
    pub(super) document: String,
    /// Literal text or a regular expression, depending on `syntax`.
    #[schemars(length(min = 1, max = 4096))]
    pub(super) pattern: String,
    /// Interpret `pattern` literally (the default) or as a regular expression.
    pub(super) syntax: Option<SearchSyntax>,
    /// Case-folding policy. The default is `insensitive`.
    pub(super) case: Option<SearchCase>,
    /// Restrict matches to Unicode-aware word boundaries.
    #[serde(default)]
    pub(super) word: bool,
    /// Visible lines of context before and after a match, from zero through five.
    #[serde(default)]
    #[schemars(range(max = 5))]
    pub(super) context_lines: u16,
    /// Maximum matching line groups returned before a continuation cursor.
    #[schemars(range(min = 1, max = 100))]
    pub(super) limit: Option<u32>,
    /// Opaque continuation token returned by an earlier identical call.
    #[schemars(length(min = 1, max = 256))]
    pub(super) cursor: Option<String>,
}

pub(super) fn validate_find(mut parameters: FindParams) -> Result<FindParams, String> {
    parameters.query = parameters
        .query
        .map(|query| query.trim().to_owned())
        .filter(|query| !query.is_empty());
    parameters.source = optional_non_empty(parameters.source, "source")?;
    parameters.manual_section = optional_non_empty(parameters.manual_section, "manualSection")?;
    validate_cursor(parameters.cursor.as_deref())?;
    if parameters.source.is_some() && parameters.manual_section.is_some() {
        return Err("source and manualSection cannot be combined".to_owned());
    }
    Ok(parameters)
}

pub(super) fn validate_document(value: &str) -> Result<String, String> {
    bounded_non_empty(value, "document", MAX_DOCUMENT_BYTES)
}

pub(super) fn validate_cursor(value: Option<&str>) -> Result<(), String> {
    if value.is_some_and(|value| value.is_empty() || value.len() > MAX_CURSOR_BYTES) {
        return Err(format!(
            "cursor must contain between 1 and {MAX_CURSOR_BYTES} bytes"
        ));
    }
    Ok(())
}

pub(super) fn validate_selectors(selectors: &[NodeSelector]) -> Result<(), String> {
    if selectors.is_empty() || selectors.len() > MAX_SELECTORS {
        return Err(format!(
            "selectors must contain between 1 and {MAX_SELECTORS} values"
        ));
    }
    for selector in selectors {
        bounded_non_empty(selector, "selector", 512)?;
    }
    Ok(())
}

pub(super) fn validate_entry(value: &str) -> Result<String, String> {
    bounded_non_empty(value, "entry", 512)
}

pub(super) fn validate_pattern(value: &str) -> Result<String, String> {
    bounded_non_empty(value, "pattern", 4096)
}

pub(super) fn validate_context_lines(value: u16) -> Result<(), String> {
    if value > 5 {
        return Err("contextLines must be between 0 and 5".to_owned());
    }
    Ok(())
}

pub(super) fn validate_search_limit(value: Option<u32>) -> Result<u32, String> {
    let value = value.unwrap_or(DEFAULT_SEARCH_PAGE_SIZE);
    if !(1..=MAX_SEARCH_PAGE_SIZE).contains(&value) {
        return Err(format!(
            "limit must be between 1 and {MAX_SEARCH_PAGE_SIZE}"
        ));
    }
    Ok(value)
}

pub(super) fn catalog_query(parameters: &FindParams, offset: u32) -> CatalogQuery {
    CatalogQuery {
        pattern: parameters.query.clone(),
        syntax: SearchSyntax::Literal,
        case: SearchCase::Insensitive,
        kind: parameters.kind,
        source: parameters.source.clone(),
        manual_section: parameters.manual_section.clone(),
        limit: FIND_PAGE_SIZE,
        offset,
    }
}

pub(super) fn request_for(document: String, view: QueryView) -> QueryRequest {
    QueryRequest {
        schema: mant_protocol::RequestSchema::V0Dot8,
        input: QueryInput::Document {
            selector: document,
            source: None,
            manual_section: None,
        },
        view,
    }
}

fn optional_non_empty(value: Option<String>, field: &str) -> Result<Option<String>, String> {
    value
        .map(|value| bounded_non_empty(&value, field, 128))
        .transpose()
}

fn bounded_non_empty(value: &str, field: &str, max: usize) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > max {
        return Err(format!("{field} must not exceed {max} bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(value.to_owned())
}
