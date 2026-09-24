//! Source-neutral, post-device display body and checked mark references.
//!
//! `DisplaySurface::text` is the sole owned visible-body byte arena. Rows and
//! runs index it; marks retain only keys, ranges and small metadata. This
//! module does not infer formatter geometry or parse roff. The containing
//! `Document` closes typed source keys against its own `SourceTable`.

use std::{fmt, num::NonZeroU32, ops::Range};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    EntryFacts, EntryKind, EntryNameEvidence, FragmentAlias, LinkTarget, NameCase, NodeId,
    ParameterKind, SourceKey, SourceSpan,
};

// Match the existing native fixed-display geometry ceiling. A small UTF-8
// arena must not authorize an unbounded sparse row when a consumer expands
// terminal columns into cells.
const MAX_FIXED_ROW_COLUMNS: u32 = 1_048_576;
const MAX_FIXED_TOTAL_COLUMNS: u64 = 32 * 1024 * 1024;
const MAX_FIXED_TOTAL_JOIN_BYTES: u64 = 32 * 1024 * 1024;

/// A final display surface, independent of any viewport width.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplaySurface {
    /// The only owned visible-body text. Every byte belongs to exactly one run.
    pub text: String,
    /// Native physical rows in emission order, including blank rows.
    pub rows: Vec<DisplayRow>,
    /// Final, coalesced runs in row and byte-arena order.
    pub runs: Vec<DisplayRun>,
}

#[derive(Deserialize)]
#[serde(
    remote = "DisplaySurface",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct DisplaySurfaceWire {
    text: String,
    rows: Vec<DisplayRow>,
    runs: Vec<DisplayRun>,
}

impl<'de> Deserialize<'de> for DisplaySurface {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let surface = DisplaySurfaceWire::deserialize(deserializer)?;
        surface.validate().map_err(serde::de::Error::custom)?;
        Ok(surface)
    }
}

/// One final native physical row. Only the last row may omit a newline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayRow {
    /// Dense one-based key equal to this row's position.
    pub key: NonZeroU32,
    /// First run in this row, or the next run key for an empty row.
    pub first_run: NonZeroU32,
    /// Number of runs belonging to the row.
    pub run_count: u32,
    /// Final native terminal-column count, without viewport reflow.
    pub column_count: u32,
    /// Whether the device emitted a newline following this row.
    pub break_after: bool,
}

/// A final run's compact, source-neutral execution labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayLabel {
    /// Native owner identity, if one survived on these glyphs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<NonZeroU32>,
    /// Native link occurrence identity, if one survived on these glyphs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<NonZeroU32>,
    /// Typed source identity; exact authored coordinates live on marks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceKey>,
    /// Final style after font overstrike folding.
    pub style: DisplayStyle,
    /// Whether this run came from buffered body output or direct drawing.
    pub role: DisplayRole,
}

/// Final font style, not raw terminal overstrike bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayStyle {
    /// Native bold styling.
    pub bold: bool,
    /// Native underline styling.
    pub underline: bool,
}

/// Final body-output role. Header and footer content is never admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayRole {
    /// Buffered native body output.
    Body,
    /// Native direct drawing, such as table rules.
    DirectDraw,
    /// Native-confirmed terminal advance used only for layout spacing.
    Layout,
}

/// One UTF-8 run in the sole text arena.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayRun {
    /// Dense one-based key equal to this run's position.
    pub key: NonZeroU32,
    /// Owning physical row.
    pub row: NonZeroU32,
    /// Native starting terminal column, zero-based.
    pub column: u32,
    /// Native terminal-column width, including wide glyph continuations.
    pub width: u32,
    /// Inclusive UTF-8 byte offset in the arena.
    pub byte_start: u64,
    /// Number of UTF-8 bytes in this run.
    pub byte_count: u64,
    /// Surviving execution labels on exactly these bytes.
    pub label: DisplayLabel,
}

