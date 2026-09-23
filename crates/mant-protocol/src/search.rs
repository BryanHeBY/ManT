//! One-occurrence search coordinates and bounded response-local validation.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use mant_ir::{Diagnostic, DocumentMeta};

use crate::SourceContext;

mod typed;
pub(crate) use typed::MAX_SEARCH_PROJECTION_BYTES;
pub use typed::*;

/// Default maximum number of occurrences returned in one page.
pub const DEFAULT_SEARCH_LIMIT: u32 = 100;
/// Aggregate response-text bound for retained search presentation.
pub const MAX_SEARCH_PRESENTATION_BYTES: usize = 32 * 1024 * 1024;
/// One default native diagnostic page plus bounded annotation coverage facts.
pub const MAX_SEARCH_DIAGNOSTICS: usize = 65_600;

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

/// Text representation searched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchScope {
    /// Search final visible document content.
    #[default]
    Visible,
    /// Search bytes of the user-exportable Markdown artifact.
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
    /// Neighboring presentation lines included around each match.
    #[serde(default)]
    #[schemars(range(max = 100))]
    pub context_lines: u16,
    /// Maximum number of complete occurrences returned.
    #[serde(default = "default_search_limit")]
    #[schemars(range(min = 1, max = 10000))]
    pub limit: u32,
    /// Number of complete occurrences skipped before retention.
    #[serde(default)]
    pub offset: u32,
}

/// Return [`DEFAULT_SEARCH_LIMIT`].
#[must_use]
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

/// Schema of the coordinate-bearing presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SearchRenderSchema {
    /// Canonical Flow or literal Fixed Markdown export.
    #[serde(rename = "mant.markdown/v1")]
    Markdown,
    /// Final native Fixed visible surface.
    #[serde(rename = "mant.fixed/v1")]
    Fixed,
}

/// The format whose coordinates are used for the search response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchRenderFormat {
    /// User-exportable Markdown artifact.
    Markdown,
    /// Final native Fixed visible surface, before viewport clipping.
    FixedVisible,
}

/// Amount of the query included in the coordinate-bearing render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchRenderScope {
    /// Complete query document, including an optional TLDR.
    Full,
}

/// Description of the coordinate-bearing presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchRender {
    /// Exact coordinate-space schema discriminator.
    pub schema: SearchRenderSchema,
    /// Visible Fixed surface or Markdown artifact.
    pub format: SearchRenderFormat,
    /// Portion represented by this presentation.
    pub scope: SearchRenderScope,
    /// One-based line numbering.
    #[schemars(range(min = 1, max = 1))]
    pub line_base: u8,
    /// One-based column numbering.
    #[schemars(range(min = 1, max = 1))]
    pub column_base: u8,
    /// Number of full presentation rows or lines.
    pub line_count: u32,
}

/// Complete paginated result with one match per returned occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("$id" = "urn:mant:search:v0.12"))]
pub struct QuerySearch {
    /// Exact response schema discriminator.
    pub schema: SearchSchema,
    /// Human-readable selected-document label.
    pub label: String,
    /// Primary document source context, absent for TLDR-only results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_context: Option<SourceContext>,
    /// Primary document metadata, when one was loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocumentMeta>,
    /// Closed, bounded visible units for retained occurrences only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_projection: Option<SearchContentProjection>,
    /// Normalized query applied by the engine.
    pub query: SearchQuery,
    /// Coordinate-space description for this result.
    pub render: SearchRender,
    /// Exact occurrence count after the complete scan.
    pub total: u32,
    /// Number of retained occurrences.
    pub returned: u32,
    /// Applied zero-based occurrence offset.
    pub offset: u32,
    /// Whether another page of occurrences remains.
    pub truncated: bool,
    /// Next occurrence offset, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    /// Completeness of semantic extraction, independent of the selected page.
    pub semantics_complete: bool,
    /// Exact number of coverage details omitted from this bounded response.
    pub coverage_details_omitted: u32,
    /// Bounded producer and coverage findings, retained even for zero hits.
    pub diagnostics: Vec<Diagnostic>,
    /// Retained complete occurrences in snapshot order.
    pub matches: Vec<SearchMatch>,
}

#[derive(Deserialize)]
#[serde(remote = "QuerySearch", rename_all = "camelCase", deny_unknown_fields)]
struct QuerySearchWire {
    schema: SearchSchema,
    label: String,
    source_context: Option<SourceContext>,
    meta: Option<DocumentMeta>,
    content_projection: Option<SearchContentProjection>,
    query: SearchQuery,
    render: SearchRender,
    total: u32,
    returned: u32,
    offset: u32,
    truncated: bool,
    next_offset: Option<u32>,
    semantics_complete: bool,
    coverage_details_omitted: u32,
    diagnostics: Vec<Diagnostic>,
    matches: Vec<SearchMatch>,
}

