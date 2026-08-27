//! Closed, compact input schemas exposed by the agent-facing MCP tools.

use mant_protocol::{
    CatalogDocumentKind, CatalogQuery, DocumentScope, DocumentSelector, DocumentTraversal,
    EntryProjection, NodeSelector, QueryInput, QueryRequest, QueryView, SearchCase, SearchSyntax,
};
use schemars::JsonSchema;
use serde::{Deserialize, de::DeserializeOwned};

/// Maximum accepted logical document selector length.
pub(super) const MAX_DOCUMENT_BYTES: usize = mant_protocol::MAX_DOCUMENT_SELECTOR_BYTES;
/// Maximum selectors accepted by one focused read.
pub(super) const MAX_SELECTORS: usize = 16;
/// Default number of catalog rows materialized by discovery.
pub(super) const DEFAULT_FIND_RESULTS: u32 = 50;
/// Default number of matching line groups materialized by search.
pub(super) const DEFAULT_SEARCH_MATCHES: u32 = 20;
/// Maximum catalog rows materialized by one discovery call.
pub(super) const MAX_FIND_RESULTS: u32 = 10_000;
/// Maximum matching line groups materialized by one agent search.
///
/// Search groups retain previews, occurrences, and optional context, unlike a
/// catalog row. Keep this separate from discovery so a compact character page
/// cannot cause an unboundedly large in-memory search projection.
pub(super) const MAX_SEARCH_MATCHES: u32 = 100;
/// Default Unicode scalar values returned by one successful tool call.
pub(super) const DEFAULT_PAGE_CHARS: u32 = 16 * 1024;
/// Maximum Unicode scalar values returned by one successful tool call.
pub(super) const MAX_PAGE_CHARS: u32 = 32 * 1024;
pub(super) const MAX_FIND_QUERY_BYTES: usize = 1024;
const MAX_SOURCE_BYTES: usize = 128;
pub(super) const MAX_MANUAL_SECTION_BYTES: usize = 32;
const MAX_SELECTOR_BYTES: usize = 512;
const MAX_PATTERN_BYTES: usize = 4096;

/// Discover logical document identities in the local catalog.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FindParams {
    /// Optional name or catalog-path pattern, bounded to 1024 UTF-8 bytes at runtime.
    pub(super) query: Option<String>,
    /// Interpret `query` literally (the default) or as a regular expression.
    pub(super) syntax: Option<SearchSyntax>,
    /// Case-folding policy. The default is `insensitive`.
    pub(super) case: Option<SearchCase>,
    /// Restrict results to registered Markdown or native manuals.
    pub(super) kind: Option<CatalogDocumentKind>,
    /// Restrict Markdown results to one configured source.
    #[schemars(length(min = 1))]
    pub(super) source: Option<String>,
    /// Restrict native manuals to one exact manual section.
    #[schemars(length(min = 1))]
    pub(super) manual_section: Option<String>,
    /// Maximum matching catalog rows included in the canonical result text.
    #[schemars(range(min = 1, max = 10_000))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_results: Option<u32>,
    /// Skip this many matching catalog rows before materialization.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) offset: u32,
    /// Zero-based Unicode scalar offset into the canonical result text.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) start_char: u32,
    /// Maximum Unicode scalar values returned from `startChar`.
    #[schemars(range(min = 1, max = 32_768))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_chars: Option<u32>,
}

/// Parameters shared by focused document tools.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OutlineParams {
    /// Unqualified name or canonical catalog path returned by `mant_find`.
    #[schemars(length(min = 1))]
    pub(super) document: String,
    /// Include no entries, compact summaries (the default), all entries, or selected kinds.
    /// Start compact, then expand a relevant returned root with all or selected kinds.
    pub(super) entries: Option<EntryProjection>,
    /// Returned section or entry path, stable ID, or unambiguous alias used as the tree root.
    /// Stable IDs are preferred for stateless follow-up calls.
    #[schemars(length(min = 1))]
    pub(super) root: Option<String>,
    /// Zero-based Unicode scalar offset into the canonical result text.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) start_char: u32,
    /// Maximum Unicode scalar values returned from `startChar`.
    #[schemars(range(min = 1, max = 32_768))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_chars: Option<u32>,
}