/// One half-open UTF-8 byte range in a final run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputSlice {
    /// Final run key, not a temporary buffer token.
    pub run: NonZeroU32,
    /// Inclusive run-relative byte offset.
    pub start_byte: u64,
    /// Exclusive run-relative byte offset.
    pub end_byte: u64,
}

/// Evidence for whether adjacent slices form one searchable logical text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "text",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum TextJoin {
    /// No byte lies between the slices, including a proven soft wrap.
    DirectContact,
    /// Exact native-consumed authored separator between the slices.
    ///
    /// A separator already visible inside a slice is not repeated here.
    AuthoredSeparator(String),
    /// A native structural boundary forbids a cross-slice match.
    HardBoundary,
    /// Adjacency exists but cannot license a cross-slice match.
    Unknown,
}

/// Ordered surviving slices and one join fact for each adjacent pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextSelection {
    /// Slices into the final display arena; never copied text.
    pub parts: Vec<OutputSlice>,
    /// Pairwise native join evidence, in the same order as `parts`.
    pub joins: Vec<TextJoin>,
}

/// A final zero-width location, distinct from an empty text slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DisplayPoint {
    /// UTF-8 boundary inside a final run, including either end.
    RunBoundary {
        /// Final run key.
        run: NonZeroU32,
        /// Run-relative UTF-8 byte boundary.
        byte: u64,
    },
    /// Native terminal-column boundary in an actual final body row.  It
    /// remains valid in a blank cell or gap with no run to borrow.
    RowColumn {
        /// One-based final body row key.
        row: NonZeroU32,
        /// Zero-based device-column boundary, including row end.
        column: u32,
    },
    /// Document end when no run can carry a point, including zero rows.
    DocumentEnd {
        /// Exact number of final physical rows.
        row_count: u32,
    },
}

/// Native section navigation, with title and direct body separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeadingMark {
    /// Dense one-based section key.
    pub key: NonZeroU32,
    /// Normalized document-local navigation identity, independent of the key.
    pub id: NodeId,
    /// Exact authored native target spellings retained for provenance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragment_aliases: Vec<FragmentAlias>,
    /// Native-generated target spellings, distinct from authored aliases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_fragment_aliases: Vec<FragmentAlias>,
    /// Fragments emitted for these targets after native HTML ID de-duplication.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rendered_fragment_aliases: Vec<FragmentAlias>,
    /// Root or an earlier section, not a derived depth.
    pub parent: Option<NonZeroU32>,
    /// Native heading-level hint.
    pub level_hint: u16,
    /// Final native display boundary where this section begins, including
    /// headings whose title has no surviving visible glyph.
    pub at: DisplayPoint,
    /// Surviving visible title.
    pub title: TextSelection,
    /// Direct content, excluding child sections.
    pub direct_body: TextSelection,
    /// Authored heading location when known.
    pub source: Option<SourceSpan>,
}

/// Native owner role before semantic declaration classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerRole {
    /// Definition-like head and body.
    Definition,
    /// Ordinary list item head and body.
    ListItem,
    /// A native owner of another role not yet classified.
    Other,
}

/// First significant native macro in a definition head, captured while the
/// upstream AST is alive. This is authored role evidence, not a semantic kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerHeadRole {
    /// An mdoc `Fl` option head.
    Option,
    /// An mdoc `Ev` environment-variable head.
    Environment,
    /// An mdoc `Ic` or `Cm` literal command head.
    Literal,
    /// A native declaration head whose authored syntax permits conservative
    /// whole-token lexical classification; no name is implied by this hint.
    Lexical,
}

