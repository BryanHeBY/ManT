//! Stable request and response contracts for structure-aware document search.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{DefinitionRole, SourceSpan};

pub const DEFAULT_SEARCH_LIMIT: u32 = 100;

/// Pattern language used for one search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchSyntax {
    #[default]
    Literal,
    Regex,
}

/// Case-folding policy applied when compiling the matcher.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchCase {
    #[default]
    Insensitive,
    Sensitive,
    Smart,
}

/// Text representation searched while Markdown remains the coordinate basis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchScope {
    /// Search the text visible after parsing `ManT`'s generated `CommonMark`.
    #[default]
    Visible,
    /// Search the generated `CommonMark` bytes, including markup.
    Markdown,
}

/// Normalized search configuration echoed in a search response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchQuery {
    #[schemars(length(min = 1, max = 4096))]
    pub pattern: String,
    #[serde(default)]
    pub syntax: SearchSyntax,
    #[serde(default)]
    pub case: SearchCase,
    #[serde(default)]
    pub scope: SearchScope,
    #[serde(default)]
    pub word: bool,
    #[serde(default)]
    #[schemars(range(max = 100))]
    pub context_lines: u16,
    #[serde(default = "default_search_limit")]
    #[schemars(range(min = 1, max = 10000))]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

#[must_use]
pub const fn default_search_limit() -> u32 {
    DEFAULT_SEARCH_LIMIT
}

/// Exact schema marker for structure-aware search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SearchSchema {
    #[serde(rename = "mant.search/v1")]
    V1,
}

/// Markdown contract used as the coordinate space for every search format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum MarkdownSchema {
    #[serde(rename = "mant.markdown/v1")]
    V1,
}

/// Canonical render format used for search coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchRenderFormat {
    Markdown,
}

/// Amount of the query included in the coordinate-bearing render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchRenderScope {
    Full,
}

/// Description of the deterministic document whose Markdown coordinates are reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchRender {
    pub schema: MarkdownSchema,
    pub format: SearchRenderFormat,
    pub scope: SearchRenderScope,
    pub line_base: u8,
    pub column_base: u8,
    pub line_count: u32,
}

/// Complete, paginatable search result returned to agents and scripts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:search:v1"))]
pub struct QuerySearch {
    pub schema: SearchSchema,
    pub topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_section: Option<String>,
    pub query: SearchQuery,
    pub render: SearchRender,
    pub total: u32,
    pub returned: u32,
    pub offset: u32,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    pub matches: Vec<SearchMatch>,
}

/// One exact occurrence and both of its structural and rendered locations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub ordinal: u32,
    pub node: SearchNode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<SearchSectionReference>,
    pub matched_text: String,
    pub markdown: SearchMarkdownRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
    pub preview: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<SearchContextLine>,
}

/// Nearest node accepted by `mant-cli --node` for a matching occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SearchNode {
    Tldr {
        path: String,
        id: String,
        title: String,
    },
    ManualSection {
        path: String,
        id: String,
        title: String,
    },
    ManualEntry {
        path: String,
        id: String,
        title: String,
        role: DefinitionRole,
        names: Vec<String>,
    },
}

impl SearchNode {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Tldr { path, .. }
            | Self::ManualSection { path, .. }
            | Self::ManualEntry { path, .. } => path,
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::Tldr { title, .. }
            | Self::ManualSection { title, .. }
            | Self::ManualEntry { title, .. } => title,
        }
    }
}

/// Addressable containing section for a non-tldr match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchSectionReference {
    pub path: String,
    pub id: String,
    pub title: String,
}

/// Half-open byte range plus one-based human coordinates in full Markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchMarkdownRange {
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// One rendered Markdown line surrounding a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchContextLine {
    pub line: u32,
    pub text: String,
    pub matched: bool,
}
