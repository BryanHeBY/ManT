//! Response-relative matching facts and ordinary display bindings.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Maximum retained Name and Form records combined per evidence owner.
pub const MAX_EXPLANATION_MATCH_RECORDS: usize = 32;
/// Maximum ordinary display bindings per evidence owner (not a names limit).
pub const MAX_EXPLANATION_NAME_BINDINGS: usize = 32;
/// Maximum source occurrences retained per match or display-binding record.
pub const MAX_EXPLANATION_OCCURRENCES: usize = 32;
/// Maximum fragments in one occurrence in each target domain.
pub const MAX_EXPLANATION_FRAGMENTS: usize = 32;
/// Combined match, display-binding and preview/body fragments per owner.
pub const MAX_EXPLANATION_POSITIONS: usize = 1024;

/// One documented name that actually matched under the owner's case policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationNameMatch {
    /// Exact authored spelling, usable even when entry metadata is absent.
    pub name: String,
    /// Complete occurrences in available response targets, not implicit aliases.
    pub occurrences: Vec<ExplanationOccurrence>,
}

/// One complete authored form that actually matched during collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationFormMatch {
    /// Snapshot-local ordinal in the original owner's forms, not a durable ID
    /// or an index into an omitted/filtered response payload.
    pub source_form_index: u32,
    /// Complete authored visible text, with original spelling.
    pub text: String,
    /// Complete positions in available returned targets.
    pub occurrences: Vec<ExplanationOccurrence>,
}

/// An actual identity match, referencing this evidence's outline fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ExplanationIdentityField {
    /// The exact outline node ID matched.
    Id,
    /// The structural outline path matched.
    Path,
}

/// One ordinary name binding, independent of whether that name matched a query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationNameBinding {
    /// Index into this response's `entry.names` (never an absent entry).
    pub name_index: u32,
    /// Validated source occurrences projected into the returned payload.
    pub occurrences: Vec<ExplanationOccurrence>,
}

/// One whole occurrence, with independent complete projections per target domain.
/// An unavailable or omitted domain is empty; a partial name is never retained.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationOccurrence {
    /// Original binding-occurrence ordinal (zero for a complete form match).
    /// A snapshot coordinate, not a durable identity or returned-array index.
    pub source_occurrence_index: u32,
    /// Ordered fragments within returned forms.
    pub forms: Vec<ExplanationFormRange>,
    /// Ordered fragments within the returned original body.
    pub content: Vec<ExplanationContentRange>,
}

/// Half-open Unicode scalar offsets in returned `entry.forms[formIndex]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationFormRange {
    /// Index into the forms actually returned in this entry's metadata.
    pub form_index: u32,
    /// Start in safe visible text, excluding renderer-added framing.
    pub start_char: u32,
    /// Exclusive end in the same text root.
    pub end_char: u32,
}

/// A typed path step relative to the single returned `content.block`.
/// Item/cell steps select an array and must be followed by a block step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ExplanationBlockStep {
    /// One ordinary list item's block array.
    ListItem {
        /// Zero-based item index.
        index: u32,
    },
    /// One definition's description block array.
    DefinitionItem {
        /// Zero-based item index.
        index: u32,
    },
    /// One table cell's block array.
    TableCell {
        /// Zero-based original row index (not a laid-out row).
        row: u32,
        /// Zero-based original cell index (not its spanned screen column).
        column: u32,
    },
    /// One block in the array selected by the preceding item/cell step.
    Block {
        /// Zero-based block index.
        index: u32,
    },
}

/// Half-open Unicode scalar offsets rooted in this response's original body.
/// The path does not reference the full source document or a preview window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ExplanationContentRange {
    /// Safe visible text of a paragraph, preformatted, equation or unsupported leaf.
    BlockText {
        /// Typed path to that leaf, starting at `content.block`.
        path: Vec<ExplanationBlockStep>,
        /// First matched scalar in the leaf text.
        start_char: u32,
        /// Exclusive end scalar in the same leaf.
        end_char: u32,
    },
    /// Safe visible inline text of exactly one original definition term.
    DefinitionTerm {
        /// Typed path to a definition-list block.
        path: Vec<ExplanationBlockStep>,
        /// Item in that returned list; an excerpted owner is item zero.
        item_index: u32,
        /// Index in that item's terms, independent of form indices.
        term_index: u32,
        /// First matched scalar in this term.
        start_char: u32,
        /// Exclusive end scalar in the same term.
        end_char: u32,
    },
}
