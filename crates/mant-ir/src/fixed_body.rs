//! Source-neutral, post-device display body and checked mark references.
//!
//! `DisplaySurface::text` is the sole owned visible-body byte arena. Rows and
//! runs index it; marks retain only keys, ranges and small metadata. This
//! module does not infer formatter geometry or parse roff. The containing
//! `Document` closes typed source keys against its own `SourceTable`.

use std::{fmt, num::NonZeroU32};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{LinkTarget, SourceKey, SourceSpan};

// Match the existing native fixed-display geometry ceiling. A small UTF-8
// arena must not authorize an unbounded sparse row when a consumer expands
// terminal columns into cells.
const MAX_FIXED_ROW_COLUMNS: u32 = 1_048_576;
const MAX_FIXED_TOTAL_COLUMNS: u64 = 32 * 1024 * 1024;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextJoin {
    /// No byte lies between the slices, including a proven soft wrap.
    DirectContact,
    /// An authored separator is already retained in one of the slices.
    AuthoredSeparator,
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
    /// Root or an earlier section, not a derived depth.
    pub parent: Option<NonZeroU32>,
    /// Native heading-level hint.
    pub level_hint: u16,
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

/// One native candidate owner, not a fabricated semantic entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerMark {
    /// Dense one-based owner key.
    pub key: NonZeroU32,
    /// Root or an earlier enclosing owner.
    pub parent: Option<NonZeroU32>,
    /// Enclosing section, if known.
    pub section: Option<NonZeroU32>,
    /// Native owner kind before classification.
    pub role: OwnerRole,
    /// Direct visible head, possibly empty.
    pub head: TextSelection,
    /// Direct visible body, excluding child owners.
    pub direct_body: TextSelection,
    /// Position for a structurally empty owner.
    pub empty_point: Option<DisplayPoint>,
    /// Authored owner location when known.
    pub source: Option<SourceSpan>,
}

/// One native link macro instance, independent of its visible slice count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkMark {
    /// Dense one-based occurrence key.
    pub key: NonZeroU32,
    /// Typed destination decoded while the native source is still available.
    pub target: LinkTarget,
    /// Surviving clickable label slices.
    pub label: TextSelection,
    /// Authored macro location when known.
    pub source: Option<SourceSpan>,
}

/// One authored native anchor at its final, zero-width display position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnchorMark {
    /// Dense one-based declaration key.
    pub key: NonZeroU32,
    /// Authored declaration spelling, not a copied visible body.
    pub name: String,
    /// Final location after any native target migration.
    pub at: DisplayPoint,
    /// Original authored declaration location, distinct from `at`.
    pub source: Option<SourceSpan>,
}

/// Native region family useful for content boundaries and joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RegionKind {
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

/// A malformed reference or relationship in an owned Fixed body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedBodyError(&'static str);

impl fmt::Display for FixedBodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for FixedBodyError {}

