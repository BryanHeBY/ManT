//! Closed input schemas exposed by the MCP tools.

use mant_protocol::{
    CatalogDocumentKind, CatalogQuery, NodeSelector, OutlineDetail, QueryInput, QueryRequest,
    QueryView, SearchCase, SearchScope, SearchSyntax,
};
use schemars::JsonSchema;
use serde::Deserialize;

/// A document name resolved through registered Markdown and native manual paths.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DocumentSelector {
    /// Registered Markdown or manual page name, for example `git` or `mant`.
    pub(super) name: String,
    /// Optional configured Markdown source. It bypasses root documents and manuals.
    pub(super) source: Option<String>,
    /// Optional native manual category. Supplying it bypasses registered Markdown.
    pub(super) manual_section: Option<String>,
}

/// Parameters for the hierarchy-discovery tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OutlineParams {
    #[serde(flatten)]
    pub(super) selector: DocumentSelector,
    /// Include only sections, or include every addressable semantic entry.
    pub(super) detail: Option<OutlineDetail>,
}

/// Parameters for retrieving one or more outline nodes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GetParams {
    #[serde(flatten)]
    pub(super) selector: DocumentSelector,
    /// Outline paths, stable IDs, or entry aliases returned by the outline tool.
    #[schemars(length(min = 1))]
    #[serde(deserialize_with = "lenient_selectors")]
    pub(super) selectors: Vec<NodeSelector>,
}

/// Parameters for resolving a single option, command, variable, or environment entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ExplainParams {
    #[serde(flatten)]
    pub(super) selector: DocumentSelector,
    /// Option spelling, command or variable name, outline path, or stable ID.
    pub(super) entry: String,
}

/// Parameters for structure-aware manual search.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SearchParams {
    #[serde(flatten)]
    pub(super) selector: DocumentSelector,
    /// Literal text or a regular expression, depending on `syntax`.
    #[schemars(length(min = 1, max = 4096))]
    pub(super) pattern: String,
    /// Interpret `pattern` literally (the default) or as a regular expression.
    pub(super) syntax: Option<SearchSyntax>,
    /// Case-folding policy. The default is `insensitive`.
    pub(super) case: Option<SearchCase>,
    /// Search visible text (the default) or generated `CommonMark` source.
    pub(super) scope: Option<SearchScope>,
    /// Restrict matches to Unicode-aware word boundaries.
    #[serde(default, deserialize_with = "lenient_scalar")]
    pub(super) word: Option<bool>,
    /// Full Markdown lines of context before and after each match, at most 100.
    #[schemars(range(max = 100))]
    #[serde(default, deserialize_with = "lenient_scalar", alias = "context_lines")]
    pub(super) context_lines: Option<u16>,
    /// Maximum result count from 1 through 10,000. The default is 100.
    #[schemars(range(min = 1, max = 10000))]
    #[serde(default, deserialize_with = "lenient_scalar")]
    pub(super) limit: Option<u32>,
    /// Number of matches to skip for deterministic pagination.
    #[serde(default, deserialize_with = "lenient_scalar")]
    pub(super) offset: Option<u32>,
}

/// Filters and pagination for the unified local document catalog.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DocumentListParams {
    /// Case-insensitive substring applied to document names.
    pub(super) query: Option<String>,
    /// Restrict discovery to registered Markdown or manual pages.
    pub(super) kind: Option<CatalogDocumentKind>,
    /// Interpret `query` literally (the default) or as a regular expression.
    pub(super) syntax: Option<SearchSyntax>,
    /// Case-folding policy. The default is `insensitive`.
    pub(super) case: Option<SearchCase>,
    /// Restrict manual pages to one exact manual category; excludes Markdown entries.
    pub(super) manual_section: Option<String>,
    /// Restrict Markdown discovery to one configured source.
    pub(super) source: Option<String>,
    /// Maximum entries returned from 1 through 10,000. The default is 100.
    #[schemars(range(min = 1, max = 10000))]
    #[serde(default, deserialize_with = "lenient_scalar")]
    pub(super) limit: Option<u32>,
    /// Number of matching entries to skip for deterministic pagination.
    #[serde(default, deserialize_with = "lenient_scalar")]
    pub(super) offset: Option<u32>,
}

/// Accepts a native scalar or its stringified spelling such as `"10"` or `"True"`.
fn lenient_scalar<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + std::str::FromStr,
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
            .map_err(|error| D::Error::custom(format!("cannot parse {text:?}: {error}"))),
        other => serde_json::from_value(other)
            .map(Some)
            .map_err(D::Error::custom),
    }
}

const SELECTORS_HINT: &str =
    r#"selectors must be an array of outline selectors such as ["2","1/e1"]"#;

/// Accepts a selector array, one bare selector, or a stringified JSON array.
fn lenient_selectors<'de, D>(deserializer: D) -> Result<Vec<NodeSelector>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    let value = serde_json::Value::deserialize(deserializer)?;
    let value = match value {
        serde_json::Value::String(text) => match serde_json::from_str(&text) {
            Ok(parsed @ serde_json::Value::Array(_)) => parsed,
            _ => return Ok(vec![text.into()]),
        },
        other => other,
    };
    serde_json::from_value(value)
        .map_err(|error| D::Error::custom(format!("{error}; {SELECTORS_HINT}")))
}

pub(super) fn validate_document_list(
    mut parameters: DocumentListParams,
) -> Result<DocumentListParams, String> {
    parameters.query = parameters
        .query
        .map(|query| query.trim().to_owned())
        .filter(|query| !query.is_empty());
    parameters.manual_section = parameters
        .manual_section
        .map(|manual_section| non_empty(&manual_section, "manualSection"))
        .transpose()?;
    parameters.source = parameters
        .source
        .map(|source| non_empty(&source, "source"))
        .transpose()?;
    if parameters.source.is_some() && parameters.manual_section.is_some() {
        return Err("source and manualSection cannot be combined".to_owned());
    }
    let limit = parameters.limit.unwrap_or(100);
    if !(1..=10_000).contains(&limit) {
        return Err("limit must be between 1 and 10000".to_owned());
    }
    parameters.limit = Some(limit);
    parameters.offset = Some(parameters.offset.unwrap_or(0));
    Ok(parameters)
}

pub(super) fn catalog_query(parameters: &DocumentListParams) -> CatalogQuery {
    CatalogQuery {
        pattern: parameters.query.clone(),
        syntax: parameters.syntax.unwrap_or_default(),
        case: parameters.case.unwrap_or_default(),
        kind: parameters.kind,
        source: parameters.source.clone(),
        manual_section: parameters.manual_section.clone(),
        limit: parameters.limit.unwrap_or(100),
        offset: parameters.offset.unwrap_or(0),
    }
}

pub(super) fn request_for(selector: DocumentSelector, view: QueryView) -> QueryRequest {
    QueryRequest {
        schema: mant_protocol::RequestSchema::V0Dot8,
        input: QueryInput::Document {
            selector: selector.name,
            source: selector.source,
            manual_section: selector.manual_section,
        },
        view,
    }
}

pub(super) fn non_empty(value: &str, field: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field} must not be empty"))
    } else {
        Ok(value.to_owned())
    }
}
