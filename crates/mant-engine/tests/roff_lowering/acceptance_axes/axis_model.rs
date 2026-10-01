//! Test-private axis model for roff acceptance assertions.
//!
//! The shape follows the shared-execution repair guide (section 5.7): every
//! case declares a comparison policy per dimension and ordered expectations
//! per axis. A selected or accepted-content projection never disables row
//! or separator assertions by itself; each axis that carries no expectation
//! must carry an explicit applicability note instead. The comparator is
//! pure data-in/report-out so its own mutation self-checks (`mutations`)
//! can prove that every axis catches the error class it owns.

use std::fmt::Write as _;

/// Nonbreaking blank as emitted by the UTF-8 device (term.c nbrsp cell).
pub(crate) const NONBREAKING_BLANK: char = '\u{a0}';

/// Applicability of the accepted-content dimension.
///
/// `Selected` and `Recovery` applicability states arrive together with the
/// recovery-classified cards of the later repair stages.
pub(crate) enum ContentPolicy {
    /// The complete accepted word sequence of the selected region compares.
    Exact,
}

/// Applicability of the physical-row dimension.
///
/// `NotApplicable` arrives with cases that pin no row fact at all.
pub(crate) enum RowsPolicy {
    /// Row text equality after the indent policy. Blank rows are not
    /// trimmed away; only the indent policy may normalize.
    ExactHardRows,
    /// Only the declared row facts (row count, hard-row relations, explicit
    /// blank counts) are asserted; responsive geometry stays unpinned
    /// (reason in the row-axis applicability note).
    ConstrainedEvents,
}

/// Applicability of the indentation dimension.
pub(crate) enum IndentPolicy {
    /// Each side's common left page margin is removed before row text
    /// compares; relative indents stay pinned.
    OmitCommonMargin,
    /// Indentation may differ for the stated row-axis reason; row events
    /// still apply.
    Responsive,
}

/// Applicability of the typed-identity dimension.
pub(crate) enum IdentityPolicy {
    /// Ordered typed identity occurrences with visible labels compare,
    /// multiplicity included.
    RichInline,
    /// No typed identity compares (reason in the identity-axis note).
    NotApplicable,
}
/// Per-dimension comparison policy of one case.
pub(crate) struct AxisPolicy {
    pub(crate) content: ContentPolicy,
    pub(crate) rows: RowsPolicy,
    pub(crate) indent: IndentPolicy,
    pub(crate) identity: IdentityPolicy,
}

/// One registered expectation that must hold in the product.
pub(crate) struct Axis<T> {
    pub(crate) expect: T,
}

impl<T> Axis<T> {
    pub(crate) fn must(expect: T) -> Self {
        Self { expect }
    }
}

/// Relation between two adjacent content units.
pub(crate) enum SeparatorRelation {
    /// Ordinary same-row word boundary, frozen by the recorded oracle.
    WordBoundary,
    /// The recorded oracle proves the units are glued with no separator.
    Joined,
    /// The units are joined by nonbreaking blanks on the reading device.
    Unbreakable,
    /// Layout may wrap responsively, but the units must never glue.
    ResponsiveBreak,
}

pub(crate) struct SeparatorExpect {
    pub(crate) left: &'static str,
    pub(crate) right: &'static str,
    pub(crate) relation: SeparatorRelation,
}

#[derive(Debug)]
pub(crate) enum HardRowRelation {
    DifferentRows,
    SameRow,
}

pub(crate) struct HardRowExpect {
    pub(crate) left: &'static str,
    pub(crate) right: &'static str,
    pub(crate) relation: HardRowRelation,
}

/// Exact count of blank rows strictly between two units' rows.
pub(crate) struct BlankCountExpect {
    pub(crate) after: &'static str,
    pub(crate) before: &'static str,
    pub(crate) count: usize,
}

/// Owner of a unit's visible range: no link, or the nth typed identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    None,
    Link(usize),
}

pub(crate) struct OwnershipExpect {
    pub(crate) unit: &'static str,
    pub(crate) owner: Owner,
}

/// Ordered typed identity occurrence with its visible label text.
pub(crate) struct IdentityExpect {
    pub(crate) uri: &'static str,
    pub(crate) label: &'static str,
}