impl DisplaySurface {
    /// Check dense final keys, row/run ownership, UTF-8 boundaries and exact
    /// one-time byte-arena coverage. No geometry is recomputed here.
    ///
    /// # Errors
    /// Returns the first malformed surface relationship.
    pub fn validate(&self) -> Result<(), FixedBodyError> {
        if self
            .text
            .chars()
            .any(|scalar| scalar.is_control() || matches!(scalar, '\u{2028}' | '\u{2029}'))
        {
            return Err(FixedBodyError(
                "display text contains unsafe control scalar",
            ));
        }
        let mut next_run = 1usize;
        let mut total_columns = 0u64;
        for (index, row) in self.rows.iter().enumerate() {
            if row.column_count > MAX_FIXED_ROW_COLUMNS {
                return Err(FixedBodyError("display row exceeds column budget"));
            }
            total_columns = total_columns
                .checked_add(u64::from(row.column_count))
                .ok_or(FixedBodyError("display column budget overflow"))?;
            if total_columns > MAX_FIXED_TOTAL_COLUMNS {
                return Err(FixedBodyError("display exceeds total column budget"));
            }
            if row.key.get() as usize != index + 1
                || row.first_run.get() as usize != next_run
                || (index + 1 < self.rows.len() && !row.break_after)
            {
                return Err(FixedBodyError("invalid display row order or break"));
            }
            next_run = next_run
                .checked_add(row.run_count as usize)
                .ok_or(FixedBodyError("display run count overflow"))?;
            if next_run > self.runs.len() + 1 {
                return Err(FixedBodyError("display row references missing runs"));
            }
            let mut last_column_end = 0u32;
            for run in &self.runs[(row.first_run.get() - 1) as usize..next_run - 1] {
                let end = run
                    .column
                    .checked_add(run.width)
                    .ok_or(FixedBodyError("display column overflow"))?;
                if run.row != row.key || run.column < last_column_end || end > row.column_count {
                    return Err(FixedBodyError("display run has invalid row or columns"));
                }
                last_column_end = end;
            }
        }
        if next_run != self.runs.len() + 1 {
            return Err(FixedBodyError("unowned display runs"));
        }
        let mut byte_end = 0usize;
        for (index, run) in self.runs.iter().enumerate() {
            let start = usize::try_from(run.byte_start)
                .map_err(|_| FixedBodyError("display byte offset overflow"))?;
            let count = usize::try_from(run.byte_count)
                .map_err(|_| FixedBodyError("display byte length overflow"))?;
            let end = start
                .checked_add(count)
                .ok_or(FixedBodyError("display byte range overflow"))?;
            if run.key.get() as usize != index + 1
                || start != byte_end
                || count == 0
                || !self.text.is_char_boundary(end)
                || end > self.text.len()
            {
                return Err(FixedBodyError("invalid display run byte coverage"));
            }
            byte_end = end;
        }
        if byte_end != self.text.len() {
            return Err(FixedBodyError("display text has uncovered bytes"));
        }
        Ok(())
    }

    /// Borrow one final run's visible text without copying it.
    #[must_use]
    pub fn run_text(&self, key: NonZeroU32) -> Option<&str> {
        let run = self.runs.get((key.get() - 1) as usize)?;
        (run.key == key).then_some(()).and_then(|()| {
            let start = usize::try_from(run.byte_start).ok()?;
            let end = start.checked_add(usize::try_from(run.byte_count).ok()?)?;
            self.text.get(start..end)
        })
    }

    fn validate_selection(&self, selection: &TextSelection) -> Result<(), FixedBodyError> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return Err(FixedBodyError("display selection joins are not pairwise"));
        }
        let mut previous: Option<OutputSlice> = None;
        for (index, part) in selection.parts.iter().enumerate() {
            let text = self
                .run_text(part.run)
                .ok_or(FixedBodyError("display selection references missing run"))?;
            let start = usize::try_from(part.start_byte)
                .map_err(|_| FixedBodyError("display selection byte overflow"))?;
            let end = usize::try_from(part.end_byte)
                .map_err(|_| FixedBodyError("display selection byte overflow"))?;
            if start >= end || text.get(start..end).is_none() {
                return Err(FixedBodyError("display selection is not a UTF-8 slice"));
            }
            if let Some(prior) = previous
                && (part.run < prior.run
                    || (part.run == prior.run && part.start_byte < prior.end_byte))
            {
                return Err(FixedBodyError(
                    "display selection is unordered or overlapping",
                ));
            }
            if let Some(prior) = previous
                && selection.joins[index - 1] == TextJoin::DirectContact
            {
                if part.run == prior.run {
                    if part.start_byte != prior.end_byte {
                        return Err(FixedBodyError(
                            "direct-contact selection skips bytes in one run",
                        ));
                    }
                } else {
                    let prior_run = &self.runs[(prior.run.get() - 1) as usize];
                    let current_run = &self.runs[(part.run.get() - 1) as usize];
                    let prior_text = self
                        .run_text(prior.run)
                        .ok_or(FixedBodyError("display selection references missing run"))?;
                    let adjacent_runs = prior.run.get().checked_add(1) == Some(part.run.get());
                    let adjacent_rows = prior_run.row == current_run.row
                        || prior_run.row.get().checked_add(1) == Some(current_run.row.get());
                    let prior_end = prior_run.column.checked_add(prior_run.width);
                    let adjacent_columns = if prior_run.row == current_run.row {
                        prior_end == Some(current_run.column)
                    } else {
                        let prior_row = &self.rows[(prior_run.row.get() - 1) as usize];
                        prior_end == Some(prior_row.column_count) && current_run.column == 0
                    };
                    if !adjacent_runs
                        || !adjacent_rows
                        || !adjacent_columns
                        || prior.end_byte != prior_text.len() as u64
                        || part.start_byte != 0
                    {
                        return Err(FixedBodyError(
                            "direct-contact selection skips visible output",
                        ));
                    }
                }
            }
            previous = Some(*part);
        }
        Ok(())
    }

    fn validate_point(&self, point: DisplayPoint) -> Result<(), FixedBodyError> {
        match point {
            DisplayPoint::RunBoundary { run, byte } => {
                let text = self
                    .run_text(run)
                    .ok_or(FixedBodyError("display point references missing run"))?;
                let byte = usize::try_from(byte)
                    .map_err(|_| FixedBodyError("display point byte overflow"))?;
                if !text.is_char_boundary(byte) {
                    return Err(FixedBodyError("display point is not a UTF-8 boundary"));
                }
            }
            DisplayPoint::DocumentEnd { row_count } => {
                if row_count as usize != self.rows.len() {
                    return Err(FixedBodyError("document-end point has wrong row count"));
                }
            }
        }
        Ok(())
    }
}

