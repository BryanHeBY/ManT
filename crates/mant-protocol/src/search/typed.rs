//! Closed, response-local coordinates for one exact search occurrence.
//!
//! A decoder can prove the projected unit text and match range without the
//! original document. Only the producer can prove that a snapshot-relative
//! Flow location, TLDR path, or Fixed row/run names the original source; it
//! performs that check before copying a fragment into this bounded response.

use std::num::NonZeroU32;

use mant_ir::ContentLocation;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use super::SearchContextLine;
use crate::OutlineTrail;

const MAX_SEARCH_PROJECTION_OBJECTS: usize = 1_000_000;
pub(crate) const MAX_SEARCH_PROJECTION_BYTES: usize = 32 * 1024 * 1024;
const MAX_SEARCH_PROJECTION_EDGES: usize = 8_000_000;

/// A response-local unit key, dense within one search projection.
pub type SearchUnitKey = NonZeroU32;
/// A response-local fragment key, dense within one search projection.
pub type SearchFragmentKey = NonZeroU32;

/// One exact match, never a group of equal-line matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchMatch {
    /// One-based occurrence ordinal in the unpaginated snapshot.
    #[schemars(range(min = 1))]
    pub ordinal: u32,
    /// Nearest addressable node for navigation, independent of match identity.
    pub outline: OutlineTrail,
    /// Exact searched Unicode text, not a presentation-only preview.
    pub matched_text: String,
    /// Authoritative tagged occurrence coordinate.
    pub location: SearchLocation,
    /// Subordinate visible fragments; empty for artifact-only hits.
    #[serde(default)]
    pub display_slices: Vec<SearchDisplaySlice>,
    /// Source of the outline owner, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_source: Option<mant_ir::SourceSpan>,
    /// Bounded human presentation; never coordinate authority.
    pub preview: String,
    /// Bounded surrounding presentation lines.
    #[serde(default)]
    pub context: Vec<SearchContextLine>,
}

/// The sole authoritative coordinate of one match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SearchLocation {
    /// A validated Flow or TLDR visible-text unit.
    VisibleFlow {
        /// Response-local complete text unit.
        unit: SearchUnitKey,
        /// Inclusive Unicode scalar offset in the unit.
        start_scalar: u64,
        /// Exclusive Unicode scalar offset in the unit.
        end_scalar: u64,
    },
    /// A validated Fixed visible-text unit.
    VisibleFixed {
        /// Response-local complete text unit.
        unit: SearchUnitKey,
        /// Inclusive Unicode scalar offset in the unit.
        start_scalar: u64,
        /// Exclusive Unicode scalar offset in the unit.
        end_scalar: u64,
    },
    /// Unicode scalar range in the exact user-exportable Markdown artifact.
    MarkdownArtifact {
        /// Inclusive artifact Unicode scalar offset.
        start_scalar: u64,
        /// Exclusive artifact Unicode scalar offset.
        end_scalar: u64,
        /// One-based start line.
        #[schemars(range(min = 1))]
        start_line: u32,
        /// One-based start column.
        #[schemars(range(min = 1))]
        start_column: u32,
        /// One-based end line.
        #[schemars(range(min = 1))]
        end_line: u32,
        /// One-based exclusive end column.
        #[schemars(range(min = 1))]
        end_column: u32,
    },
}

/// The part of one response fragment that highlights the actual display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchDisplaySlice {
    /// Fragment referenced by this display slice.
    pub fragment: SearchFragmentKey,
    /// Inclusive fragment-relative Unicode scalar offset.
    pub start_scalar: u64,
    /// Exclusive fragment-relative Unicode scalar offset.
    pub end_scalar: u64,
}

/// Only retained occurrences contribute fragments and units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchContentProjection {
    /// Dense copied fragments, each with a typed producer-checked source.
    pub fragments: Vec<SearchFragment>,
    /// Dense ordered units closing every visible match.
    pub units: Vec<SearchTextUnit>,
}

