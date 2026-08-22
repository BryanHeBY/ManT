//! Stable request and response contracts for structure-aware document search.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use mant_ir::{DocumentMeta, DocumentSource, SourceSpan};

use crate::OutlineTrail;

/// Default maximum number of matching line groups returned in one page.
pub const DEFAULT_SEARCH_LIMIT: u32 = 100;

/// Pattern language used for one search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchSyntax {
    /// Match the pattern as ordinary text.
    #[default]
    Literal,
    /// Interpret the pattern as a Rust regular expression.
    Regex,
}

/// Case-folding policy applied when compiling the matcher.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchCase {
    /// Ignore case distinctions.
    #[default]
    Insensitive,
    /// Preserve case distinctions.
    Sensitive,
    /// Match case-sensitively only when the pattern contains uppercase text.
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
    /// Literal or regular-expression search pattern.
    #[schemars(length(min = 1, max = 4096))]
    pub pattern: String,
    /// Pattern language.
    #[serde(default)]
    pub syntax: SearchSyntax,
    /// Case-matching policy.
    #[serde(default)]
    pub case: SearchCase,
    /// Text representation searched.
    #[serde(default)]
    pub scope: SearchScope,
    /// Require matches to be bounded by word boundaries.
    #[serde(default)]
    pub word: bool,
    /// Neighboring rendered lines included around each match.
    #[serde(default)]
    #[schemars(range(max = 100))]
    pub context_lines: u16,
    /// Maximum number of matching line groups returned.
    #[serde(default = "default_search_limit")]
    #[schemars(range(min = 1, max = 10000))]
    pub limit: u32,
    /// Number of matching line groups skipped before collection.
    #[serde(default)]
    pub offset: u32,
}

#[must_use]
/// Return [`DEFAULT_SEARCH_LIMIT`].
pub const fn default_search_limit() -> u32 {
    DEFAULT_SEARCH_LIMIT
}

/// Exact schema marker for structure-aware search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SearchSchema {
    /// Version 0.9 of the pre-stable search protocol.
    #[serde(rename = "mant.search/v0.9")]
    V0Dot9,
}

impl SearchSchema {
    /// Serialized identifier of the current search contract.
    pub const ID: &'static str = "mant.search/v0.9";
}

/// Markdown contract used as the coordinate space for every search format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum MarkdownSchema {
    /// Version 1 of `ManT`'s deterministic Markdown rendering contract.
    #[serde(rename = "mant.markdown/v1")]
    V1,
}

/// Canonical render format used for search coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchRenderFormat {
    /// Generated `CommonMark` text.
    Markdown,
}

/// Amount of the query included in the coordinate-bearing render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchRenderScope {
    /// Complete query document, including optional tldr content.
    Full,
}

/// Description of the deterministic document whose Markdown coordinates are reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchRender {
    /// Coordinate-space schema discriminator.
    pub schema: MarkdownSchema,
    /// Rendered text format.
    pub format: SearchRenderFormat,
    /// Portion of the query represented by the render.
    pub scope: SearchRenderScope,
    /// First valid human-readable line number.
    #[schemars(range(min = 1, max = 1))]
    pub line_base: u8,
    /// First valid human-readable column number.
    #[schemars(range(min = 1, max = 1))]
    pub column_base: u8,
    /// Total rendered line count.
    pub line_count: u32,
}

/// Complete, paginatable search result returned to agents and scripts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:search:v0.9"))]
pub struct QuerySearch {
    /// Exact response schema discriminator.
    pub schema: SearchSchema,
    /// Human-readable selected-document label.
    pub label: String,
    /// Authoritative document source, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DocumentSource>,
    /// Document metadata, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    /// Normalized query applied by the engine.
    pub query: SearchQuery,
    /// Coordinate-space description shared by all matching line groups.
    pub render: SearchRender,
    /// Total matching line groups before pagination.
    pub total: u32,
    /// Number of matching line groups present in [`Self::matches`].
    pub returned: u32,
    /// Applied zero-based matching-line offset.
    pub offset: u32,
    /// Whether additional matching line groups remain.
    pub truncated: bool,
    /// Offset for the next page, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    /// Matching line groups in render order.
    pub matches: Vec<SearchHit>,
}

/// One rendered line or line span containing one or more exact occurrences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// One-based line-group number in the unpaginated result set.
    #[schemars(range(min = 1))]
    pub ordinal: u32,
    /// Complete logical location of the nearest addressable node.
    pub outline: OutlineTrail,
    /// Exact matcher occurrences on this rendered line or line span.
    #[schemars(length(min = 1, max = 256))]
    pub occurrences: Vec<SearchOccurrence>,
    /// Total exact matcher occurrences represented by this line group.
    #[schemars(range(min = 1))]
    pub occurrence_count: u32,
    /// Whether [`Self::occurrences`] omits exact ranges to remain bounded.
    pub occurrences_truncated: bool,
    /// Original-source location of the owning outline node, when retained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_source: Option<SourceSpan>,
    /// Compact single-string presentation of the match.
    pub preview: String,
    /// Optional rendered lines surrounding the match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<SearchContextLine>,
}

/// One exact matcher occurrence in the canonical Markdown render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchOccurrence {
    /// Exact text consumed by the matcher.
    pub matched_text: String,
    /// Location in the deterministic full Markdown render.
    pub markdown: SearchMarkdownRange,
    /// Exact ranges within the anchor-free Markdown lines used for presentation.
    pub line_ranges: Vec<SearchLineRange>,
}

/// One exact occurrence fragment within an anchor-free rendered Markdown line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchLineRange {
    /// One-based line number in the deterministic full Markdown render.
    #[schemars(range(min = 1))]
    pub line: u32,
    /// Inclusive zero-based UTF-8 byte offset within the presented Markdown line.
    pub start_byte: u32,
    /// Exclusive zero-based UTF-8 byte offset within the presented Markdown line.
    pub end_byte: u32,
}

/// Half-open byte range plus one-based human coordinates in full Markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchMarkdownRange {
    /// Inclusive zero-based UTF-8 byte offset.
    pub start_byte: u64,
    /// Exclusive zero-based UTF-8 byte offset.
    pub end_byte: u64,
    /// One-based starting line.
    #[schemars(range(min = 1))]
    pub start_line: u32,
    /// One-based starting column.
    #[schemars(range(min = 1))]
    pub start_column: u32,
    /// One-based ending line.
    #[schemars(range(min = 1))]
    pub end_line: u32,
    /// One-based exclusive ending column.
    #[schemars(range(min = 1))]
    pub end_column: u32,
}

/// One rendered Markdown line surrounding a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchContextLine {
    /// One-based line number in the deterministic Markdown render.
    #[schemars(range(min = 1))]
    pub line: u32,
    /// Complete rendered line without its newline terminator.
    pub text: String,
    /// Whether this is one of the lines intersecting the match.
    pub matched: bool,
}