impl FixedBody {
    /// Validate all display and mark references without consulting source bytes.
    /// The containing `Document` must additionally close all `SourceKey`s.
    ///
    /// # Errors
    /// Returns the first malformed source-neutral relationship.
    pub fn validate(&self) -> Result<(), FixedBodyError> {
        self.surface.validate()?;
        for (index, heading) in self.headings.iter().enumerate() {
            dense_key(heading.key, index)?;
            earlier(heading.parent, heading.key)?;
            self.surface.validate_selection(&heading.title)?;
            self.surface.validate_selection(&heading.direct_body)?;
        }
        for (index, owner) in self.owners.iter().enumerate() {
            dense_key(owner.key, index)?;
            earlier(owner.parent, owner.key)?;
            reference(owner.section, self.headings.len())?;
            self.surface.validate_selection(&owner.head)?;
            self.surface.validate_selection(&owner.direct_body)?;
            if selections_overlap(&owner.head, &owner.direct_body) {
                return Err(FixedBodyError("owner head and direct body overlap"));
            }
            for part in owner.head.parts.iter().chain(&owner.direct_body.parts) {
                let run = &self.surface.runs[(part.run.get() - 1) as usize];
                if run.label.owner != Some(owner.key) {
                    return Err(FixedBodyError("owner selection crosses owner labels"));
                }
            }
            if let Some(point) = owner.empty_point {
                self.surface.validate_point(point)?;
            }
            if !owner.head.parts.is_empty() || !owner.direct_body.parts.is_empty() {
                if owner.empty_point.is_some() {
                    return Err(FixedBodyError("nonempty owner has an empty point"));
                }
            } else if owner.empty_point.is_none() {
                return Err(FixedBodyError("empty owner has no display point"));
            }
        }
        let mut link_covered_bytes = vec![0u64; self.surface.runs.len()];
        for (index, link) in self.links.iter().enumerate() {
            dense_key(link.key, index)?;
            validate_link_target(&link.target)?;
            self.surface.validate_selection(&link.label)?;
            for part in &link.label.parts {
                let run_index = (part.run.get() - 1) as usize;
                let run = &self.surface.runs[run_index];
                if run.label.link != Some(link.key) {
                    return Err(FixedBodyError("link selection crosses occurrence labels"));
                }
                link_covered_bytes[run_index] = link_covered_bytes[run_index]
                    .checked_add(part.end_byte - part.start_byte)
                    .ok_or(FixedBodyError("link selection byte count overflows"))?;
            }
        }
        for (index, anchor) in self.anchors.iter().enumerate() {
            dense_key(anchor.key, index)?;
            self.surface.validate_point(anchor.at)?;
        }
        for (index, region) in self.regions.iter().enumerate() {
            dense_key(region.key, index)?;
            earlier(region.parent, region.key)?;
            reference(region.owner, self.owners.len())?;
            self.surface.validate_selection(&region.selection)?;
            if let Some(point) = region.empty_point {
                self.surface.validate_point(point)?;
            }
            if region.selection.parts.is_empty() == region.empty_point.is_none() {
                return Err(FixedBodyError(
                    "region needs exactly one visible selection or empty point",
                ));
            }
        }
        for (index, run) in self.surface.runs.iter().enumerate() {
            reference(run.label.owner, self.owners.len())?;
            reference(run.label.link, self.links.len())?;
            if run.label.link.is_some() && link_covered_bytes[index] != run.byte_count {
                return Err(FixedBodyError("link label does not cover its labeled run"));
            }
        }
        Ok(())
    }