#[derive(Deserialize)]
#[serde(
    remote = "SearchContentProjection",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct SearchContentProjectionWire {
    fragments: Vec<SearchFragment>,
    units: Vec<SearchTextUnit>,
}

impl<'de> Deserialize<'de> for SearchContentProjection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = SearchContentProjectionWire::deserialize(deserializer)?;
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

/// One exact visible fragment; text is copied only for a retained hit.
/// The sole empty form is a derived terminal endpoint for a render join.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchFragment {
    /// Dense one-based response-local key.
    pub key: SearchFragmentKey,
    /// Exact UTF-8 bytes contributed by this fragment.
    pub text: String,
    /// Snapshot-relative producer-checked origin or honest derived status.
    pub source: SearchFragmentSource,
}

/// Provenance of a retained visible fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SearchFragmentSource {
    /// Exact Flow content location and root-relative source bytes.
    Flow(SearchFlowFragmentSource),
    /// Exact quick-reference path and source bytes, not a manual `SourceKey`.
    Tldr(SearchTldrFragmentSource),
    /// Visible bytes produced by a rendering transform without proven authored bytes.
    RenderDerived(SearchRenderDerivedFragmentSource),
    /// Final native row/run bytes.
    Fixed(SearchFixedFragmentSource),
}

/// Exact Flow source of one copied fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchFlowFragmentSource {
    /// Required source-kind discriminator.
    pub kind: SearchFlowSourceKind,
    /// Complete document-local structural location.
    pub location: ContentLocation,
    /// Inclusive root-relative UTF-8 byte offset.
    pub start_byte: u64,
    /// Exclusive root-relative UTF-8 byte offset.
    pub end_byte: u64,
}

/// Literal discriminator of a Flow fragment source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SearchFlowSourceKind {
    /// A source fragment of the primary Flow document.
    #[serde(rename = "flow")]
    Flow,
}

/// Exact TLDR source of one copied fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchTldrFragmentSource {
    /// Required source-kind discriminator.
    pub kind: SearchTldrSourceKind,
    /// Current TLDR selection path, independent of the primary source table.
    pub path: String,
    /// Inclusive TLDR logical-unit UTF-8 byte offset.
    pub start_byte: u64,
    /// Exclusive TLDR logical-unit UTF-8 byte offset.
    pub end_byte: u64,
}

/// Literal discriminator of a TLDR fragment source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SearchTldrSourceKind {
    /// A source fragment of the optional quick reference.
    #[serde(rename = "tldr")]
    Tldr,
}

/// A visible transformation whose exact authored byte origin is unproven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchRenderDerivedFragmentSource {
    /// Required source-kind discriminator.
    pub kind: SearchRenderDerivedSourceKind,
}

/// Literal discriminator of render-derived visible bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SearchRenderDerivedSourceKind {
    /// This is not an authored root range.
    #[serde(rename = "render-derived")]
    RenderDerived,
}

/// Navigation metadata for a Fixed fragment; producer validates it against the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchFixedFragmentSource {
    /// One-based final physical row.
    pub row: NonZeroU32,
    /// One-based final run key.
    pub run: NonZeroU32,
    /// Inclusive run-relative UTF-8 byte offset.
    pub start_byte: u64,
    /// Exclusive run-relative UTF-8 byte offset.
    pub end_byte: u64,
}

/// One complete concatenated searchable unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchTextUnit {
    /// Dense one-based response-local key.
    pub key: SearchUnitKey,
    /// Fragment keys in exact visible order.
    pub fragments: Vec<SearchFragmentKey>,
    /// One explicit join for each adjacent fragment pair.
    pub joins: Vec<SearchTextJoin>,
}