/// Retrieve one or more nodes selected from a document outline.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReadParams {
    /// Unqualified name or canonical catalog path returned by `mant_find`.
    #[schemars(length(min = 1))]
    pub(super) document: String,
    /// Outline paths, stable IDs, or semantic aliases.
    #[schemars(length(min = 1, max = 16))]
    #[serde(deserialize_with = "deserialize_selectors")]
    pub(super) selectors: Vec<NodeSelector>,
    /// Zero-based Unicode scalar offset into the canonical result text.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) start_char: u32,
    /// Maximum Unicode scalar values returned from `startChar`.
    #[schemars(range(min = 1, max = 32_768))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_chars: Option<u32>,
}

/// Resolve one semantic entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ExplainParams {
    /// One or more unqualified names or canonical IDs returned by `mant_find`.
    #[schemars(length(min = 1, max = 16))]
    #[serde(deserialize_with = "deserialize_documents")]
    pub(super) documents: Vec<String>,
    /// Follow typed links from the initial documents.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) follow_links: bool,
    /// Maximum followed-link distance; valid only with `followLinks`.
    #[schemars(range(max = 32))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_depth: Option<u16>,
    /// Maximum distinct documents including roots; valid only with `followLinks`.
    #[schemars(range(min = 1, max = 256))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_documents: Option<u32>,
    /// Exact alias, outline path, or stable ID of the entry.
    #[schemars(length(min = 1))]
    pub(super) entry: String,
    /// Zero-based Unicode scalar offset into the canonical result text.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) start_char: u32,
    /// Maximum Unicode scalar values returned from `startChar`.
    #[schemars(range(min = 1, max = 32_768))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_chars: Option<u32>,
}

/// Search visible document text with bounded result pages.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SearchParams {
    /// One or more unqualified names or canonical IDs returned by `mant_find`.
    #[schemars(length(min = 1, max = 16))]
    #[serde(deserialize_with = "deserialize_documents")]
    pub(super) documents: Vec<String>,
    /// Follow typed links from the initial documents.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) follow_links: bool,
    /// Maximum followed-link distance; valid only with `followLinks`.
    #[schemars(range(max = 32))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_depth: Option<u16>,
    /// Maximum distinct documents including roots; valid only with `followLinks`.
    #[schemars(range(min = 1, max = 256))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_documents: Option<u32>,
    /// Literal text or a regular expression, depending on `syntax`.
    #[schemars(length(min = 1))]
    pub(super) pattern: String,
    /// Interpret `pattern` literally (the default) or as a regular expression.
    pub(super) syntax: Option<SearchSyntax>,
    /// Case-folding policy. The default is `insensitive`.
    pub(super) case: Option<SearchCase>,
    /// Search visible text (the default) or generated `CommonMark` markup.
    pub(super) scope: Option<mant_protocol::SearchScope>,
    /// Restrict matches to Unicode-aware word boundaries.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) word: bool,
    /// Visible lines of context before and after a match, from zero through five.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    #[schemars(range(max = 5))]
    pub(super) context_lines: u16,
    /// Maximum matching line groups included in the canonical result text.
    #[schemars(range(min = 1, max = 100))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_matches: Option<u32>,
    /// Skip this many global matching-line groups before materialization.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) offset: u32,
    /// Zero-based Unicode scalar offset into the canonical result text.
    #[serde(default, deserialize_with = "deserialize_compat_scalar")]
    pub(super) start_char: u32,
    /// Maximum Unicode scalar values returned from `startChar`.
    #[schemars(range(min = 1, max = 32_768))]
    #[serde(default, deserialize_with = "deserialize_compat_optional_scalar")]
    pub(super) max_chars: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PageRequest {
    pub(super) start_char: u32,
    pub(super) max_chars: u32,
}