/// Scalar character span of one owner's visible label inside the recorded
/// oracle row (half-open, counted in Unicode scalars).
pub(crate) struct ScalarRangeExpect {
    pub(crate) owner: Owner,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnitStyle {
    Plain,
    Emphasis,
    Strong,
    Code,
}

pub(crate) struct StyleExpect {
    pub(crate) unit: &'static str,
    pub(crate) style: UnitStyle,
}

/// Authored source line of one unit (one-based).
pub(crate) struct SourceExpect {
    pub(crate) unit: &'static str,
    pub(crate) line: u32,
}

/// Registered expectations. `None` means the axis is not applicable; the
/// case must carry an explicit note for it.
pub(crate) struct GoldCard {
    pub(crate) accepted_units: Option<Axis<Vec<&'static str>>>,
    pub(crate) forbidden_units: Option<Axis<Vec<&'static str>>>,
    pub(crate) separators: Option<Axis<Vec<SeparatorExpect>>>,
    pub(crate) hard_rows: Option<Axis<Vec<HardRowExpect>>>,
    pub(crate) blank_counts: Option<Axis<Vec<BlankCountExpect>>>,
    pub(crate) row_count: Option<Axis<usize>>,
    pub(crate) identities: Option<Axis<Vec<IdentityExpect>>>,
    pub(crate) ownership: Option<Axis<Vec<OwnershipExpect>>>,
    pub(crate) scalar_ranges: Option<Axis<Vec<ScalarRangeExpect>>>,
    pub(crate) styles: Option<Axis<Vec<StyleExpect>>>,
    pub(crate) sources: Option<Axis<Vec<SourceExpect>>>,
}

impl GoldCard {
    pub(crate) const fn none() -> Self {
        Self {
            accepted_units: None,
            forbidden_units: None,
            separators: None,
            hard_rows: None,
            blank_counts: None,
            row_count: None,
            identities: None,
            ownership: None,
            scalar_ranges: None,
            styles: None,
            sources: None,
        }
    }
}

/// The comparison axes. Each maps to one error class of the mutation table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AxisKind {
    Content,
    ForbiddenContent,
    Separator,
    HardRows,
    BlankCount,
    RowCount,
    ExactRows,
    Identity,
    Ownership,
    ScalarRange,
    Style,
    Source,
}

impl AxisKind {
    pub(crate) const ALL: [AxisKind; 12] = [
        AxisKind::Content,
        AxisKind::ForbiddenContent,
        AxisKind::Separator,
        AxisKind::HardRows,
        AxisKind::BlankCount,
        AxisKind::RowCount,
        AxisKind::ExactRows,
        AxisKind::Identity,
        AxisKind::Ownership,
        AxisKind::ScalarRange,
        AxisKind::Style,
        AxisKind::Source,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            AxisKind::Content => "accepted-content",
            AxisKind::ForbiddenContent => "forbidden-content",
            AxisKind::Separator => "separator",
            AxisKind::HardRows => "hard-rows",
            AxisKind::BlankCount => "blank-count",
            AxisKind::RowCount => "row-count",
            AxisKind::ExactRows => "exact-rows",
            AxisKind::Identity => "identity",
            AxisKind::Ownership => "ownership",
            AxisKind::ScalarRange => "scalar-range",
            AxisKind::Style => "style",
            AxisKind::Source => "source",
        }
    }
}

/// One case: identity, policy, expectations and explicit applicability
/// notes for every axis without an expectation.
pub(crate) struct AcceptanceCase {
    pub(crate) id: &'static str,
    pub(crate) family: &'static str,
    pub(crate) policy: AxisPolicy,
    pub(crate) gold: GoldCard,
    pub(crate) unregistered: &'static [(AxisKind, &'static str)],
}

impl AcceptanceCase {
    /// Whether the case registers an expectation on the axis.
    pub(crate) fn registers(&self, kind: AxisKind) -> bool {
        match kind {
            AxisKind::Content => self.gold.accepted_units.is_some(),
            AxisKind::ForbiddenContent => self.gold.forbidden_units.is_some(),
            AxisKind::Separator => self.gold.separators.is_some(),
            AxisKind::HardRows => self.gold.hard_rows.is_some(),
            AxisKind::BlankCount => self.gold.blank_counts.is_some(),
            AxisKind::RowCount => self.gold.row_count.is_some(),
            AxisKind::ExactRows => {
                matches!(self.policy.rows, RowsPolicy::ExactHardRows)
            }
            AxisKind::Identity => self.gold.identities.is_some(),
            AxisKind::Ownership => self.gold.ownership.is_some(),
            AxisKind::ScalarRange => self.gold.scalar_ranges.is_some(),
            AxisKind::Style => self.gold.styles.is_some(),
            AxisKind::Source => self.gold.sources.is_some(),
        }
    }