/// Exact visible text between adjacent response fragments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SearchTextJoin {
    /// Proven zero-byte contact, including a native soft wrap.
    DirectContact,
    /// Exact native-consumed authored separator between fragments.
    AuthoredSeparator {
        /// One or more source-authored ASCII spaces absent from final runs.
        text: String,
    },
    /// Structural boundary that blocks a cross-fragment match.
    HardBoundary,
    /// Unknown connection that blocks a cross-fragment match.
    Unknown,
    /// Exact bytes introduced by Flow/TLDR rendering between roots.
    RenderSeparator {
        /// Exact inserted UTF-8 text, represented only in this join.
        text: String,
    },
}

impl SearchContentProjection {
    /// Expanded UTF-8 bytes inspected when validating one unit occurrence.
    pub(crate) fn unit_expanded_len(&self, key: SearchUnitKey) -> Result<usize, &'static str> {
        let unit = self
            .units
            .get((key.get() - 1) as usize)
            .filter(|unit| unit.key == key)
            .ok_or("search match references missing unit")?;
        let mut bytes = 0usize;
        for fragment_key in &unit.fragments {
            let fragment = self
                .fragments
                .get((fragment_key.get() - 1) as usize)
                .ok_or("search unit references missing fragment")?;
            bytes = bytes
                .checked_add(fragment.text.len())
                .ok_or("search match validation work overflows")?;
        }
        for join in &unit.joins {
            if let SearchTextJoin::AuthoredSeparator { text }
            | SearchTextJoin::RenderSeparator { text } = join
            {
                bytes = bytes
                    .checked_add(text.len())
                    .ok_or("search match validation work overflows")?;
            }
        }
        Ok(bytes)
    }

    /// Check dense keys, copied-byte bounds, UTF-8 source widths and unit closure.
    /// Original snapshot-relative addresses remain the producer's obligation.
    ///
    /// # Errors
    /// Returns a malformed or oversized response-local projection.
    pub fn validate(&self) -> Result<(), &'static str> {
        let objects = self
            .fragments
            .len()
            .checked_add(self.units.len())
            .ok_or("search projection object count overflows")?;
        if objects > MAX_SEARCH_PROJECTION_OBJECTS {
            return Err("search projection exceeds object budget");
        }
        let mut bytes = 0usize;
        for (index, fragment) in self.fragments.iter().enumerate() {
            if usize::try_from(fragment.key.get()).ok() != Some(index + 1) {
                return Err("search fragments must have dense keys");
            }
            bytes = bytes
                .checked_add(validate_fragment_source(fragment)?)
                .ok_or("search projection byte count overflows")?;
        }
        let mut edges = 0usize;
        let mut expanded_bytes = 0usize;
        let mut used_fragments = vec![false; self.fragments.len()];
        for (index, unit) in self.units.iter().enumerate() {
            if usize::try_from(unit.key.get()).ok() != Some(index + 1)
                || unit.fragments.is_empty()
                || unit.joins.len() != unit.fragments.len() - 1
            {
                return Err("search unit has invalid keys or join density");
            }
            edges = edges
                .checked_add(unit.fragments.len())
                .ok_or("search projection edge count overflows")?;
            if edges > MAX_SEARCH_PROJECTION_EDGES {
                return Err("search projection exceeds edge budget");
            }
            for (position, key) in unit.fragments.iter().enumerate() {
                let fragment_index =
                    usize::try_from(key.get() - 1).map_err(|_| "search fragment key overflows")?;
                let fragment = self
                    .fragments
                    .get(fragment_index)
                    .ok_or("search unit references missing fragment")?;
                used_fragments[fragment_index] = true;
                if fragment.text.is_empty() {
                    let is_terminal = position + 1 == unit.fragments.len();
                    let has_real_fragment = unit.fragments.len() > 1;
                    let has_render_join = matches!(
                        unit.joins.last(),
                        Some(SearchTextJoin::RenderSeparator { .. })
                    );
                    if !is_terminal || !has_real_fragment || !has_render_join {
                        return Err("empty derived fragment is not a terminal render sentinel");
                    }
                }
                expanded_bytes = expanded_bytes
                    .checked_add(fragment.text.len())
                    .ok_or("search projection expansion overflows")?;
            }
            for join in &unit.joins {
                if let SearchTextJoin::RenderSeparator { text }
                | SearchTextJoin::AuthoredSeparator { text } = join
                {
                    if text.is_empty() {
                        return Err("search separator is empty");
                    }
                    if matches!(join, SearchTextJoin::AuthoredSeparator { .. })
                        && !text.bytes().all(|byte| byte == b' ')
                    {
                        return Err("search authored separator is not native ASCII space");
                    }
                    bytes = bytes
                        .checked_add(text.len())
                        .ok_or("search projection byte count overflows")?;
                    expanded_bytes = expanded_bytes
                        .checked_add(text.len())
                        .ok_or("search projection expansion overflows")?;
                }
            }
        }
        if bytes > MAX_SEARCH_PROJECTION_BYTES || expanded_bytes > MAX_SEARCH_PROJECTION_BYTES {
            return Err("search projection exceeds byte budget");
        }
        if used_fragments.iter().any(|used| !used) {
            return Err("search projection retains an unreferenced fragment");
        }
        Ok(())
    }

    /// Rebuild only one selected unit, checking its copied UTF-8 and joins.
    ///
    /// # Errors
    /// Returns an unknown unit key or allocation/length overflow.
    pub fn unit_text(&self, key: SearchUnitKey) -> Result<String, &'static str> {
        let unit = self
            .units
            .get((key.get() - 1) as usize)
            .filter(|unit| unit.key == key)
            .ok_or("search match references missing unit")?;
        let mut text = String::new();
        for (index, fragment_key) in unit.fragments.iter().enumerate() {
            if let Some(
                SearchTextJoin::RenderSeparator { text: separator }
                | SearchTextJoin::AuthoredSeparator { text: separator },
            ) = index.checked_sub(1).and_then(|prior| unit.joins.get(prior))
            {
                text.push_str(separator);
            }
            let fragment = self
                .fragments
                .get((fragment_key.get() - 1) as usize)
                .filter(|fragment| fragment.key == *fragment_key)
                .ok_or("search unit references missing fragment")?;
            text.push_str(&fragment.text);
            if text.len() > MAX_SEARCH_PROJECTION_BYTES {
                return Err("search unit exceeds byte budget");
            }
        }
        Ok(text)
    }

    /// Validate one occurrence against its complete response-local unit.
    ///
    /// # Errors
    /// Returns mismatched text, a bad coordinate or dangling display slices.
    pub fn validate_match(&self, matched: &SearchMatch) -> Result<(), &'static str> {
        let (unit_key, start, end, fixed) = match matched.location {
            SearchLocation::VisibleFlow {
                unit,
                start_scalar,
                end_scalar,
            } => (unit, start_scalar, end_scalar, false),
            SearchLocation::VisibleFixed {
                unit,
                start_scalar,
                end_scalar,
            } => (unit, start_scalar, end_scalar, true),
            SearchLocation::MarkdownArtifact { .. } => {
                return Err("artifact match must not carry a visible projection");
            }
        };
        let unit = self
            .units
            .get((unit_key.get() - 1) as usize)
            .filter(|unit| unit.key == unit_key)
            .ok_or("search match references missing unit")?;
        let text = self.unit_text(unit_key)?;
        let start = scalar_to_byte(&text, start).ok_or("search match scalar offset overflows")?;
        let end = scalar_to_byte(&text, end).ok_or("search match scalar offset overflows")?;
        if start >= end || text.get(start..end) != Some(matched.matched_text.as_str()) {
            return Err("search match does not equal its authoritative unit range");
        }
        let mut cursor = 0usize;
        let mut expected_slices = Vec::new();
        let mut has_flow = false;
        let mut has_tldr = false;
        for (index, fragment_key) in unit.fragments.iter().enumerate() {
            if index != 0 {
                let join = unit
                    .joins
                    .get(index - 1)
                    .ok_or("search unit join is missing")?;
                if (fixed && matches!(join, SearchTextJoin::RenderSeparator { .. }))
                    || (!fixed && matches!(join, SearchTextJoin::AuthoredSeparator { .. }))
                {
                    return Err("search join is invalid for this visible body family");
                }
                if matches!(join, SearchTextJoin::HardBoundary | SearchTextJoin::Unknown)
                    && start < cursor
                    && end > cursor
                {
                    return Err("search match crosses a blocked join");
                }
                if let SearchTextJoin::RenderSeparator { text }
                | SearchTextJoin::AuthoredSeparator { text } = join
                {
                    cursor = cursor
                        .checked_add(text.len())
                        .ok_or("search unit length overflows")?;
                }
            }
            let fragment_index = usize::try_from(fragment_key.get() - 1)
                .map_err(|_| "search fragment key overflows")?;
            let fragment = self
                .fragments
                .get(fragment_index)
                .ok_or("search unit references missing fragment")?;
            if fixed != matches!(&fragment.source, SearchFragmentSource::Fixed(_)) {
                return Err("search unit mixes Flow and Fixed fragment sources");
            }
            has_flow |= matches!(&fragment.source, SearchFragmentSource::Flow(_));
            has_tldr |= matches!(&fragment.source, SearchFragmentSource::Tldr(_));
            if has_flow && has_tldr {
                return Err("search unit crosses the TLDR/manual owner boundary");
            }
            let fragment_start = cursor;
            cursor = cursor
                .checked_add(fragment.text.len())
                .ok_or("search unit length overflows")?;
            let slice_start = start.max(fragment_start);
            let slice_end = end.min(cursor);
            if slice_start < slice_end {
                let relative_start = slice_start - fragment_start;
                let relative_end = slice_end - fragment_start;
                if fragment.text.get(relative_start..relative_end).is_none() {
                    return Err("search match slices a fragment inside a UTF-8 scalar");
                }
                expected_slices.push(SearchDisplaySlice {
                    fragment: *fragment_key,
                    start_scalar: fragment.text[..relative_start].chars().count() as u64,
                    end_scalar: fragment.text[..relative_end].chars().count() as u64,
                });
            }
        }
        if matched.display_slices != expected_slices {
            return Err("search display slices do not equal match/fragment intersections");
        }
        Ok(())
    }
}

