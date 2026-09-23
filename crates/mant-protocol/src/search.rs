//! Stable request and response contracts for structure-aware document search.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use mant_ir::{ContentProjection, ContentRootKey, DocumentMeta, SourceSpan};

use crate::{OutlineTrail, SourceContext};

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

/// Text representation searched while Markdown remains available for presentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchScope {
    /// Search visible document text.
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
    /// Version 0.12 of the pre-stable search protocol.
    #[serde(rename = "mant.search/v0.12")]
    V0Dot12,
}

impl SearchSchema {
    /// Serialized identifier of the current search contract.
    pub const ID: &'static str = "mant.search/v0.12";
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("$id" = "urn:mant:search:v0.12"))]
pub struct QuerySearch {
    /// Exact response schema discriminator.
    pub schema: SearchSchema,
    /// Human-readable selected-document label.
    pub label: String,
    /// Authoritative document source, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_context: Option<SourceContext>,
    /// Document metadata, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    /// Closed response-local store resolving every returned logical root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_projection: Option<ContentProjection>,
    /// Normalized query applied by the engine.
    pub query: SearchQuery,
    /// Coordinate-space description shared by all matching line groups.
    pub render: SearchRender,
    /// Total matching line groups before pagination.
    pub total: u32,
    /// Number of matching line groups present in [`Self::matches`].
    pub returned: u32,
    /// Applied zero-based matching-line-group offset.
    pub offset: u32,
    /// Whether additional matching line groups remain.
    pub truncated: bool,
    /// Offset for the next page, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    /// Matching line groups in canonical render order.
    pub matches: Vec<SearchHit>,
}

#[derive(Deserialize)]
#[serde(remote = "QuerySearch", rename_all = "camelCase", deny_unknown_fields)]
struct QuerySearchWire {
    pub schema: SearchSchema,
    pub label: String,
    pub source_context: Option<SourceContext>,
    pub meta: Option<DocumentMeta>,
    #[serde(default)]
    pub content_projection: Option<ContentProjection>,
    pub query: SearchQuery,
    pub render: SearchRender,
    pub total: u32,
    pub returned: u32,
    pub offset: u32,
    pub truncated: bool,
    pub next_offset: Option<u32>,
    pub matches: Vec<SearchHit>,
}

impl<'de> Deserialize<'de> for QuerySearch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = QuerySearchWire::deserialize(deserializer)?;
        validate_search_page(&value).map_err(serde::de::Error::custom)?;
        if value.query.scope == SearchScope::Markdown && value.content_projection.is_some() {
            return Err(serde::de::Error::custom(
                "Markdown-scope search must use rendered coordinates",
            ));
        }
        crate::document::validate_optional_source_spans(
            value.source_context.as_ref(),
            value.matches.iter().filter_map(|hit| hit.node_source),
        )
        .map_err(serde::de::Error::custom)?;
        crate::document::validate_projection_sources(
            value.source_context.as_ref(),
            value.content_projection.as_ref(),
        )
        .map_err(serde::de::Error::custom)?;
        validate_search_content(value.content_projection.as_ref(), &value.matches)
            .map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

fn validate_search_page(value: &QuerySearch) -> Result<(), &'static str> {
    if usize::try_from(value.returned).ok() != Some(value.matches.len()) {
        return Err("search returned count does not match retained hits");
    }
    let end = checked_page_end(value.offset, value.returned)?;
    if (value.returned != 0 && end > value.total)
        || (value.returned == 0 && value.offset < value.total)
    {
        return Err("search pagination metadata is inconsistent");
    }
    let truncated = value.returned != 0 && end < value.total;
    if value.truncated != truncated || value.next_offset != truncated.then_some(end) {
        return Err("search pagination metadata is inconsistent");
    }
    for (index, hit) in value.matches.iter().enumerate() {
        let expected = checked_ordinal(value.offset, index)?;
        if hit.ordinal != expected {
            return Err("search hit ordinal disagrees with pagination order");
        }
    }
    Ok(())
}

fn checked_page_end(offset: u32, returned: u32) -> Result<u32, &'static str> {
    offset
        .checked_add(returned)
        .ok_or("search pagination metadata overflows")
}

fn checked_ordinal(offset: u32, index: usize) -> Result<u32, &'static str> {
    offset
        .checked_add(u32::try_from(index).map_err(|_| "search ordinal overflows")?)
        .and_then(|ordinal| ordinal.checked_add(1))
        .ok_or("search ordinal overflows")
}

#[cfg(test)]
mod pagination_tests {
    use super::{checked_ordinal, checked_page_end};

    #[test]
    fn page_arithmetic_never_saturates_into_a_plausible_response() {
        assert_eq!(checked_page_end(u32::MAX - 1, 1), Ok(u32::MAX));
        assert!(checked_page_end(u32::MAX, 1).is_err());
        assert_eq!(checked_ordinal(u32::MAX - 1, 0), Ok(u32::MAX));
        assert!(checked_ordinal(u32::MAX, 0).is_err());
        assert!(checked_ordinal(0, usize::MAX).is_err());
    }
}

/// One rendered line group containing one or more exact occurrences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// One-based line-group number in the unpaginated result set.
    #[schemars(range(min = 1))]
    pub ordinal: u32,
    /// Complete logical location of the nearest addressable node.
    pub outline: OutlineTrail,
    /// Retained exact occurrences represented by this hit.
    #[schemars(length(min = 1, max = 256))]
    pub occurrences: Vec<SearchOccurrence>,
    /// Total exact occurrences, including any omitted to bound the response.
    #[schemars(range(min = 1))]
    pub occurrence_count: u32,
    /// Whether occurrence ranges were omitted to bound a rendered line group.
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