    /// Note explaining why an axis carries no expectation.
    pub(crate) fn note_for(&self, kind: AxisKind) -> Option<&'static str> {
        self.unregistered
            .iter()
            .find(|(noted, _)| *noted == kind)
            .map(|(_, reason)| *reason)
    }

    /// Responsive indentation only makes sense with constrained row
    /// events; exact row text would contradict it.
    pub(crate) fn assert_policy_consistency(&self) {
        if matches!(self.policy.indent, IndentPolicy::Responsive)
            && matches!(self.policy.rows, RowsPolicy::ExactHardRows)
        {
            panic!(
                "{}: responsive indentation cannot pair with exact row text",
                self.id
            );
        }
    }
}

/// Observed facts of one rendering. Row facts come from the projected
/// region rows; identity, ownership, style and source facts come from the
/// semantic model behind the rendering.
pub(crate) struct Observed {
    pub(crate) rows: Vec<String>,
    pub(crate) identities: Vec<(String, String)>,
    pub(crate) ownership: Vec<(String, Owner)>,
    pub(crate) scalar_ranges: Vec<(Owner, usize, usize)>,
    pub(crate) styles: Vec<(String, UnitStyle)>,
    pub(crate) sources: Vec<(String, u32)>,
}

impl Observed {
    /// Row facts only, with no semantic model behind them.
    pub(crate) fn from_rows(rows: &[String]) -> Self {
        Self {
            rows: rows.to_vec(),
            identities: Vec::new(),
            ownership: Vec::new(),
            scalar_ranges: Vec::new(),
            styles: Vec::new(),
            sources: Vec::new(),
        }
    }
}

pub(crate) struct AxisFailure {
    pub(crate) kind: AxisKind,
    pub(crate) detail: String,
}

pub(crate) struct AxisReport {
    pub(crate) failures: Vec<AxisFailure>,
}

impl AxisReport {
    pub(crate) fn fails_on(&self, kind: AxisKind) -> bool {
        self.failures.iter().any(|failure| failure.kind == kind)
    }