impl<'de> Deserialize<'de> for QuerySearch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = QuerySearchWire::deserialize(deserializer)?;
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl QuerySearch {
    /// Validate one in-memory result before presenting or serializing it.
    ///
    /// # Errors
    /// Returns malformed pagination, coordinates, coverage or source closure.
    pub fn validate(&self) -> Result<(), String> {
        if self.query.limit == 0
            || self.query.limit > 10_000
            || self.query.context_lines > 100
            || self.returned > self.query.limit
        {
            return Err("search request or returned page exceeds protocol bounds".into());
        }
        validate_search_page(
            self.offset,
            self.returned,
            self.total,
            self.truncated,
            self.next_offset,
            &self.matches,
        )?;
        validate_search_content(
            self.query.scope,
            &self.render,
            self.content_projection.as_ref(),
            &self.matches,
        )?;
        validate_search_coverage(
            self.semantics_complete,
            self.coverage_details_omitted,
            &self.diagnostics,
        )?;
        validate_search_presentation(&self.matches)?;
        crate::document::validate_optional_source_spans(
            self.source_context.as_ref(),
            self.matches
                .iter()
                .filter_map(|matched| matched.node_source)
                .chain(
                    self.diagnostics
                        .iter()
                        .filter_map(|diagnostic| diagnostic.source),
                ),
        )?;
        Ok(())
    }
}

pub(crate) fn validate_search_presentation(matches: &[SearchMatch]) -> Result<(), &'static str> {
    let mut bytes = 0usize;
    for matched in matches {
        bytes = bytes
            .checked_add(matched.matched_text.len())
            .and_then(|value| value.checked_add(matched.preview.len()))
            .ok_or("search presentation byte count overflows")?;
        if matched.context.len() > 201 {
            return Err("search context exceeds line bound");
        }
        for line in &matched.context {
            if line.line == 0 {
                return Err("search context line is not one-based");
            }
            bytes = bytes
                .checked_add(line.text.len())
                .ok_or("search presentation byte count overflows")?;
        }
        if bytes > MAX_SEARCH_PRESENTATION_BYTES {
            return Err("search presentation exceeds byte budget");
        }
    }
    Ok(())
}

pub(crate) fn validate_search_page(
    offset: u32,
    returned: u32,
    total: u32,
    truncated: bool,
    next_offset: Option<u32>,
    matches: &[SearchMatch],
) -> Result<(), &'static str> {
    if usize::try_from(returned).ok() != Some(matches.len()) {
        return Err("search returned count does not match retained occurrences");
    }
    let end = offset
        .checked_add(returned)
        .ok_or("search pagination metadata overflows")?;
    if (returned != 0 && end > total) || (returned == 0 && offset < total) {
        return Err("search pagination metadata is inconsistent");
    }
    let expected_truncated = returned != 0 && end < total;
    if truncated != expected_truncated || next_offset != expected_truncated.then_some(end) {
        return Err("search pagination metadata is inconsistent");
    }
    for (index, matched) in matches.iter().enumerate() {
        let expected = offset
            .checked_add(u32::try_from(index).map_err(|_| "search ordinal overflows")?)
            .and_then(|ordinal| ordinal.checked_add(1))
            .ok_or("search ordinal overflows")?;
        if matched.ordinal != expected {
            return Err("search occurrence ordinal disagrees with pagination order");
        }
    }
    Ok(())
}

