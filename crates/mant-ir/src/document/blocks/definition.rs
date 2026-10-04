//! Definition owners and their separate semantic and presentation facts.
use super::{Block, Inline, LayoutHint, SourceSpan};
use crate::EntryFacts;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

mod body;
pub use body::DefinitionBodyRef;

/// Displayed terms share a description containing arbitrary blocks.
/// Sharing that content does not establish behavioral equivalence of the terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionItem {
    /// Original term-and-description owner span, not its containing list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
    /// Optional source-neutral semantic facts. The original terms and
    /// description remain authoritative regardless of fact validity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryFacts>,
    /// One or more displayed terms sharing this description, not necessarily
    /// equivalent names or interchangeable invocation forms.
    pub terms: Vec<crate::DefinitionTerm>,
    /// Block content describing the terms.
    pub description: Vec<Block>,
    /// Resolved row and word relation between authoritative head and body.
    /// This source-neutral fact remains independent of presentation geometry.
    #[serde(default, skip_serializing_if = "is_default_relation")]
    pub head_body_relation: HeadBodyRelation,
    /// Item presentation, independent of any attached semantic facts.
    #[serde(default, skip_serializing_if = "DefinitionLayout::is_empty")]
    pub layout: DefinitionLayout,
}

/// Definition-item presentation. Missing spacing inherits list compactness;
/// explicit zero spacing is a distinct, preserved source request.
///
/// The semantic source of truth is [`HeadBodyRelation`]; the column-valued
/// fields are pre-resolved hints for fixed-width terminal presentation and
/// must not drive structure decisions in other renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionLayout {
    /// Preferred first-body alignment when the item shares a separated row.
    /// A joined word seam never acquires padding from this preference.
    #[serde(default, skip_serializing_if = "is_default_alignment")]
    pub body_alignment: DefinitionBodyAlignment,
    /// Description content origin relative to the label origin. Hard and
    /// wrapped continuation lines use this origin even if a long run-in head
    /// forces the first description text further right. A pre-resolved
    /// fixed-width hint (mandoc `offset`, term.c:134-136); the generic
    /// default is four cells.
    #[serde(
        default = "default_definition_indent",
        skip_serializing_if = "is_default_definition_indent"
    )]
    pub body_indent_columns: i32,
    /// Minimum separation after a run-in term, independent of body origin.
    /// A pre-resolved fixed-width hint (mandoc `trailspace`, mdoc_term.c:801-812).
    #[serde(
        default = "default_term_gap",
        skip_serializing_if = "is_default_term_gap"
    )]
    pub min_term_gap_columns: u16,
    /// Terminal rows requested before this item when man(7) changes `.PD`.
    /// `None` inherits the containing list's compactness policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing_before_lines: Option<u16>,
}

/// Source-neutral row and word-boundary facts.
/// Column hints never override a joined word boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HeadBodyRelation {
    /// The body starts on a separate row at its own content origin.
    #[default]
    Separate,
    /// The first body row shares the final head row. Its word seam is
    /// independent from the preferred alignment and later body origins.
    Shared {
        /// Whether consumers may insert separation after the accepted head.
        word_boundary: DefinitionWordBoundary,
    },
}

// A tagged unit variant silently accepts extra fields during Serde decoding.
// Keep the public default ergonomic while closing every wire variant.
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ClosedHeadBodyRelation {
    Separate {},
    Shared {
        word_boundary: DefinitionWordBoundary,
    },
}

impl<'de> Deserialize<'de> for HeadBodyRelation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ClosedHeadBodyRelation::deserialize(deserializer)? {
            ClosedHeadBodyRelation::Separate {} => Self::Separate,
            ClosedHeadBodyRelation::Shared { word_boundary } => Self::Shared { word_boundary },
        })
    }
}

/// Word boundary at a shared definition head/body seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionWordBoundary {
    /// The accepted head and body are adjacent; add no synthetic cells.
    Joined,
    /// Apply the minimum gap and preferred first-body alignment.
    Separated,
}

/// Preferred alignment for only the first body row of a shared definition.
/// Hard/wrapped continuations and later blocks use their own body origins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionBodyAlignment {
    /// Follow the final head row, with only the resolved minimum gap.
    AfterTerm,
    /// Prefer the body content origin, without moving before the head end.
    #[default]
    Indented,
}

impl DefinitionLayout {
    /// Whether all presentation choices inherit their existing defaults.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self.body_alignment, DefinitionBodyAlignment::Indented)
            && self.spacing_before_lines.is_none()
            && self.body_indent_columns == default_definition_indent()
            && self.min_term_gap_columns == default_term_gap()
    }
}

impl HeadBodyRelation {
    /// Share the final head row behind a word separator.
    #[must_use]
    pub const fn separated() -> Self {
        Self::Shared {
            word_boundary: DefinitionWordBoundary::Separated,
        }
    }

    /// Share the final head row with no synthetic word separator or padding.
    #[must_use]
    pub const fn joined() -> Self {
        Self::Shared {
            word_boundary: DefinitionWordBoundary::Joined,
        }
    }

    /// Whether the accepted BODY follows the final HEAD without extra cells.
    #[must_use]
    pub const fn joins_without_separator(self) -> bool {
        matches!(
            self,
            Self::Shared {
                word_boundary: DefinitionWordBoundary::Joined,
                ..
            }
        )
    }

    /// Whether the relation carries the generic default.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Separate)
    }
}

impl From<bool> for HeadBodyRelation {
    fn from(shares_row: bool) -> Self {
        if shares_row {
            Self::separated()
        } else {
            Self::Separate
        }
    }
}

impl Default for DefinitionLayout {
    fn default() -> Self {
        Self {
            body_alignment: DefinitionBodyAlignment::Indented,
            body_indent_columns: default_definition_indent(),
            min_term_gap_columns: default_term_gap(),
            spacing_before_lines: None,
        }
    }
}

const fn default_definition_indent() -> i32 {
    4
}
const fn default_term_gap() -> u16 {
    1
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
const fn is_default_definition_indent(value: &i32) -> bool {
    *value == default_definition_indent()
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
const fn is_default_term_gap(value: &u16) -> bool {
    *value == default_term_gap()
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
const fn is_default_relation(value: &HeadBodyRelation) -> bool {
    value.is_default()
}
#[allow(clippy::trivially_copy_pass_by_ref)] // Serde predicate.
const fn is_default_alignment(value: &DefinitionBodyAlignment) -> bool {
    matches!(value, DefinitionBodyAlignment::Indented)
}

impl DefinitionItem {
    /// Whether the resolved body shares the final term row.
    #[must_use]
    pub const fn inline_term(&self) -> bool {
        !matches!(self.head_body_relation, HeadBodyRelation::Separate)
    }
    /// Generic default for authored definitions. Source-specific producers
    /// set [`DefinitionLayout::body_indent_columns`]; consumers must read that
    /// resolved field rather than applying this default a second time.
    pub const DESCRIPTION_INDENT_COLUMNS: u16 = 4;
}
