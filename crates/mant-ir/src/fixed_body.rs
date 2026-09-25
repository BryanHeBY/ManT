//! Source-neutral, post-device display body and checked mark references.
//!
//! `DisplaySurface::text` is the sole owned visible-body byte arena. Rows and
//! runs index it; marks retain only keys, ranges and small metadata. This
//! module does not infer formatter geometry or parse roff. The containing
//! `Document` closes typed source keys against its own `SourceTable`.

use std::{collections::BTreeSet, fmt, num::NonZeroU32, ops::Range};

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

type StyledArgumentScan = (Vec<(String, Range<usize>)>, usize);

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

/// First significant native macro in a definition head, captured while the
/// upstream AST is alive. This is authored role evidence, not a semantic kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerHeadRole {
    /// An authored option head, such as mdoc `Fl`.
    Option,
    /// An mdoc `Ev` environment-variable head.
    Environment,
    /// An mdoc `Ic` or `Cm` literal command head.
    Literal,
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
    /// Native first-head role, if one survived the head AST boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_role: Option<OwnerHeadRole>,
    /// Optional native role prefix when an authored macro supplies one.
    /// Man lexical heads use the complete checked display selection instead
    /// of freezing a prefix from their raw roff operand spelling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_role_prefix: Option<String>,
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

/// Map each native component to a contiguous HEAD part interval. A component
/// may leave unrelated HEAD glyphs outside its interval, but cannot skip a
/// part or substitute a different join inside its own visible spelling.
fn component_part_ranges(
    head: &TextSelection,
    components: &[OwnerHeadComponent],
) -> Option<Vec<Range<usize>>> {
    let mut ranges = Vec::with_capacity(components.len());
    let mut cursor = 0;
    for component in components {
        let first = component.selection.parts.first()?;
        while head.parts.get(cursor).is_some_and(|part| part != first) {
            cursor += 1;
        }
        let end = cursor.checked_add(component.selection.parts.len())?;
        if head.parts.get(cursor..end)? != component.selection.parts
            || head.joins.get(cursor..end.saturating_sub(1))? != component.selection.joins
        {
            return None;
        }
        ranges.push(cursor..end);
        cursor = end;
    }
    Some(ranges)
}

fn group_name_occurrences(
    found: impl IntoIterator<Item = (String, TextSelection)>,
) -> Vec<(String, Vec<TextSelection>)> {
    let mut indices: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut grouped: Vec<(String, Vec<TextSelection>)> = Vec::new();
    for (name, selection) in found {
        if let Some(&index) = indices.get(&name) {
            grouped[index].1.push(selection);
        } else {
            indices.insert(name.clone(), grouped.len());
            grouped.push((name, vec![selection]));
        }
    }
    grouped
}

impl FixedBody {
    /// Check one PP/RS declaration against its direct native continuation,
    /// without scanning unrelated owners or regions. The paragraph head must
    /// be complete syntax; a textless RS qualifies only with a checked nested
    /// definition head, not merely a table or empty layout scope.
    #[must_use]
    pub fn hanging_declaration_ready(&self, owner: &OwnerMark) -> bool {
        let Some(region) = owner
            .hanging_continuation
            .and_then(|key| self.regions.get((key.get() - 1) as usize))
        else {
            return false;
        };
        let nested_head_ready = owner
            .hanging_nested_head
            .and_then(|key| self.regions.get((key.get() - 1) as usize))
            .is_some_and(|head| {
                head.kind == RegionKind::OwnerHead
                    && head.parent == Some(region.key)
                    && head.section == owner.section
                    && head.owner.is_some_and(|nested| {
                        nested != owner.key
                            && self
                                .owners
                                .get((nested.get() - 1) as usize)
                                .is_some_and(|child| {
                                    child.section == owner.section
                                        && child.parent == owner.parent
                                        && child.role == OwnerRole::Definition
                                        && child.head == head.selection
                                })
                    })
            });
        owner.hanging_candidate
            && owner.role == OwnerRole::Definition
            && owner.head_role == Some(OwnerHeadRole::Lexical)
            && region.kind == RegionKind::HangingContinuation
            && region.continuation_of == Some(owner.key)
            && region.owner.is_none()
            && region.section == owner.section
            && (owner.hanging_nested_head.is_none() || nested_head_ready)
            && (!region.selection.parts.is_empty() || nested_head_ready)
            && self
                .owner_complete_form(owner)
                .is_some_and(|form| crate::is_complete_hanging_option_head(&form))
    }