pub(crate) fn validate_search_content(
    scope: SearchScope,
    render: &SearchRender,
    projection: Option<&SearchContentProjection>,
    matches: &[SearchMatch],
) -> Result<(), &'static str> {
    if render.line_base != 1 || render.column_base != 1 {
        return Err("search render coordinates must be one-based");
    }
    match (render.schema, render.format, scope) {
        (SearchRenderSchema::Markdown, SearchRenderFormat::Markdown, _)
        | (SearchRenderSchema::Fixed, SearchRenderFormat::FixedVisible, SearchScope::Visible) => {}
        _ => return Err("search render schema, format or scope is inconsistent"),
    }
    if let Some(projection) = projection {
        projection.validate()?;
        // A Fixed render can also carry a TLDR visible unit, but never a
        // fragment borrowed from the mutually exclusive Flow document body.
        if render.format == SearchRenderFormat::FixedVisible
            && projection
                .fragments
                .iter()
                .any(|fragment| matches!(&fragment.source, SearchFragmentSource::Flow(_)))
        {
            return Err("fixed search render contains a Flow body fragment");
        }
    }
    if matches.is_empty() && projection.is_some() {
        return Err("search without retained hits must not carry a projection");
    }
    let mut used_units = projection.map(|projection| vec![false; projection.units.len()]);
    let mut validation_bytes = 0usize;
    for matched in matches {
        match (scope, matched.location) {
            (
                SearchScope::Markdown,
                SearchLocation::MarkdownArtifact {
                    start_byte,
                    end_byte,
                    start_line,
                    start_column,
                    end_line,
                    end_column,
                },
            ) => {
                if projection.is_some()
                    || !matched.display_slices.is_empty()
                    || start_byte >= end_byte
                    || end_byte - start_byte != matched.matched_text.len() as u64
                    || start_line == 0
                    || start_column == 0
                    || end_line == 0
                    || end_column == 0
                    || start_line > end_line
                    || (start_line == end_line && start_column >= end_column)
                    || end_line > render.line_count
                {
                    return Err("search artifact coordinate is invalid");
                }
            }
            (SearchScope::Visible, SearchLocation::VisibleFlow { .. }) => {
                let projection = projection.ok_or("visible search hit requires a projection")?;
                if let SearchLocation::VisibleFlow { unit, .. } = matched.location {
                    validation_bytes = validation_bytes
                        .checked_add(projection.unit_expanded_len(unit)?)
                        .ok_or("search match validation work overflows")?;
                }
                if validation_bytes > MAX_SEARCH_PROJECTION_BYTES {
                    return Err("search match validation exceeds byte budget");
                }
                projection.validate_match(matched)?;
                if let SearchLocation::VisibleFlow { unit, .. } = matched.location {
                    used_units.as_mut().expect("visible projection")[unit.get() as usize - 1] =
                        true;
                }
            }
            (SearchScope::Visible, SearchLocation::VisibleFixed { .. })
                if render.format == SearchRenderFormat::FixedVisible =>
            {
                let projection = projection.ok_or("visible search hit requires a projection")?;
                if let SearchLocation::VisibleFixed { unit, .. } = matched.location {
                    validation_bytes = validation_bytes
                        .checked_add(projection.unit_expanded_len(unit)?)
                        .ok_or("search match validation work overflows")?;
                }
                if validation_bytes > MAX_SEARCH_PROJECTION_BYTES {
                    return Err("search match validation exceeds byte budget");
                }
                projection.validate_match(matched)?;
                if let SearchLocation::VisibleFixed { unit, .. } = matched.location {
                    used_units.as_mut().expect("visible projection")[unit.get() as usize - 1] =
                        true;
                }
            }
            _ => return Err("search match has wrong coordinate basis"),
        }
    }
    if used_units.is_some_and(|units| units.into_iter().any(|used| !used)) {
        return Err("search projection retains an unreferenced unit");
    }
    Ok(())
}

pub(crate) fn validate_search_coverage(
    complete: bool,
    omitted: u32,
    diagnostics: &[Diagnostic],
) -> Result<(), &'static str> {
    if diagnostics.len() > MAX_SEARCH_DIAGNOSTICS {
        return Err("search diagnostics exceed response bound");
    }
    let summaries = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code.as_deref() == Some("annotated.coverage.summary")
                && diagnostic.impact == mant_ir::DiagnosticImpact::SemanticCoverage
                && diagnostic.coverage_scope == Some(mant_ir::CoverageScope::Document)
        })
        .count();
    if (omitted == 0 && summaries != 0) || (omitted != 0 && summaries != 1) {
        return Err("search coverage omission summary is inconsistent");
    }
    if complete != mant_ir::semantics_complete(diagnostics) {
        return Err("search semantic completeness disagrees with diagnostics");
    }
    Ok(())
}

/// One presentation line surrounding an exact occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchContextLine {
    /// One-based presentation line number.
    #[schemars(range(min = 1))]
    pub line: u32,
    /// Rendered line without its newline terminator.
    pub text: String,
    /// Whether this line intersects the occurrence.
    pub matched: bool,
}

#[cfg(test)]
mod tests {
    use super::validate_search_page;

    #[test]
    fn page_arithmetic_never_saturates_into_a_plausible_response() {
        assert!(validate_search_page(u32::MAX, 1, u32::MAX, false, None, &[]).is_err());
        assert!(validate_search_page(u32::MAX, 0, 0, false, None, &[]).is_ok());
    }
}