pub(super) struct ValidatedFindParams {
    pub(super) query: Option<String>,
    pub(super) syntax: SearchSyntax,
    pub(super) case: SearchCase,
    pub(super) kind: Option<CatalogDocumentKind>,
    pub(super) source: Option<String>,
    pub(super) manual_section: Option<String>,
    pub(super) max_results: u32,
    pub(super) offset: u32,
    pub(super) page: PageRequest,
}

pub(super) struct ValidatedOutlineParams {
    pub(super) document: String,
    pub(super) entries: EntryProjection,
    pub(super) root: Option<NodeSelector>,
    pub(super) page: PageRequest,
}

pub(super) struct ValidatedReadParams {
    pub(super) document: String,
    pub(super) selectors: Vec<NodeSelector>,
    pub(super) page: PageRequest,
}

pub(super) struct ValidatedExplainParams {
    pub(super) scope: DocumentScope,
    pub(super) entry: String,
    pub(super) page: PageRequest,
}

pub(super) struct ValidatedSearchParams {
    pub(super) documents: DocumentScope,
    pub(super) pattern: String,
    pub(super) syntax: SearchSyntax,
    pub(super) case: SearchCase,
    pub(super) scope: mant_protocol::SearchScope,
    pub(super) word: bool,
    pub(super) context_lines: u16,
    pub(super) max_matches: u32,
    pub(super) offset: u32,
    pub(super) page: PageRequest,
}

/// Preserve the canonical array schema while accepting collection spellings
/// that some MCP clients serialize as one string.
fn deserialize_documents<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_compat_list(deserializer, "documents")
}

/// Preserve the canonical array schema while accepting one selector or a
/// stringified selector array from an MCP client.
fn deserialize_selectors<'de, D>(deserializer: D) -> Result<Vec<NodeSelector>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_compat_list(deserializer, "selectors")
}

fn deserialize_compat_list<'de, D, T>(deserializer: D, field: &str) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned,
{
    use serde::de::Error as _;

    let value = serde_json::Value::deserialize(deserializer)?;
    let value = match value {
        serde_json::Value::String(text) => match serde_json::from_str(&text) {
            Ok(parsed @ serde_json::Value::Array(_)) => parsed,
            _ => serde_json::Value::Array(vec![serde_json::Value::String(text)]),
        },
        other => other,
    };
    serde_json::from_value(value).map_err(|error| {
        D::Error::custom(format!(
            "invalid {field}: {error}; use a JSON array such as [\"manual/1/git\"]"
        ))
    })
}

/// Accept a native MCP scalar or the string spelling emitted by clients that
/// do not preserve the generated JSON Schema type.
fn deserialize_compat_scalar<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned + Default + std::str::FromStr,
    T::Err: std::fmt::Display,
{
    use serde::de::Error as _;

    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(T::default()),
        serde_json::Value::String(text) => text
            .trim()
            .to_ascii_lowercase()
            .parse()
            .map_err(|_| D::Error::custom("invalid stringified scalar value")),
        other => serde_json::from_value(other).map_err(D::Error::custom),
    }
}

/// Optional counterpart to [`deserialize_compat_scalar`].
fn deserialize_compat_optional_scalar<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned + std::str::FromStr,
    T::Err: std::fmt::Display,
{
    use serde::de::Error as _;

    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(text) => text
            .trim()
            .to_ascii_lowercase()
            .parse()
            .map(Some)
            .map_err(|_| D::Error::custom("invalid stringified scalar value")),
        other => serde_json::from_value(other)
            .map(Some)
            .map_err(D::Error::custom),
    }
}