    pub(crate) fn details_for(&self, kind: AxisKind) -> String {
        self.failures
            .iter()
            .filter(|failure| failure.kind == kind)
            .map(|failure| failure.detail.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn add(&mut self, kind: AxisKind, detail: String) {
        self.failures.push(AxisFailure { kind, detail });
    }
}

/// Words of one row: ordinary blanks separate words; a nonbreaking blank
/// is a device cell, not a word boundary.
pub(crate) fn row_words(row: &str) -> Vec<&str> {
    row.split([' ', '\t'])
        .filter(|word| !word.is_empty())
        .collect()
}

/// Ordered accepted words of a whole region.
pub(crate) fn region_words(rows: &[String]) -> Vec<String> {
    rows.iter()
        .flat_map(|row| row_words(row))
        .map(str::to_owned)
        .collect()
}

/// A blank row carries no content cells. Nonbreaking blanks are cells, so
/// a row of only nonbreaking blanks is not blank.
pub(crate) fn is_blank_row(row: &str) -> bool {
    row.chars()
        .all(|character| character == ' ' || character == '\t')
}

/// Index of the first row containing the unit as a substring.
pub(crate) fn row_containing(rows: &[String], unit: &str) -> Option<usize> {
    rows.iter().position(|row| row.contains(unit))
}

/// Common leading blank count of the non-blank rows.
pub(crate) fn common_margin(rows: &[String]) -> usize {
    rows.iter()
        .filter(|row| !is_blank_row(row))
        .map(|row| row.len() - row.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0)
}

fn strip_margin(row: &str, margin: usize) -> String {
    let cut = row
        .char_indices()
        .nth(margin)
        .map_or(row.len(), |(at, _)| at);
    row[cut..].to_owned()
}

/// Compare a rendering against the registered expectations.
pub(crate) fn evaluate(
    case: &AcceptanceCase,
    oracle_rows: &[String],
    observed: &Observed,
) -> AxisReport {
    let mut report = AxisReport {
        failures: Vec::new(),
    };
    if matches!(case.policy.content, ContentPolicy::Exact) {
        check_content(case, observed, &mut report);
        check_forbidden(case, observed, &mut report);
    }
    check_separators(case, observed, &mut report);
    check_hard_rows(case, observed, &mut report);
    check_blank_counts(case, observed, &mut report);
    check_row_count(case, observed, &mut report);
    check_exact_rows(case, oracle_rows, observed, &mut report);
    if matches!(case.policy.identity, IdentityPolicy::RichInline) {
        check_identities(case, observed, &mut report);
    }
    check_ownership(case, observed, &mut report);
    check_scalar_ranges(case, observed, &mut report);
    check_styles(case, observed, &mut report);
    check_sources(case, observed, &mut report);
    report
}

fn check_content(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.accepted_units else {
        return;
    };
    let actual = region_words(&observed.rows);
    if actual != axis.expect {
        report.add(
            AxisKind::Content,
            format!("accepted units {actual:?} differ from {:?}", axis.expect),
        );
    }
}

fn check_forbidden(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.forbidden_units else {
        return;
    };
    let actual = region_words(&observed.rows);
    let present: Vec<&str> = axis
        .expect
        .iter()
        .filter(|unit| actual.iter().any(|word| word == *unit))
        .copied()
        .collect();
    if !present.is_empty() {
        report.add(
            AxisKind::ForbiddenContent,
            format!("rejected or forbidden units leaked: {present:?}"),
        );
    }
}

fn check_separators(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.separators else {
        return;
    };
    for expect in &axis.expect {
        if !separator_holds(&observed.rows, expect) {
            report.add(
                AxisKind::Separator,
                format!(
                    "relation between {:?} and {:?} ({}) is violated",
                    expect.left,
                    expect.right,
                    relation_name(&expect.relation)
                ),
            );
        }
    }
}

fn relation_name(relation: &SeparatorRelation) -> &'static str {
    match relation {
        SeparatorRelation::WordBoundary => "word boundary",
        SeparatorRelation::Joined => "proven join",
        SeparatorRelation::Unbreakable => "nonbreaking",
        SeparatorRelation::ResponsiveBreak => "responsive break, never glued",
    }
}

/// Whether one separator expectation holds on the observed rows.
pub(crate) fn separator_holds(rows: &[String], expect: &SeparatorExpect) -> bool {
    match expect.relation {
        SeparatorRelation::WordBoundary => rows.iter().any(|row| {
            let words = row_words(row);
            words
                .windows(2)
                .any(|pair| pair[0] == expect.left && pair[1] == expect.right)
        }),
        SeparatorRelation::Joined => rows
            .iter()
            .any(|row| row.contains(format!("{}{}", expect.left, expect.right).as_str())),
        SeparatorRelation::Unbreakable => rows.iter().any(|row| nonbreaking_join(row, expect)),
        SeparatorRelation::ResponsiveBreak => {
            row_containing(rows, expect.left).is_some()
                && row_containing(rows, expect.right).is_some()
                && !rows
                    .iter()
                    .any(|row| row.contains(format!("{}{}", expect.left, expect.right).as_str()))
        }
    }
}

fn nonbreaking_join(row: &str, expect: &SeparatorExpect) -> bool {
    let Some(at) = row.find(expect.left) else {
        return false;
    };
    let rest = &row[at + expect.left.len()..];
    let mut consumed = 0usize;
    let mut blanks = 0usize;
    for character in rest.chars() {
        if character == NONBREAKING_BLANK {
            blanks += 1;
            consumed += character.len_utf8();
        } else {
            break;
        }
    }
    blanks > 0 && rest[consumed..].starts_with(expect.right)
}

fn check_hard_rows(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.hard_rows else {
        return;
    };
    for expect in &axis.expect {
        let (Some(left), Some(right)) = (
            row_containing(&observed.rows, expect.left),
            row_containing(&observed.rows, expect.right),
        ) else {
            report.add(
                AxisKind::HardRows,
                format!(
                    "units {:?} and {:?} are not both observed",
                    expect.left, expect.right
                ),
            );
            continue;
        };
        let holds = match expect.relation {
            HardRowRelation::DifferentRows => left != right,
            HardRowRelation::SameRow => left == right,
        };
        if !holds {
            report.add(
                AxisKind::HardRows,
                format!(
                    "units {:?} (row {left}) and {:?} (row {right}) violate {:?}",
                    expect.left, expect.right, expect.relation
                ),
            );
        }
    }
}

fn check_blank_counts(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.blank_counts else {
        return;
    };
    for expect in &axis.expect {
        match blank_count_between(&observed.rows, expect.after, expect.before) {
            Some(actual) if actual == expect.count => {}
            Some(actual) => report.add(
                AxisKind::BlankCount,
                format!(
                    "blank rows between {:?} and {:?}: {actual}, expected {}",
                    expect.after, expect.before, expect.count
                ),
            ),
            None => report.add(
                AxisKind::BlankCount,
                format!(
                    "units {:?} and {:?} are not both observed",
                    expect.after, expect.before
                ),
            ),
        }
    }
}

/// Blank rows strictly between the two units' rows, when both are observed.
pub(crate) fn blank_count_between(rows: &[String], after: &str, before: &str) -> Option<usize> {
    let left = row_containing(rows, after)?;
    let right = row_containing(rows, before)?;
    if right < left {
        return None;
    }
    if right == left {
        return Some(0);
    }
    Some(
        rows[left + 1..right]
            .iter()
            .filter(|row| is_blank_row(row))
            .count(),
    )
}

fn check_row_count(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.row_count else {
        return;
    };
    if observed.rows.len() != axis.expect {
        report.add(
            AxisKind::RowCount,
            format!(
                "region has {} rows, expected {}",
                observed.rows.len(),
                axis.expect
            ),
        );
    }
}

fn check_exact_rows(
    case: &AcceptanceCase,
    oracle_rows: &[String],
    observed: &Observed,
    report: &mut AxisReport,
) {
    if !matches!(case.policy.rows, RowsPolicy::ExactHardRows) {
        return;
    }
    let expected = normalized_rows(oracle_rows, &case.policy.indent);
    let actual = normalized_rows(&observed.rows, &case.policy.indent);
    if expected != actual {
        let mut detail = String::from("hard rows differ");
        let _ = write!(
            &mut detail,
            " expected{} actual{}",
            RowsDebug(&expected),
            RowsDebug(&actual)
        );
        report.add(AxisKind::ExactRows, detail);
    }
}

/// Render row vectors compactly for failure details.
struct RowsDebug<'a>(&'a [String]);