    /// Read option spellings only from source-identified bold operands of an
    /// alternating-font man `HEAD`. `man_term.c::pre_alternate` prints adjacent
    /// operands without inserting a space; their final glyphs alone cannot
    /// recover the boundary between a name and an italic argument.
    #[must_use]
    pub fn lexical_component_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        if owner.head_role != Some(OwnerHeadRole::Lexical)
            || owner.head_role_prefix.is_some()
            || owner.head_components.is_empty()
        {
            return None;
        }
        let form = self.owner_complete_form(owner)?;
        let ranges = component_part_ranges(&owner.head, &owner.head_components)?;
        let byte_ranges = self.component_byte_ranges(&owner.head, &ranges)?;
        let mut names = Vec::new();
        for (component, outer) in owner.head_components.iter().zip(byte_ranges) {
            if component.role != OwnerHeadRole::Lexical || !component.has_source_identity() {
                return None;
            }
            let text = self.selection_text(&component.selection)?;
            for (name, inner) in crate::literal_option_names(&text) {
                if names.len() == 64 {
                    return None;
                }
                let selection = self.selection_subrange(&component.selection, inner.clone())?;
                if self.selection_text(&selection).as_deref() != Some(name.as_str()) {
                    return None;
                }
                names.push((
                    name,
                    selection,
                    outer.start + inner.start..outer.start + inner.end,
                ));
            }
        }
        self.checked_lexical_names(&form, names)
            .filter(|names| !names.is_empty())
    }

    /// Select names from one complete native lexical head. Both source-neutral
    /// spellings and native bold components pass the same declaration segments
    /// and final-display parameter boundary before they become names.
    #[must_use]
    pub fn lexical_literal_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        if owner.head_role != Some(OwnerHeadRole::Lexical) {
            return None;
        }
        let form = self.owner_complete_form(owner)?;
        let (styled, _) = self.lexical_styled_argument_names(owner, &form)?;
        let mut candidates = crate::literal_option_names(&form);
        // A displayed operand after a complete initial option is not another
        // name. The native HEAD role, rather than the raw roff spelling, is
        // the independent evidence for this first declaration.
        if candidates.is_empty() {
            let leading = form.len() - form.trim_start().len();
            if let Some(name) = crate::option_prefix(&form[leading..])
                && crate::lexical_option_token(name)
                && form[leading + name.len()..]
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_whitespace() || character == '=')
            {
                candidates.push((name.to_owned(), leading..leading + name.len()));
            }
        }
        for (name, range) in styled {
            candidates.retain(|(_, existing)| existing.start != range.start);
            candidates.push((name, range));
        }
        candidates.sort_by_key(|(_, range)| range.start);
        let mut names = Vec::new();
        for (name, range) in candidates {
            let selection = self.selection_subrange(&owner.head, range.clone())?;
            if self.selection_text(&selection).as_deref() != Some(name.as_str()) {
                return None;
            }
            names.push((name, selection, range));
        }
        let names = self.checked_lexical_names(&form, names)?;
        // The parser-alive prefix is a candidate for the first declaration,
        // not permission to ignore the rest of the native HEAD.  Keep it
        // tied to the same final glyphs when it was recorded.
        if let Some(prefix) = owner.head_role_prefix.as_deref()
            && names.first().map(|(name, _, _)| name.as_str()) != Some(prefix)
        {
            return None;
        }
        Some(names)
    }

    /// One checked result for producer, validator, and query positions.
    /// Native components supply additional source-identified evidence, but
    /// cannot truncate names found in the complete visible HEAD.
    #[must_use]
    pub fn lexical_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        let literal = self.lexical_literal_names(owner);
        let components = self.lexical_component_names(owner);
        if literal.is_none() && components.is_none() {
            return None;
        }
        let mut names = literal.unwrap_or_default();
        for candidate in components.unwrap_or_default() {
            if let Some(existing) = names.iter().find(|(_, _, range)| *range == candidate.2) {
                if existing.0 != candidate.0 || existing.1 != candidate.1 {
                    return None;
                }
            } else {
                names.push(candidate);
            }
        }
        if names.len() > 64 {
            return None;
        }
        names.sort_by_key(|(_, _, range)| range.start);
        if names.windows(2).any(|pair| pair[0].2.end > pair[1].2.start) {
            return None;
        }
        if names.is_empty() {
            return Some(names);
        }
        self.checked_lexical_names(&self.owner_complete_form(owner)?, names)
    }

    /// An immediately underlined suffix is a native parameter boundary,
    /// even when `term.c::term_word()` prints it in direct contact with the
    /// preceding option (`-L` followed by italic `dir`).  Recover only a
    /// complete option before that boundary; ordinary roman suffixes and
    /// a wholly underlined option are not additional declarations.
    fn lexical_styled_argument_names(
        &self,
        owner: &OwnerMark,
        form: &str,
    ) -> Option<StyledArgumentScan> {
        let segments = crate::entry::literal_declaration_ranges(form);
        let mut candidates = Vec::new();
        let mut segment_index = 0;
        let mut examined_segment = None;
        let mut attempts = 0;
        let mut offset = 0usize;
        for (index, part) in owner.head.parts.iter().enumerate() {
            if index != 0 {
                offset = offset.checked_add(match &owner.head.joins[index - 1] {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            let start = offset;
            let run = self.surface.runs.get((part.run.get() - 1) as usize)?;
            let length = usize::try_from(part.end_byte.checked_sub(part.start_byte)?).ok()?;
            offset = offset.checked_add(length)?;
            if !run.label.style.underline || run.label.style.bold {
                continue;
            }
            while segments
                .get(segment_index)
                .is_some_and(|segment| segment.end < start)
            {
                segment_index += 1;
            }
            let Some(segment) = segments.get(segment_index) else {
                continue;
            };
            // A later underlined run in the same declaration cannot reveal
            // a new leading name: its prefix already contains the first
            // underlined argument.  More importantly, never rescan that
            // growing prefix once per font fragment.
            if examined_segment == Some(segment_index) {
                continue;
            }
            examined_segment = Some(segment_index);
            attempts += 1;
            if index == 0 || owner.head.joins[index - 1] != TextJoin::DirectContact {
                continue;
            }
            if start <= segment.start || start > segment.end {
                continue;
            }
            let before = form.get(segment.start..start)?.trim_start();
            if !crate::lexical_option_token(before) {
                continue;
            }
            let name_start = start.checked_sub(before.len())?;
            let range = name_start..start;
            let selection = self.selection_subrange(&owner.head, range.clone())?;
            if selection.parts.iter().any(|part| {
                self.surface
                    .runs
                    .get((part.run.get() - 1) as usize)
                    .is_some_and(|run| run.label.style.underline && !run.label.style.bold)
            }) {
                continue;
            }
            if candidates.len() == 64 {
                return None;
            }
            candidates.push((before.to_owned(), range));
        }
        (offset == form.len()).then_some((candidates, attempts))
    }

    fn checked_lexical_names(
        &self,
        form: &str,
        candidates: Vec<(String, TextSelection, std::ops::Range<usize>)>,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        let segments = crate::entry::literal_declaration_ranges(form);
        let literal = crate::literal_option_names(form);
        let mut names = Vec::new();
        let mut blocked_segment = None;
        let mut segment_cursor = 0;
        let mut style_rejected = false;
        for (name, selection, range) in candidates {
            while segments
                .get(segment_cursor)
                .is_some_and(|segment| segment.end < range.start)
            {
                segment_cursor += 1;
            }
            let segment = segments.get(segment_cursor)?;
            if segment.start > range.start || range.end > segment.end {
                return None;
            }
            if blocked_segment == Some(segment_cursor) {
                continue;
            }
            let leading = form.get(segment.start..range.start)?;
            let at_segment_start = leading
                .chars()
                .all(|character| character.is_whitespace() || matches!(character, '[' | '{' | '('));
            if !at_segment_start && !literal.iter().any(|(_, found)| *found == range) {
                continue;
            }
            // Underline is final native display evidence, not a recovered
            // italic opcode. Conservatively treat an underlined nonbold name
            // as a parameter within this segment only; a later independently
            // delimited declaration remains eligible.
            if selection.parts.iter().any(|part| {
                self.surface
                    .runs
                    .get((part.run.get() - 1) as usize)
                    .is_some_and(|run| run.label.style.underline && !run.label.style.bold)
            }) {
                style_rejected = true;
                blocked_segment = Some(segment_cursor);
                continue;
            }
            // A font change inside one spelling is an operand boundary, not
            // permission to concatenate bold and nonbold glyphs into a new
            // option. An independently delimited roman name remains eligible.
            let mut bold = None;
            for part in &selection.parts {
                let part_bold = self
                    .surface
                    .runs
                    .get((part.run.get() - 1) as usize)?
                    .label
                    .style
                    .bold;
                if bold
                    .replace(part_bold)
                    .is_some_and(|previous| previous != part_bold)
                {
                    style_rejected = true;
                    blocked_segment = Some(segment_cursor);
                    break;
                }
            }
            if blocked_segment == Some(segment_cursor) {
                continue;
            }
            names.push((name, selection, range));
        }
        // An empty result is meaningful only when final italic styling
        // rejected a candidate: the producer must not reclassify that same
        // visible spelling through the generic identity fallback. A head
        // with no option candidate remains eligible as an ordinary Term.
        (!names.is_empty() || style_rejected).then_some(names)
    }

    /// Bind each source-identified `Fl` macro to its own final glyphs without
    /// requiring those names to cover the entire `HEAD`.
    /// `mdoc_macro.c::blk_full()` keeps `Ar` operands in the same `It` `HEAD`,
    /// and `mdoc_term.c::termp_fl_pre()` prints every distinct `Fl` invocation.
    /// The complete `HEAD` remains one form; these components prove names, not
    /// separate forms or alias relationships.
    #[must_use]
    pub fn option_component_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        if owner.head_role != Some(OwnerHeadRole::Option)
            || owner.head_components.len() < 2
            || self.owner_complete_form(owner).is_none()
        {
            return None;
        }
        let ranges = component_part_ranges(&owner.head, &owner.head_components)?;
        let byte_ranges = self.component_byte_ranges(&owner.head, &ranges)?;
        let mut names = Vec::with_capacity(owner.head_components.len());
        for (component, range) in owner.head_components.iter().zip(byte_ranges) {
            if component.role != OwnerHeadRole::Option
                || component.selection.parts.is_empty()
                || !component.has_source_identity()
            {
                return None;
            }
            let text = self.selection_text(&component.selection)?;
            if !crate::native_option_token(&text) {
                return None;
            }
            names.push((text, component.selection.clone(), range));
        }
        Some(names)
    }

    fn component_byte_ranges(
        &self,
        head: &TextSelection,
        part_ranges: &[std::ops::Range<usize>],
    ) -> Option<Vec<std::ops::Range<usize>>> {
        let mut output = Vec::with_capacity(part_ranges.len());
        let mut offset = 0usize;
        let mut component = 0usize;
        let mut start = 0usize;
        for (index, part) in head.parts.iter().enumerate() {
            if index != 0 {
                offset = offset.checked_add(match &head.joins[index - 1] {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            if part_ranges
                .get(component)
                .is_some_and(|range| range.start == index)
            {
                start = offset;
            }
            let run = self.surface.run_text(part.run)?;
            let text = run.get(
                usize::try_from(part.start_byte).ok()?..usize::try_from(part.end_byte).ok()?,
            )?;
            offset = offset.checked_add(text.len())?;
            if part_ranges
                .get(component)
                .is_some_and(|range| range.end == index + 1)
            {
                output.push(start..offset);
                component += 1;
            }
        }
        (component == part_ranges.len()).then_some(output)
    }

    /// Return multiple exact option declarations only when distinct native
    /// `Fl` macro instances cover every non-separator glyph in one complete
    /// definition HEAD. Typography or punctuation alone never creates names.
    #[must_use]
    pub fn option_component_forms(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection)>> {
        if owner.role != OwnerRole::Definition || owner.head_components.len() < 2 {
            return None;
        }
        let mut forms = Vec::with_capacity(owner.head_components.len());
        let mut seen = BTreeSet::new();
        for component in &owner.head_components {
            if component.role != OwnerHeadRole::Option
                || component.selection.parts.is_empty()
                || !component.has_source_identity()
            {
                return None;
            }
            let text = self.selection_text(&component.selection)?;
            if !crate::native_option_token(&text) || !seen.insert(text.clone()) {
                return None;
            }
            forms.push((text, component.selection.clone()));
        }
        let mut selected = owner
            .head_components
            .iter()
            .flat_map(|component| component.selection.parts.iter())
            .peekable();
        for part in &owner.head.parts {
            if selected.peek().is_some_and(|component| *component == part) {
                selected.next();
                continue;
            }
            let run = self.surface.run_text(part.run)?;
            let start = usize::try_from(part.start_byte).ok()?;
            let end = usize::try_from(part.end_byte).ok()?;
            if !run
                .get(start..end)?
                .bytes()
                .all(|byte| byte.is_ascii_whitespace() || matches!(byte, b',' | b'|' | b'/'))
            {
                return None;
            }
        }
        selected.next().is_none().then_some(forms)
    }

    /// Borrow only the current conservative Fixed facts whose form, name and
    /// lexical binding close against this owner's surviving native head.
    /// Recheck at read time: an in-memory `Document` can be changed after its
    /// deserialization guard ran.
    #[allow(clippy::too_many_lines)] // One read-time closure of all entry fact variants.
    pub(crate) fn validated_entry<'a>(
        &self,
        owner: &'a OwnerMark,
    ) -> Option<&'a EntryFacts<TextSelection>> {
        let entry = owner.entry.as_ref()?;
        if owner.hanging_candidate {
            if !self.hanging_declaration_ready(owner) {
                return None;
            }
        } else if owner.hanging_continuation.is_some() || owner.hanging_nested_head.is_some() {
            return None;
        }
        if entry.forms.len() > 1 {
            let forms = self.option_component_forms(owner)?;
            let valid = entry.id == owner.id
                && entry.kind
                    == EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    }
                && entry.case == NameCase::Sensitive
                && entry.alias_groups.is_empty()
                && entry.alias_of.is_none()
                && entry.value_domain.is_none()
                && entry.forms.len() == forms.len()
                && entry.names.len() == forms.len()
                && entry.name_bindings.len() == forms.len()
                && forms.iter().enumerate().all(|(index, (name, selection))| {
                    entry.names[index] == *name
                        && entry.forms[index] == *selection
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::NativeMarkup
                        && entry.name_bindings[index].occurrences.as_slice()
                            == std::slice::from_ref(selection)
                });
            return valid.then_some(entry);
        }
        let Some(form) = self.owner_complete_form(owner) else {
            // mdoc_macro.c::blk_full may keep a long Xo HEAD whose later
            // output joins are unknown. Its first Ic/Cm component can still
            // prove a complete command word; no other entry kind may borrow
            // this partial form or infer the rest of the HEAD.
            return self
                .validated_partial_literal_command(owner, entry)
                .then_some(entry);
        };
        let [only_form] = entry.forms.as_slice() else {
            return None;
        };
        if entry.kind == EntryKind::Term && owner.head_role.is_none() {
            let valid = entry.id == owner.id
                && entry.case == NameCase::Sensitive
                && entry.alias_groups.is_empty()
                && entry.alias_of.is_none()
                && entry.value_domain.is_none()
                && only_form == &owner.head
                && entry.names.is_empty()
                && entry.name_bindings.is_empty();
            return valid.then_some(entry);
        }
        if owner.head_role == Some(OwnerHeadRole::Option)
            && owner.head_components.len() > 1
            && self.option_component_names(owner).is_some()
        {
            return self
                .validated_component_names(owner, entry, only_form)
                .then_some(entry);
        }
        if entry.names.len() > 1 {
            return (self.validated_lexical_names(owner, entry, only_form)
                || self.validated_component_names(owner, entry, only_form))
            .then_some(entry);
        }
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
            ) => self.validated_lexical_names(owner, entry, only_form),
            (
                Some(OwnerHeadRole::Option | OwnerHeadRole::Lexical),
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                EntryNameEvidence::NativeMarkup,
            )
            | (
                Some(OwnerHeadRole::Environment),
                EntryKind::EnvironmentVariable,
                EntryNameEvidence::NativeMarkup,
            )
            | (Some(OwnerHeadRole::Literal), EntryKind::Command, EntryNameEvidence::NativeMarkup) => {
                self.native_markup_name_matches(
                    owner,
                    entry.kind,
                    &form,
                    only_name,
                    &binding.occurrences,
                )
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

    fn validated_partial_literal_command(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
    ) -> bool {
        let Some((name, component)) = self.literal_command_component(owner) else {
            return false;
        };
        let ([only_form], [only_name], [binding]) = (
            entry.forms.as_slice(),
            entry.names.as_slice(),
            entry.name_bindings.as_slice(),
        ) else {
            return false;
        };
        entry.id == owner.id
            && entry.kind == EntryKind::Command
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == component
            && only_name == &name
            && binding.name == 0
            && binding.evidence == EntryNameEvidence::NativeMarkup
            && binding.occurrences.as_slice() == std::slice::from_ref(component)
    }

    fn native_markup_name_matches(
        &self,
        owner: &OwnerMark,
        kind: EntryKind,
        form: &str,
        name: &str,
        occurrences: &[TextSelection],
    ) -> bool {
        let start = form.len() - form.trim_start().len();
        let Some(end) = start.checked_add(name.len()) else {
            return false;
        };
        if kind == EntryKind::Command && owner.head_role == Some(OwnerHeadRole::Literal) {
            let Some((native_name, component)) = self.literal_command_component(owner) else {
                return false;
            };
            return native_name == name
                && form.get(start..end) == Some(name)
                && form.get(end..).is_some_and(|suffix| {
                    suffix.is_empty() || suffix.starts_with(char::is_whitespace)
                })
                && self.selection_subrange(&owner.head, start..end).as_ref() == Some(component)
                && occurrences == std::slice::from_ref(component);
        }
        let Some(role_prefix) = owner.head_role_prefix.as_deref() else {
            return false;
        };
        let role_proves_name = match (kind, owner.head_role) {
            (EntryKind::EnvironmentVariable, _) => {
                crate::environment_variable_alias(role_prefix).as_deref() == Some(name)
            }
            (_, Some(OwnerHeadRole::Lexical)) => {
                role_prefix == name && crate::lexical_option_token(name)
            }
            _ => role_prefix == name && crate::native_option_token(name),
        };
        role_proves_name
            && form.trim_start().starts_with(role_prefix)
            && form.get(start..end) == Some(name)
            && (owner.head_role != Some(OwnerHeadRole::Lexical)
                || form.get(end..).is_some_and(|suffix| {
                    suffix.is_empty()
                        || suffix.starts_with('=')
                        || suffix.starts_with(char::is_whitespace)
                }))
            && self.selection_subrange(&owner.head, start..end).as_ref() == occurrences.first()
            && occurrences.len() == 1
    }

    /// A first native Ic/Cm component may prove one complete command token
    /// even when a later Xo HEAD join is unknown. The component must be the
    /// exact visible prefix and must end at a proved word boundary; layout
    /// adjacency alone never supplies that boundary.
    #[must_use]
    pub fn literal_command_component<'a>(
        &self,
        owner: &'a OwnerMark,
    ) -> Option<(String, &'a TextSelection)> {
        if owner.role != OwnerRole::Definition || owner.head_role != Some(OwnerHeadRole::Literal) {
            return None;
        }
        let component = owner.head_components.first()?;
        if component.role != OwnerHeadRole::Literal || !component.has_source_identity() {
            return None;
        }
        let name = self.selection_text(&component.selection)?;
        if !crate::native_command_token(&name) || component.selection.parts.is_empty() {
            return None;
        }
        let count = component.selection.parts.len();
        if count > owner.head.parts.len()
            || component.selection.joins.as_slice() != owner.head.joins.get(..count - 1)?
            || component.selection.parts.as_slice() != owner.head.parts.get(..count)?
        {
            return None;
        }
        let boundary = if count == owner.head.parts.len() {
            true
        } else {
            match owner.head.joins.get(count - 1)? {
                TextJoin::AuthoredSeparator(separator)
                | TextJoin::GeneratedSeparator(separator) => {
                    separator.starts_with(char::is_whitespace)
                }
                TextJoin::DirectContact => {
                    let next = &owner.head.parts[count];
                    let text = self.surface.run_text(next.run)?;
                    text.get(usize::try_from(next.start_byte).ok()?..)?
                        .starts_with(char::is_whitespace)
                }
                TextJoin::HardBoundary | TextJoin::Unknown => false,
            }
        };
        boundary.then_some((name, &component.selection))
    }

    /// Close every lexical alias against the same original HEAD and one
    /// exact, surviving display sub-selection. The syntax cannot stand in for
    /// the native role or for a missing glyph range.
    fn validated_lexical_names(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
        only_form: &TextSelection,
    ) -> bool {
        if owner.head_role != Some(OwnerHeadRole::Lexical) {
            return false;
        }
        let Some(found) = self.lexical_names(owner) else {
            return false;
        };
        let grouped = group_name_occurrences(
            found
                .into_iter()
                .map(|(name, selection, _)| (name, selection)),
        );
        entry.id == owner.id
            && entry.kind
                == EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                }
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == &owner.head
            && entry.names.len() == grouped.len()
            && entry.name_bindings.len() == grouped.len()
            && grouped
                .iter()
                .enumerate()
                .all(|(index, (name, occurrences))| {
                    entry.names[index] == *name
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::Lexical
                        && entry.name_bindings[index].occurrences == *occurrences
                })
    }

    fn validated_component_names(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
        only_form: &TextSelection,
    ) -> bool {
        let Some(names) = self.option_component_names(owner) else {
            return false;
        };
        let grouped = group_name_occurrences(
            names
                .into_iter()
                .map(|(name, selection, _)| (name, selection)),
        );
        entry.id == owner.id
            && entry.kind
                == EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                }
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == &owner.head
            && entry.names.len() == grouped.len()
            && entry.name_bindings.len() == grouped.len()
            && grouped
                .iter()
                .enumerate()
                .all(|(index, (name, occurrences))| {
                    entry.names[index] == *name
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::NativeMarkup
                        && entry.name_bindings[index].occurrences == *occurrences
                })
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
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => text.push_str(separator),
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
    /// touching a consumed authored or generated separator has no final
    /// glyph to bind and is rejected; no byte or cell coordinate is inferred
    /// from layout.
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
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => {
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