/// One native candidate owner, not a fabricated semantic entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerMark {
    /// Dense one-based owner key.
    pub key: NonZeroU32,
    /// Normalized document-local identity, distinct from the native key.
    pub id: NodeId,
    /// Root or an earlier enclosing owner.
    pub parent: Option<NonZeroU32>,
    /// Enclosing section, if known.
    pub section: Option<NonZeroU32>,
    /// Native owner kind before classification.
    pub role: OwnerRole,
    /// Native first-head role, if one survived the head AST boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_role: Option<OwnerHeadRole>,
    /// Conservative rendered prefix of the first native head macro's own
    /// operand. The final display selection and name remain separately
    /// checked; this is evidence, not copied body content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_role_prefix: Option<String>,
    /// Checked semantic facts referring only to this owner's final display
    /// selections. The display surface remains the sole text owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryFacts<TextSelection>>,
    /// Direct visible head, possibly empty.
    pub head: TextSelection,
    /// Direct visible body, excluding child owners.
    pub direct_body: TextSelection,
    /// Position for a structurally empty owner.
    pub empty_point: Option<DisplayPoint>,
    /// Authored owner location when known.
    pub source: Option<SourceSpan>,
}

impl OwnerMark {
    /// Whether the surviving head is one complete evidence-backed form.
    /// Unknown or structural joins never license a selectable entry name.
    #[must_use]
    pub fn has_complete_form(&self) -> bool {
        self.role == OwnerRole::Definition
            && !self.head.parts.is_empty()
            && self.head.joins.iter().all(|join| {
                matches!(
                    join,
                    TextJoin::DirectContact | TextJoin::AuthoredSeparator(_)
                )
            })
    }
}

impl FixedBody {
    /// Borrow only the current conservative Fixed facts whose form, name and
    /// lexical binding close against this owner's surviving native head.
    /// Recheck at read time: an in-memory `Document` can be changed after its
    /// deserialization guard ran.
    pub(crate) fn validated_entry<'a>(
        &self,
        owner: &'a OwnerMark,
    ) -> Option<&'a EntryFacts<TextSelection>> {
        let entry = owner.entry.as_ref()?;
        let form = self.owner_complete_form(owner)?;
        let [only_form] = entry.forms.as_slice() else {
            return None;
        };
        let [only_name] = entry.names.as_slice() else {
            return None;
        };
        let [binding] = entry.name_bindings.as_slice() else {
            return None;
        };
        let binding_matches = match (owner.head_role, entry.kind, binding.evidence) {
            (_, EntryKind::Term, EntryNameEvidence::Lexical) => {
                only_name == &form
                    && binding.occurrences.as_slice() == std::slice::from_ref(&owner.head)
            }
            (
                Some(OwnerHeadRole::Lexical),
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                EntryNameEvidence::Lexical,
            ) => {
                only_name == &form
                    && crate::lexical_option_token(only_name)
                    && binding.occurrences.as_slice() == std::slice::from_ref(&owner.head)
            }
            (
                Some(OwnerHeadRole::Option),
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                EntryNameEvidence::NativeMarkup,
            )
            | (
                Some(OwnerHeadRole::Environment),
                EntryKind::EnvironmentVariable,
                EntryNameEvidence::NativeMarkup,
            ) => {
                let start = form.len() - form.trim_start().len();
                let end = start.checked_add(only_name.len())?;
                let role_prefix = owner.head_role_prefix.as_deref()?;
                let role_proves_name = match entry.kind {
                    EntryKind::EnvironmentVariable => {
                        crate::environment_variable_alias(role_prefix).as_deref()
                            == Some(only_name.as_str())
                    }
                    _ => role_prefix == only_name && crate::native_option_token(only_name),
                };
                role_proves_name
                    && form.trim_start().starts_with(role_prefix)
                    && form.get(start..end) == Some(only_name.as_str())
                    && self.selection_subrange(&owner.head, start..end).as_ref()
                        == binding.occurrences.first()
                    && binding.occurrences.len() == 1
            }
            _ => false,
        };
        (entry.id == owner.id
            && binding_matches
            && entry.case == NameCase::Sensitive
            && only_form == &owner.head
            && binding.name == 0
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none())
        .then_some(entry)
    }

    /// Project a checked final-display selection into logical text without
    /// copying the surface into a second stored body.
    #[must_use]
    pub fn selection_text(&self, selection: &TextSelection) -> Option<String> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return None;
        }
        let mut text = String::new();
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator) => text.push_str(separator),
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let run = self.surface.run_text(part.run)?;
            let start = usize::try_from(part.start_byte).ok()?;
            let end = usize::try_from(part.end_byte).ok()?;
            text.push_str(run.get(start..end)?);
        }
        Some(text)
    }

    /// Map a logical UTF-8 range back to surviving display slices. A range
    /// touching an authored separator has no final glyph to bind and is
    /// rejected; no byte or cell coordinate is inferred from layout.
    #[must_use]
    pub fn selection_subrange(
        &self,
        selection: &TextSelection,
        range: Range<usize>,
    ) -> Option<TextSelection> {
        let logical = self.selection_text(selection)?;
        if range.start >= range.end || logical.get(range.clone()).is_none() {
            return None;
        }
        let mut cursor = 0usize;
        let mut parts = Vec::new();
        let mut joins = Vec::new();
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator) => {
                        let end = cursor.checked_add(separator.len())?;
                        if range.start < end && cursor < range.end {
                            return None;
                        }
                        cursor = end;
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let length = usize::try_from(part.end_byte.checked_sub(part.start_byte)?).ok()?;
            let end = cursor.checked_add(length)?;
            let start_in_part = range.start.max(cursor);
            let end_in_part = range.end.min(end);
            if start_in_part < end_in_part {
                let start_byte = part
                    .start_byte
                    .checked_add(u64::try_from(start_in_part - cursor).ok()?)?;
                let end_byte = part
                    .start_byte
                    .checked_add(u64::try_from(end_in_part - cursor).ok()?)?;
                self.surface
                    .run_text(part.run)?
                    .get(usize::try_from(start_byte).ok()?..usize::try_from(end_byte).ok()?)?;
                if !parts.is_empty() {
                    joins.push(selection.joins[index - 1].clone());
                }
                parts.push(OutputSlice {
                    run: part.run,
                    start_byte,
                    end_byte,
                });
            }
            cursor = end;
        }
        (cursor == logical.len() && !parts.is_empty()).then_some(TextSelection { parts, joins })
    }

    /// Read one complete surviving definition head without inferring bytes
    /// from neighboring rows, owners or unknown native joins.
    #[must_use]
    pub fn owner_complete_form(&self, owner: &OwnerMark) -> Option<String> {
        if !owner.has_complete_form() {
            return None;
        }
        let form = self.selection_text(&owner.head)?;
        (!form.trim().is_empty()).then_some(form)
    }
}

