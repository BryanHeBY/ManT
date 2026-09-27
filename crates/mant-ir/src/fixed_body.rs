//! Source-neutral, post-device display body and checked mark references.
//!
//! `DisplaySurface::text` is the sole owned visible-body byte arena. Rows and
//! runs index it; marks retain only keys, ranges and small metadata. This
//! module does not infer formatter geometry or parse roff. The containing
//! `Document` closes typed source keys against its own `SourceTable`.

use std::{fmt, num::NonZeroU32};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{EntryFacts, FragmentAlias, LinkTarget, NodeId, SourceKey, SourceSpan};

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
    /// Exact formatter-generated ASCII spaces consumed between final runs.
    /// These bytes have no authored source span and are never inferred from
    /// row or terminal-column geometry.
    GeneratedSeparator(String),
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
    /// Source identity when no authored heading position can be proved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
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

/// Native macro role in a definition head, captured while the upstream AST is
/// alive. The typed owner retains the first surviving component role when an
/// earlier authored instance emits no glyph. This is role evidence, not a
/// semantic kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerHeadRole {
    /// An authored option head, such as mdoc `Fl`.
    Option,
    /// An mdoc `Ev` environment-variable head.
    Environment,
    /// An authored mdoc `Va` variable declaration head.
    Variable,
    /// An authored mdoc `Dv` defined-variable head; this role alone does not
    /// imply a distinct public entry kind.
    DefinedVariable,
    /// An mdoc `Ic` or `Cm` literal command head.
    Literal,
    /// An authored mdoc `Ar` argument instance within a definition HEAD.
    /// It can delimit a preceding key but never names an entry by itself.
    Argument,
    /// A native declaration head whose authored syntax permits conservative
    /// whole-token lexical classification; no name is implied by this hint.
    Lexical,
}

/// One native macro instance whose glyphs survived inside an owner's HEAD.
/// Its role is authored evidence; it does not itself classify an entry or
/// split a visible form. The selection borrows the sole Fixed display surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerHeadComponent {
    /// Native macro role before source-neutral declaration recognition.
    pub role: OwnerHeadRole,
    /// Exact final glyphs from that one macro instance, possibly empty.
    pub selection: TextSelection,
    /// Authored macro location, separate from generated final glyphs.
    pub source: Option<SourceSpan>,
    /// Source identity when the macro's authored position cannot be proved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
}

impl OwnerHeadComponent {
    /// An expanded macro still proves its native role when its source key is
    /// known, even if no authored line and column survive reparsing.
    const fn has_source_identity(&self) -> bool {
        self.source.is_some() || self.source_key.is_some()
    }
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
    /// Earlier direct man definition sibling proved by the native AST and
    /// flow boundary. This is reading-context evidence, not owned body text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preceding_owner: Option<NonZeroU32>,
    /// The native PP presentation candidate bit. It is not itself semantic
    /// evidence; an explicit, readable RS relation and complete head are
    /// required before this owner may carry an entry.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hanging_candidate: bool,
    /// Direct checked key of the ownerless RS region, when that candidate was
    /// upgraded by a positive native indentation. Direct text or an enclosed
    /// definition head must separately prove readable description content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hanging_continuation: Option<NonZeroU32>,
    /// Direct native `OwnerHead` child of the continuation, when an `RS` body
    /// consists of a nested definition rather than directly emitted text.
    /// This is structural description evidence, not another text owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hanging_nested_head: Option<NonZeroU32>,
    /// Enclosing section, if known.
    pub section: Option<NonZeroU32>,
    /// Native owner kind before classification.
    pub role: OwnerRole,
    /// Native first-head role, if any. Projection replaces a zero-glyph first
    /// instance with the first visible component role; a missing raw role is
    /// never inferred from a later component. Read-time proof checks this
    /// persisted role against surviving component selections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_role: Option<OwnerHeadRole>,
    /// Optional native role prefix when an authored macro supplies one.
    /// Man lexical heads use the complete checked display selection instead
    /// of freezing a prefix from their raw roff operand spelling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_role_prefix: Option<String>,
    /// Native evidence that a lexical HEAD may remain a plain TP/TQ term
    /// when its complete displayed spelling contains no accepted option.
    /// A broad IP or alternating-font candidate does not have this fallback.
    #[serde(default, skip_serializing_if = "is_false")]
    pub lexical_term_witness: bool,
    /// Native HEAD macro evidence in authoring order; not inferred from font.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub head_components: Vec<OwnerHeadComponent>,
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
    /// Source identity when no authored owner position can be proved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
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
                    TextJoin::DirectContact
                        | TextJoin::AuthoredSeparator(_)
                        | TextJoin::GeneratedSeparator(_)
                )
            })
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde's skip callback takes &T.
fn is_false(value: &bool) -> bool {
    !*value
}

mod evidence;
use evidence::component_part_ranges;
mod literal_boundaries;
pub use evidence::{FixedEntryPass, FixedNonOptionLimit, FixedNonOptionRecognition};

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
    /// Source identity when no authored link position can be proved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
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
    #[serde(default)]
    source_key: Option<SourceKey>,
}

impl<'de> Deserialize<'de> for LinkMark {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = LinkMarkWire::deserialize(deserializer)?;
        Ok(Self {
            key: wire.key,
            target: wire.target.0,
            label: wire.label,
            source: wire.source,
            source_key: wire.source_key,
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
    /// Source identity when no authored declaration position can be proved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
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
    /// Native paragraph or indented block continuing an earlier definition.
    HangingContinuation,
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
    /// Earlier definition whose reading body includes this ownerless native
    /// region. This is not a new owner or a transfer of source ownership.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_of: Option<NonZeroU32>,
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
    /// Source identity when no authored region position can be proved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<SourceKey>,
}

/// The sole owned Fixed arm of a future exclusive document body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedBody {
    /// Final native display surface; marks contain no visible-body copy.
    pub surface: DisplaySurface,
    /// Weak configuration-manual hint derived from the document's normalized
    /// title/names. It never creates an entry without a complete native head;
    /// the containing Document rechecks it against current metadata.
    #[serde(default)]
    pub root_configuration_hint: bool,
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
    #[serde(default)]
    root_configuration_hint: bool,
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