impl std::fmt::Display for RowsDebug<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, row) in self.0.iter().enumerate() {
            write!(formatter, " [row {index}: {row:?}]")?;
        }
        Ok(())
    }
}

fn normalized_rows(rows: &[String], policy: &IndentPolicy) -> Vec<String> {
    match policy {
        IndentPolicy::OmitCommonMargin => {
            let margin = common_margin(rows);
            rows.iter().map(|row| strip_margin(row, margin)).collect()
        }
        IndentPolicy::Responsive => rows.to_vec(),
    }
}

fn check_identities(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.identities else {
        return;
    };
    let expected: Vec<(String, String)> = axis
        .expect
        .iter()
        .map(|identity| (identity.uri.to_owned(), identity.label.to_owned()))
        .collect();
    if observed.identities != expected {
        report.add(
            AxisKind::Identity,
            format!(
                "typed identity occurrences {:?} differ from {expected:?}",
                observed.identities
            ),
        );
    }
}

fn check_ownership(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.ownership else {
        return;
    };
    for expect in &axis.expect {
        // A unit the rendering lost is a content failure, not an ownership
        // one; present units must sit in the declared visible range.
        let Some((_, owner)) = observed
            .ownership
            .iter()
            .find(|(unit, _)| unit == expect.unit)
        else {
            continue;
        };
        if *owner != expect.owner {
            report.add(
                AxisKind::Ownership,
                format!(
                    "unit {:?} sits in visible range {owner:?}, expected {:?}",
                    expect.unit, expect.owner
                ),
            );
        }
    }
}

fn check_scalar_ranges(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.scalar_ranges else {
        return;
    };
    let expected: Vec<(Owner, usize, usize)> = axis
        .expect
        .iter()
        .map(|range| (range.owner, range.start, range.end))
        .collect();
    if observed.scalar_ranges != expected {
        report.add(
            AxisKind::ScalarRange,
            format!(
                "scalar ranges {:?} differ from {expected:?}",
                observed.scalar_ranges
            ),
        );
    }
}

fn check_styles(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.styles else {
        return;
    };
    for expect in &axis.expect {
        let Some((_, style)) = observed.styles.iter().find(|(unit, _)| unit == expect.unit) else {
            report.add(
                AxisKind::Style,
                format!("unit {:?} has no observed style", expect.unit),
            );
            continue;
        };
        if *style != expect.style {
            report.add(
                AxisKind::Style,
                format!(
                    "unit {:?} has style {style:?}, expected {:?}",
                    expect.unit, expect.style
                ),
            );
        }
    }
}

fn check_sources(case: &AcceptanceCase, observed: &Observed, report: &mut AxisReport) {
    let Some(axis) = &case.gold.sources else {
        return;
    };
    for expect in &axis.expect {
        let Some((_, line)) = observed
            .sources
            .iter()
            .find(|(unit, _)| unit == expect.unit)
        else {
            report.add(
                AxisKind::Source,
                format!("unit {:?} has no observed source line", expect.unit),
            );
            continue;
        };
        if *line != expect.line {
            report.add(
                AxisKind::Source,
                format!(
                    "unit {:?} has source line {line}, expected {}",
                    expect.unit, expect.line
                ),
            );
        }
    }
}