/// One native link macro instance, independent of its visible slice count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkMark {
    /// Dense one-based occurrence key.
    pub key: NonZeroU32,
    /// Typed destination decoded while the native source is still available.
    /// `None` preserves a real link macro instance that rendered without href.
    #[schemars(with = "RequiredNullableLinkTarget")]
    pub target: Option<LinkTarget>,
    /// Surviving visible label slices; no-href instances are not clickable.
    pub label: TextSelection,
    /// Authored macro location when known.
    pub source: Option<SourceSpan>,
}

// An omitted `target` is not the same as a native macro with no href.  Keep
// explicit JSON null for the latter while preserving `Option<LinkTarget>` in
// the typed IR.  A non-Option wire field makes Serde reject missing target.
#[derive(JsonSchema)]
#[schemars(transparent)]
struct RequiredNullableLinkTarget(Option<LinkTarget>);

impl<'de> Deserialize<'de> for RequiredNullableLinkTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RequiredNullableLinkTarget;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an explicit link target or null")
            }

            fn visit_newtype_struct<D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                Option::<LinkTarget>::deserialize(deserializer).map(RequiredNullableLinkTarget)
            }
        }

        // Serde's missing-field adapter supplies `None` to deserialize_option,
        // so use the newtype entry point: it rejects an omitted field while
        // preserving explicit JSON null as a real no-href occurrence.
        deserializer.deserialize_newtype_struct("RequiredNullableLinkTarget", Visitor)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LinkMarkWire {
    key: NonZeroU32,
    target: RequiredNullableLinkTarget,
    label: TextSelection,
    source: Option<SourceSpan>,
}