    /// Iterate every typed source identity retained in run labels and marks.
    pub fn source_keys(&self) -> impl Iterator<Item = SourceKey> + '_ {
        self.surface
            .runs
            .iter()
            .filter_map(|run| run.label.source)
            .chain(self.source_spans().map(|span| span.source))
    }

    /// Iterate authored mark spans for containing-document range validation.
    pub fn source_spans(&self) -> impl Iterator<Item = SourceSpan> + '_ {
        self.headings
            .iter()
            .filter_map(|mark| mark.source)
            .chain(self.owners.iter().filter_map(|mark| mark.source))
            .chain(self.links.iter().filter_map(|mark| mark.source))
            .chain(self.anchors.iter().filter_map(|mark| mark.source))
            .chain(self.regions.iter().filter_map(|mark| mark.source))
    }
}

fn selections_overlap(left: &TextSelection, right: &TextSelection) -> bool {
    let (mut left_index, mut right_index) = (0, 0);
    while let (Some(a), Some(b)) = (left.parts.get(left_index), right.parts.get(right_index)) {
        if a.run < b.run || (a.run == b.run && a.end_byte <= b.start_byte) {
            left_index += 1;
        } else if b.run < a.run || (a.run == b.run && b.end_byte <= a.start_byte) {
            right_index += 1;
        } else {
            return true;
        }
    }
    false
}

fn validate_link_target(target: &LinkTarget) -> Result<(), FixedBodyError> {
    let valid = match target {
        LinkTarget::External { uri } => crate::is_valid_external_uri(uri),
        LinkTarget::Email { address } => crate::is_valid_email_address(address),
        LinkTarget::Document { name, fragment } => {
            !name.is_empty()
                && !name.starts_with('/')
                && !name.contains(['\\', '?', '#'])
                && !name.chars().any(char::is_control)
                && name.split('/').all(|component| !component.is_empty())
                && fragment.as_deref().is_none_or(|fragment| {
                    !fragment.is_empty() && !fragment.chars().any(char::is_control)
                })
        }
        LinkTarget::Manual {
            name,
            manual_section,
        } => {
            !name.is_empty()
                && !name.contains(['/', '\\'])
                && !name
                    .chars()
                    .any(|scalar| scalar.is_whitespace() || scalar.is_control())
                && manual_section
                    .as_deref()
                    .is_none_or(crate::is_manual_section)
        }
        LinkTarget::Section { id } => crate::is_normalized_node_id(id.as_str()),
    };
    valid
        .then_some(())
        .ok_or(FixedBodyError("invalid fixed link target"))
}

fn dense_key(key: NonZeroU32, index: usize) -> Result<(), FixedBodyError> {
    if key.get() as usize == index + 1 {
        Ok(())
    } else {
        Err(FixedBodyError("display mark key is not dense"))
    }
}

fn earlier(parent: Option<NonZeroU32>, child: NonZeroU32) -> Result<(), FixedBodyError> {
    if parent.is_none_or(|key| key < child) {
        Ok(())
    } else {
        Err(FixedBodyError("display mark parent must be earlier"))
    }
}