impl FindParams {
    pub(super) fn validate(self) -> Result<ValidatedFindParams, String> {
        let query = self
            .query
            .filter(|query| !query.trim().is_empty())
            .map(|query| bounded_normalized(&query, "query", MAX_FIND_QUERY_BYTES))
            .transpose()?;
        let source = optional_normalized(self.source, "source", MAX_SOURCE_BYTES)?;
        let manual_section = optional_normalized(
            self.manual_section,
            "manualSection",
            MAX_MANUAL_SECTION_BYTES,
        )?;
        let max_results = validate_result_limit(
            self.max_results,
            DEFAULT_FIND_RESULTS,
            MAX_FIND_RESULTS,
            "maxResults",
        )?;
        let page = validate_page(self.start_char, self.max_chars)?;
        if source.is_some() && manual_section.is_some() {
            return Err("source and manualSection cannot be combined".to_owned());
        }
        Ok(ValidatedFindParams {
            query,
            syntax: self.syntax.unwrap_or_default(),
            case: self.case.unwrap_or_default(),
            kind: self.kind,
            source,
            manual_section,
            max_results,
            offset: self.offset,
            page,
        })
    }
}

impl OutlineParams {
    pub(super) fn validate(self) -> Result<ValidatedOutlineParams, String> {
        Ok(ValidatedOutlineParams {
            document: bounded_normalized(&self.document, "document", MAX_DOCUMENT_BYTES)?,
            entries: self.entries.unwrap_or_default(),
            root: optional_normalized(self.root, "root", MAX_SELECTOR_BYTES)?
                .map(NodeSelector::new),
            page: validate_page(self.start_char, self.max_chars)?,
        })
    }
}

impl ReadParams {
    pub(super) fn validate(self) -> Result<ValidatedReadParams, String> {
        if self.selectors.is_empty() || self.selectors.len() > MAX_SELECTORS {
            return Err(format!(
                "selectors must contain between 1 and {MAX_SELECTORS} values"
            ));
        }
        let selectors = self
            .selectors
            .into_iter()
            .map(|selector| {
                bounded_normalized(selector.as_str(), "selector", MAX_SELECTOR_BYTES)
                    .map(NodeSelector::new)
            })
            .collect::<Result<_, _>>()?;
        Ok(ValidatedReadParams {
            document: bounded_normalized(&self.document, "document", MAX_DOCUMENT_BYTES)?,
            selectors,
            page: validate_page(self.start_char, self.max_chars)?,
        })
    }
}

impl ExplainParams {
    pub(super) fn validate(self) -> Result<ValidatedExplainParams, String> {
        Ok(ValidatedExplainParams {
            scope: validate_scope(
                self.documents,
                self.follow_links,
                self.max_depth,
                self.max_documents,
            )?,
            entry: bounded_normalized(
                &self.entry,
                "entry",
                mant_protocol::MAX_SEMANTIC_ENTRY_BYTES,
            )?,
            page: validate_page(self.start_char, self.max_chars)?,
        })
    }
}

impl SearchParams {
    pub(super) fn validate(self) -> Result<ValidatedSearchParams, String> {
        if self.context_lines > 5 {
            return Err("contextLines must be between 0 and 5".to_owned());
        }
        let max_matches = validate_result_limit(
            self.max_matches,
            DEFAULT_SEARCH_MATCHES,
            MAX_SEARCH_MATCHES,
            "maxMatches",
        )?;
        Ok(ValidatedSearchParams {
            documents: validate_scope(
                self.documents,
                self.follow_links,
                self.max_depth,
                self.max_documents,
            )?,
            pattern: bounded_exact(&self.pattern, "pattern", MAX_PATTERN_BYTES)?,
            syntax: self.syntax.unwrap_or_default(),
            case: self.case.unwrap_or_default(),
            scope: self.scope.unwrap_or_default(),
            word: self.word,
            context_lines: self.context_lines,
            max_matches,
            offset: self.offset,
            page: validate_page(self.start_char, self.max_chars)?,
        })
    }
}