impl<'de> Deserialize<'de> for LinkMark {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = LinkMarkWire::deserialize(deserializer)?;
        Ok(Self {
            key: wire.key,
            target: wire.target.0,
            label: wire.label,
            source: wire.source,
        })
    }
}

/// One authored native anchor at its final, zero-width display position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnchorMark {
    /// Dense one-based declaration key.
    pub key: NonZeroU32,
    /// Normalized document-local identity, independent of authored spelling.
    pub id: NodeId,
    /// Enclosing native section, if the declaration was made within one.
    pub section: Option<NonZeroU32>,
    /// Authored declaration spelling, not a copied visible body.
    pub name: String,
    /// Fragment emitted after native HTML ID de-duplication.
    pub rendered_fragment: FragmentAlias,
    /// Whether the retained target originated in an explicit `.Tg` request.
    /// Generated native tags remain addressable but are not authored aliases.
    pub authored: bool,
    /// Final location after any native target migration.
    pub at: DisplayPoint,
    /// Original authored declaration location, distinct from `at`.
    pub source: Option<SourceSpan>,
}

/// Native region family useful for content boundaries and joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RegionKind {
    /// Direct body text outside an explicit section or owner.
    Unsectioned,
    /// Heading title.
    HeadingTitle,
    /// Heading direct body.
    HeadingBody,
    /// Owner term or label.
    OwnerHead,
    /// Owner direct body.
    OwnerBody,
    /// List container.
    List,
    /// Literal or no-fill content.
    Literal,
    /// Native table span.
    TableSpan,
    /// Native table cell.
    TableCell,
    /// Native equation region.
    Equation,
    /// Generated physical-line margin glyph, outside semantic heads/titles.
    Margin,
}

/// One native region without a parallel text buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegionMark {
    /// Dense one-based region key.
    pub key: NonZeroU32,
    /// Root or an earlier enclosing region.
    pub parent: Option<NonZeroU32>,
    /// Owning native candidate, when present.
    pub owner: Option<NonZeroU32>,
    /// Enclosing native section, including ownerless table and literal regions.
    pub section: Option<NonZeroU32>,
    /// Region family.
    pub kind: RegionKind,
    /// Final visible contents, possibly empty.
    pub selection: TextSelection,
    /// Final boundary when visible contents are empty.
    pub empty_point: Option<DisplayPoint>,
    /// Authored region location when known.
    pub source: Option<SourceSpan>,
}

/// The sole owned Fixed arm of a future exclusive document body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedBody {
    /// Final native display surface; marks contain no visible-body copy.
    pub surface: DisplaySurface,
    /// Native section marks in source identity order.
    pub headings: Vec<HeadingMark>,
    /// Native owner marks in identity order.
    pub owners: Vec<OwnerMark>,
    /// Native link occurrences in identity order.
    pub links: Vec<LinkMark>,
    /// Native anchor declarations in identity order.
    pub anchors: Vec<AnchorMark>,
    /// Native content boundary marks in identity order.
    pub regions: Vec<RegionMark>,
}

#[derive(Deserialize)]
#[serde(remote = "FixedBody", rename_all = "camelCase", deny_unknown_fields)]
struct FixedBodyWire {
    surface: DisplaySurface,
    headings: Vec<HeadingMark>,
    owners: Vec<OwnerMark>,
    links: Vec<LinkMark>,
    anchors: Vec<AnchorMark>,
    regions: Vec<RegionMark>,
}

impl<'de> Deserialize<'de> for FixedBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let body = FixedBodyWire::deserialize(deserializer)?;
        body.validate().map_err(serde::de::Error::custom)?;
        Ok(body)
    }
}

mod validation;
pub use validation::FixedBodyError;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod hardening_tests;