fn scalar_to_byte(text: &str, scalar: u64) -> Option<usize> {
    let scalar = usize::try_from(scalar).ok()?;
    text.char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .nth(scalar)
}

fn validate_fragment_source(fragment: &SearchFragment) -> Result<usize, &'static str> {
    if fragment.text.is_empty()
        && !matches!(fragment.source, SearchFragmentSource::RenderDerived(_))
    {
        return Err("only a derived terminal sentinel may be empty");
    }
    let source_range = match &fragment.source {
        SearchFragmentSource::Flow(source) => {
            if source.location.as_ref().to_owned().is_none() {
                return Err("search Flow source location is out of bounds");
            }
            Some((source.start_byte, source.end_byte))
        }
        SearchFragmentSource::Tldr(source) => {
            if source.path != "0" {
                return Err("search TLDR source path is not the selected path");
            }
            Some((source.start_byte, source.end_byte))
        }
        SearchFragmentSource::Fixed(source) => Some((source.start_byte, source.end_byte)),
        SearchFragmentSource::RenderDerived(_) => None,
    };
    if let Some((start, end)) = source_range
        && (end <= start || end - start != fragment.text.len() as u64)
    {
        return Err("search source range does not cover fragment text");
    }
    Ok(fragment.text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OutlineNodeReference;

    #[test]
    fn authored_separator_is_exact_and_display_slices_are_intersections() {
        let one = NonZeroU32::MIN;
        let two = NonZeroU32::new(2).unwrap();
        let mut projection = SearchContentProjection {
            fragments: vec![
                SearchFragment {
                    key: one,
                    text: "a".to_owned(),
                    source: SearchFragmentSource::Fixed(SearchFixedFragmentSource {
                        row: one,
                        run: one,
                        start_byte: 0,
                        end_byte: 1,
                    }),
                },
                SearchFragment {
                    key: two,
                    text: "b".to_owned(),
                    source: SearchFragmentSource::Fixed(SearchFixedFragmentSource {
                        row: two,
                        run: two,
                        start_byte: 0,
                        end_byte: 1,
                    }),
                },
            ],
            units: vec![SearchTextUnit {
                key: one,
                fragments: vec![one, two],
                joins: vec![SearchTextJoin::AuthoredSeparator {
                    text: "   ".to_owned(),
                }],
            }],
        };
        projection.validate().unwrap();
        assert_eq!(projection.unit_text(one).unwrap(), "a   b");
        let mut matched = SearchMatch {
            ordinal: 1,
            outline: OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::DocumentRoot {
                    path: "root".into(),
                    id: mant_ir::DOCUMENT_ROOT_ID.into(),
                    title: "OVERVIEW".to_owned(),
                },
            },
            matched_text: "a   b".to_owned(),
            location: SearchLocation::VisibleFixed {
                unit: one,
                start_scalar: 0,
                end_scalar: 5,
            },
            display_slices: vec![
                SearchDisplaySlice {
                    fragment: one,
                    start_scalar: 0,
                    end_scalar: 1,
                },
                SearchDisplaySlice {
                    fragment: two,
                    start_scalar: 0,
                    end_scalar: 1,
                },
            ],
            node_source: None,
            preview: "a   b".to_owned(),
            context: Vec::new(),
        };
        projection.validate_match(&matched).unwrap();
        projection.units[0].joins[0] = SearchTextJoin::RenderSeparator {
            text: "   ".to_owned(),
        };
        assert!(projection.validate_match(&matched).is_err());
        projection.units[0].joins[0] = SearchTextJoin::AuthoredSeparator {
            text: "   ".to_owned(),
        };
        for fragment in &mut projection.fragments {
            fragment.source =
                SearchFragmentSource::RenderDerived(SearchRenderDerivedFragmentSource {
                    kind: SearchRenderDerivedSourceKind::RenderDerived,
                });
        }
        matched.location = SearchLocation::VisibleFlow {
            unit: one,
            start_scalar: 0,
            end_scalar: 5,
        };
        assert!(projection.validate_match(&matched).is_err());
        projection.units[0].joins[0] = SearchTextJoin::RenderSeparator {
            text: "   ".to_owned(),
        };
        projection.validate_match(&matched).unwrap();
        matched.display_slices.pop();
        assert!(projection.validate_match(&matched).is_err());
    }

    #[test]
    fn terminal_render_sentinel_closes_separator_without_a_display_slice() {
        let one = NonZeroU32::MIN;
        let two = NonZeroU32::new(2).unwrap();
        let projection = SearchContentProjection {
            fragments: vec![
                SearchFragment {
                    key: one,
                    text: "alpha".to_owned(),
                    source: SearchFragmentSource::RenderDerived(
                        SearchRenderDerivedFragmentSource {
                            kind: SearchRenderDerivedSourceKind::RenderDerived,
                        },
                    ),
                },
                SearchFragment {
                    key: two,
                    text: String::new(),
                    source: SearchFragmentSource::RenderDerived(
                        SearchRenderDerivedFragmentSource {
                            kind: SearchRenderDerivedSourceKind::RenderDerived,
                        },
                    ),
                },
            ],
            units: vec![SearchTextUnit {
                key: one,
                fragments: vec![one, two],
                joins: vec![SearchTextJoin::RenderSeparator { text: "\n".into() }],
            }],
        };
        projection.validate().unwrap();
        assert_eq!(projection.unit_text(one).unwrap(), "alpha\n");
        let mut malformed = projection.clone();
        malformed.units[0].fragments.swap(0, 1);
        assert!(malformed.validate().is_err());
        let mut malformed = projection;
        malformed.units[0].joins[0] = SearchTextJoin::DirectContact;
        assert!(malformed.validate().is_err());
    }
}