fn reference(key: Option<NonZeroU32>, len: usize) -> Result<(), FixedBodyError> {
    if key.is_none_or(|key| key.get() as usize <= len) {
        Ok(())
    } else {
        Err(FixedBodyError("display mark reference does not resolve"))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod hardening_tests {
    use super::*;

    fn key(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn body_with_run(text: &str, width: u32) -> FixedBody {
        FixedBody {
            surface: DisplaySurface {
                text: text.to_owned(),
                rows: vec![DisplayRow {
                    key: key(1),
                    first_run: key(1),
                    run_count: 1,
                    column_count: width,
                    break_after: false,
                }],
                runs: vec![DisplayRun {
                    key: key(1),
                    row: key(1),
                    column: 0,
                    width,
                    byte_start: 0,
                    byte_count: text.len() as u64,
                    label: DisplayLabel {
                        owner: None,
                        link: None,
                        source: None,
                        style: DisplayStyle {
                            bold: false,
                            underline: false,
                        },
                        role: DisplayRole::Body,
                    },
                }],
            },
            headings: Vec::new(),
            owners: Vec::new(),
            links: Vec::new(),
            anchors: Vec::new(),
            regions: Vec::new(),
        }
    }

    fn selection(parts: &[(u64, u64)], joins: Vec<TextJoin>) -> TextSelection {
        TextSelection {
            parts: parts
                .iter()
                .map(|&(start_byte, end_byte)| OutputSlice {
                    run: key(1),
                    start_byte,
                    end_byte,
                })
                .collect(),
            joins,
        }
    }

    #[test]
    fn fixed_surface_rejects_controls_but_keeps_combining_text() {
        let combining = body_with_run("e\u{301}", 1);
        combining.validate().unwrap();
        for unsafe_text in ["a\n", "a\t", "a\u{1b}", "a\u{85}", "a\u{2028}", "a\u{2029}"] {
            let body = body_with_run(unsafe_text, 1);
            assert!(body.validate().is_err(), "{unsafe_text:?}");
            assert!(
                serde_json::from_value::<FixedBody>(serde_json::to_value(body).unwrap()).is_err()
            );
        }
    }

    #[test]
    fn sparse_geometry_has_a_bounded_row_and_total_column_count() {
        let mut body = body_with_run("a", 1);
        body.surface.rows[0].column_count = MAX_FIXED_ROW_COLUMNS + 1;
        assert!(body.validate().is_err());

        let mut body = body_with_run("a", 1);
        body.surface.rows = (1..=33)
            .map(|index| DisplayRow {
                key: key(index),
                first_run: key(if index == 1 { 1 } else { 2 }),
                run_count: u32::from(index == 1),
                column_count: MAX_FIXED_ROW_COLUMNS,
                break_after: index != 33,
            })
            .collect();
        assert!(body.validate().is_err());
    }

    #[test]
    fn direct_contact_must_not_skip_bytes_in_one_run() {
        let mut body = body_with_run("abc", 3);
        body.links.push(LinkMark {
            key: key(1),
            target: LinkTarget::External {
                uri: "https://example.test".to_owned(),
            },
            label: selection(&[(0, 1), (2, 3)], vec![TextJoin::DirectContact]),
            source: None,
        });
        body.surface.runs[0].label.link = Some(key(1));
        assert!(body.validate().is_err());
        body.links[0].label.joins[0] = TextJoin::Unknown;
        body.surface
            .validate_selection(&body.links[0].label)
            .unwrap();
        assert!(body.validate().is_err()); // The link must still cover every labeled byte.
        body.links[0].label.parts[1].start_byte = 1;
        body.links[0].label.joins[0] = TextJoin::DirectContact;
        body.validate().unwrap();
    }

    #[test]
    fn direct_contact_must_not_skip_a_visible_run() {
        let mut body = body_with_run("abc", 3);
        let first = body.surface.runs[0].clone();
        body.surface.runs = (0..3)
            .map(|index| DisplayRun {
                key: key(index + 1),
                column: index,
                width: 1,
                byte_start: u64::from(index),
                byte_count: 1,
                ..first.clone()
            })
            .collect();
        body.surface.rows[0].run_count = 3;
        let selection = TextSelection {
            parts: vec![
                OutputSlice {
                    run: key(1),
                    start_byte: 0,
                    end_byte: 1,
                },
                OutputSlice {
                    run: key(3),
                    start_byte: 0,
                    end_byte: 1,
                },
            ],
            joins: vec![TextJoin::DirectContact],
        };
        assert!(body.surface.validate_selection(&selection).is_err());
        let adjacent = TextSelection {
            parts: vec![
                OutputSlice {
                    run: key(1),
                    start_byte: 0,
                    end_byte: 1,
                },
                OutputSlice {
                    run: key(2),
                    start_byte: 0,
                    end_byte: 1,
                },
            ],
            joins: vec![TextJoin::DirectContact],
        };
        body.surface.validate_selection(&adjacent).unwrap();
    }

    #[test]
    fn direct_contact_across_rows_must_not_skip_column_padding() {
        let mut body = body_with_run("ab", 1);
        body.surface.runs[0].byte_count = 1;
        body.surface.runs.push(DisplayRun {
            key: key(2),
            row: key(2),
            column: 0,
            width: 1,
            byte_start: 1,
            byte_count: 1,
            ..body.surface.runs[0].clone()
        });
        body.surface.rows[0].column_count = 2;
        body.surface.rows[0].break_after = true;
        body.surface.rows.push(DisplayRow {
            key: key(2),
            first_run: key(2),
            run_count: 1,
            column_count: 1,
            break_after: false,
        });
        let selection = TextSelection {
            parts: vec![
                OutputSlice {
                    run: key(1),
                    start_byte: 0,
                    end_byte: 1,
                },
                OutputSlice {
                    run: key(2),
                    start_byte: 0,
                    end_byte: 1,
                },
            ],
            joins: vec![TextJoin::DirectContact],
        };
        body.surface.validate().unwrap();
        assert!(body.surface.validate_selection(&selection).is_err());
        body.surface.rows[0].column_count = 1;
        body.surface.validate_selection(&selection).unwrap();
        body.surface.runs[1].column = 1;
        body.surface.rows[1].column_count = 2;
        assert!(body.surface.validate_selection(&selection).is_err());
    }

    #[test]
    fn link_run_labels_require_complete_occurrence_coverage() {
        let mut body = body_with_run("abc", 3);
        body.surface.runs[0].label.link = Some(key(1));
        body.links.push(LinkMark {
            key: key(1),
            target: LinkTarget::External {
                uri: "https://example.test".to_owned(),
            },
            label: TextSelection {
                parts: Vec::new(),
                joins: Vec::new(),
            },
            source: None,
        });
        assert!(body.validate().is_err());
        body.links[0].label.parts.push(OutputSlice {
            run: key(1),
            start_byte: 0,
            end_byte: 3,
        });
        body.validate().unwrap();
    }

    #[test]
    fn empty_region_requires_its_final_point() {
        let mut body = body_with_run("a", 1);
        body.regions.push(RegionMark {
            key: key(1),
            parent: None,
            owner: None,
            kind: RegionKind::Literal,
            selection: TextSelection {
                parts: Vec::new(),
                joins: Vec::new(),
            },
            empty_point: None,
            source: None,
        });
        assert!(body.validate().is_err());
        body.regions[0].empty_point = Some(DisplayPoint::DocumentEnd { row_count: 1 });
        body.validate().unwrap();
    }

    #[test]
    fn owner_head_and_body_cannot_claim_the_same_bytes() {
        let mut body = body_with_run("abc", 3);
        body.owners.push(OwnerMark {
            key: key(1),
            parent: None,
            section: None,
            role: OwnerRole::Definition,
            head: selection(&[(0, 2)], Vec::new()),
            direct_body: selection(&[(1, 3)], Vec::new()),
            empty_point: None,
            source: None,
        });
        body.surface.runs[0].label.owner = Some(key(1));
        assert!(body.validate().is_err());
        body.owners[0].direct_body.parts[0].start_byte = 2;
        body.validate().unwrap();
    }

    #[test]
    fn fixed_links_obey_the_shared_target_grammar() {
        let mut body = body_with_run("a", 1);
        body.links.push(LinkMark {
            key: key(1),
            target: LinkTarget::External {
                uri: "https://example.test".to_owned(),
            },
            label: selection(&[(0, 1)], Vec::new()),
            source: None,
        });
        body.surface.runs[0].label.link = Some(key(1));
        body.validate().unwrap();
        for invalid in [
            LinkTarget::External {
                uri: "https://bad host".to_owned(),
            },
            LinkTarget::Email {
                address: "bad@@example.test".to_owned(),
            },
            LinkTarget::Document {
                name: "../?bad".to_owned(),
                fragment: None,
            },
            LinkTarget::Manual {
                name: "bad/name".to_owned(),
                manual_section: None,
            },
            LinkTarget::Section {
                id: "Mixed.Target".into(),
            },
        ] {
            body.links[0].target = invalid;
            assert!(body.validate().is_err());
            assert!(
                serde_json::from_value::<FixedBody>(serde_json::to_value(&body).unwrap()).is_err()
            );
        }
    }
}