fn validate_result_limit(
    value: Option<u32>,
    default: u32,
    maximum: u32,
    field: &str,
) -> Result<u32, String> {
    let value = value.unwrap_or(default);
    if !(1..=maximum).contains(&value) {
        return Err(format!("{field} must be between 1 and {maximum}"));
    }
    Ok(value)
}

fn validate_page(start_char: u32, max_chars: Option<u32>) -> Result<PageRequest, String> {
    let max_chars = max_chars.unwrap_or(DEFAULT_PAGE_CHARS);
    if !(1..=MAX_PAGE_CHARS).contains(&max_chars) {
        return Err(format!("maxChars must be between 1 and {MAX_PAGE_CHARS}"));
    }
    Ok(PageRequest {
        start_char,
        max_chars,
    })
}

fn validate_scope(
    documents: Vec<String>,
    follow_links: bool,
    max_depth: Option<u16>,
    max_documents: Option<u32>,
) -> Result<DocumentScope, String> {
    if documents.is_empty() || documents.len() > mant_protocol::MAX_SCOPE_DOCUMENTS {
        return Err(format!(
            "documents must contain between 1 and {} values",
            mant_protocol::MAX_SCOPE_DOCUMENTS
        ));
    }
    if !follow_links && (max_depth.is_some() || max_documents.is_some()) {
        return Err("maxDepth and maxDocuments require followLinks=true".to_owned());
    }
    let documents = documents
        .into_iter()
        .map(|document| {
            bounded_normalized(&document, "document", MAX_DOCUMENT_BYTES).map(|selector| {
                DocumentSelector {
                    selector,
                    source: None,
                    manual_section: None,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let effective_max_documents =
        max_documents.unwrap_or(mant_protocol::DEFAULT_SCOPE_DOCUMENT_LIMIT);
    if effective_max_documents < u32::try_from(documents.len()).unwrap_or(u32::MAX)
        || effective_max_documents > mant_protocol::MAX_SCOPE_DOCUMENT_LIMIT
    {
        return Err(format!(
            "maxDocuments must include every initial document and not exceed {}",
            mant_protocol::MAX_SCOPE_DOCUMENT_LIMIT
        ));
    }
    let effective_max_depth = max_depth.unwrap_or(mant_protocol::DEFAULT_SCOPE_DEPTH);
    if effective_max_depth > mant_protocol::MAX_SCOPE_DEPTH {
        return Err(format!(
            "maxDepth must not exceed {}",
            mant_protocol::MAX_SCOPE_DEPTH
        ));
    }
    Ok(DocumentScope {
        documents,
        traversal: DocumentTraversal {
            follow_links,
            max_depth,
            max_documents,
        },
    })
}

pub(super) fn catalog_query(parameters: &ValidatedFindParams) -> CatalogQuery {
    CatalogQuery {
        pattern: parameters.query.clone(),
        syntax: parameters.syntax,
        case: parameters.case,
        kind: parameters.kind,
        source: parameters.source.clone(),
        manual_section: parameters.manual_section.clone(),
        limit: parameters.max_results,
        offset: parameters.offset,
    }
}

pub(super) fn request_for(document: String, view: QueryView) -> QueryRequest {
    QueryRequest {
        schema: mant_protocol::RequestSchema::V0Dot10,
        input: QueryInput::Document {
            selector: document,
            source: None,
            manual_section: None,
        },
        view,
    }
}

fn optional_normalized(
    value: Option<String>,
    field: &str,
    max: usize,
) -> Result<Option<String>, String> {
    value
        .map(|value| bounded_normalized(&value, field, max))
        .transpose()
}

fn bounded_normalized(value: &str, field: &str, max: usize) -> Result<String, String> {
    let value = value.trim();
    bounded_exact(value, field, max)
}

fn bounded_exact(value: &str, field: &str, max: usize) -> Result<String, String> {
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