/// One exact matcher occurrence in either a logical root or the rendered document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchOccurrence {
    /// Bounded presentation echo; the logical range remains authoritative.
    pub matched_text: String,
    /// Response-local logical root containing the complete match, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<ContentRootKey>,
    /// Root-relative logical UTF-8 byte and Unicode-scalar range, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical: Option<SearchLogicalRange>,
    /// Zero or more presentation-only placements in canonical Markdown v1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markdown_projections: Vec<SearchMarkdownRange>,
    /// Authoritative Markdown range for a render-only hit, including TLDR text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<SearchMarkdownRange>,
    /// Anchor-free presentation fragments for a render-only hit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line_ranges: Vec<SearchLineRange>,
}

/// One exact occurrence fragment within an anchor-free rendered Markdown line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchLineRange {
    /// One-based line number in the deterministic full Markdown render.
    #[schemars(range(min = 1))]
    pub line: u32,
    /// Inclusive zero-based UTF-8 byte offset within the presented line.
    pub start_byte: u32,
    /// Exclusive zero-based UTF-8 byte offset within the presented line.
    pub end_byte: u32,
}

/// Half-open root-relative logical coordinates for one matcher occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchLogicalRange {
    /// Inclusive UTF-8 byte offset in the root's canonical logical sequence.
    pub start_byte: u64,
    /// Exclusive UTF-8 byte offset in the root's canonical logical sequence.
    pub end_byte: u64,
    /// Inclusive Unicode-scalar offset in the same sequence.
    pub start_scalar: u64,
    /// Exclusive Unicode-scalar offset in the same sequence.
    pub end_scalar: u64,
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

pub(crate) fn validate_search_content(
    projection: Option<&ContentProjection>,
    matches: &[SearchHit],
) -> Result<(), &'static str> {
    for hit in matches {
        if hit.occurrences.is_empty()
            || hit.occurrences.len() > 256
            || hit.occurrence_count < u32::try_from(hit.occurrences.len()).unwrap_or(u32::MAX)
            || hit.occurrences_truncated != (hit.occurrence_count as usize > hit.occurrences.len())
        {
            return Err("search hit occurrence count is inconsistent");
        }
        if projection.is_some()
            && (hit.occurrences.len() != 1
                || hit.occurrence_count != 1
                || hit.occurrences_truncated)
        {
            return Err("logical search hit must contain exactly one complete occurrence");
        }
    }
    if matches.is_empty() {
        return if projection.is_none() {
            Ok(())
        } else {
            Err("search without retained hits must not carry a content projection")
        };
    }
    for occurrence in matches.iter().flat_map(|hit| &hit.occurrences) {
        match (occurrence.root, occurrence.logical, occurrence.markdown) {
            (Some(root), Some(logical), None) => {
                let projection =
                    projection.ok_or("logical search hits require a content projection")?;
                let text = projection
                    .content_store
                    .root_logical_text(root)
                    .ok_or("search hit root does not resolve in its content projection")?;
                validate_logical_range(&text, logical)?;
                let start = usize::try_from(logical.start_byte)
                    .map_err(|_| "search byte range is too large")?;
                let end = usize::try_from(logical.end_byte)
                    .map_err(|_| "search byte range is too large")?;
                if text.get(start..end) != Some(occurrence.matched_text.as_str()) {
                    return Err(
                        "search matched text does not equal its authoritative logical range",
                    );
                }
                if !occurrence.line_ranges.is_empty() {
                    return Err("logical search hit must not carry render-only line ranges");
                }
                for markdown in &occurrence.markdown_projections {
                    validate_markdown_range(*markdown)?;
                }
            }
            (None, None, Some(markdown)) => {
                if projection.is_some() || !occurrence.markdown_projections.is_empty() {
                    return Err("render-only search hit must not carry a logical projection");
                }
                validate_markdown_range(markdown)?;
                if occurrence.line_ranges.is_empty()
                    || occurrence
                        .line_ranges
                        .iter()
                        .any(|range| range.line == 0 || range.start_byte >= range.end_byte)
                {
                    return Err("render-only search hit has no valid presented line range");
                }
            }
            _ => return Err("search hit must have exactly one complete coordinate basis"),
        }
    }
    Ok(())
}

fn validate_logical_range(text: &str, range: SearchLogicalRange) -> Result<(), &'static str> {
    let start = usize::try_from(range.start_byte).map_err(|_| "search byte range is too large")?;
    let end = usize::try_from(range.end_byte).map_err(|_| "search byte range is too large")?;
    if start >= end || text.get(start..end).is_none() {
        return Err("search byte range is empty or outside its logical root");
    }
    let start_scalar = text[..start].chars().count() as u64;
    let end_scalar = start_scalar.saturating_add(text[start..end].chars().count() as u64);
    if range.start_scalar != start_scalar || range.end_scalar != end_scalar {
        return Err("search scalar range does not match its logical byte range");
    }
    Ok(())
}

fn validate_markdown_range(range: SearchMarkdownRange) -> Result<(), &'static str> {
    if range.start_byte >= range.end_byte
        || range.start_line == 0
        || range.end_line == 0
        || range.start_column == 0
        || range.end_column == 0
        || range.start_line > range.end_line
        || (range.start_line == range.end_line && range.start_column >= range.end_column)
    {
        return Err("search Markdown projection range is empty or unordered");
    }
    Ok(())
}

/// One logical-root line surrounding a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchContextLine {
    /// One-based line number inside the authoritative logical root.
    #[schemars(range(min = 1))]
    pub line: u32,
    /// Complete rendered line without its newline terminator.
    pub text: String,
    /// Whether this is one of the lines intersecting the match.
    pub matched: bool,
}
