//! Filled flow preserves font, spacing and pending boundaries across scopes.
use super::{Font, ZeroAdvanceState, needs_boundary_space, push_text, updated_spacing};
use libmandoc_rs::MacroSet;
use mant_ir::Inline;
use mant_ir::{first_visible_character, has_printable_character, last_visible_character};

mod builder;
mod definition;
pub(in crate::mandoc) use definition::DefinitionGeometryCheckpoint;
mod execution;
pub(in crate::mandoc) mod field_buffer;
use definition::DefinitionFieldState;
mod lifecycle;
mod native_field;
mod tab_stops;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use tab_stops::TabStops;
mod no_fill;
pub(in crate::mandoc) use native_field::{FieldFlag, FieldFlags};
mod operand_capture;
mod output;
pub(in crate::mandoc) use operand_capture::HeadOperandCapture;
pub(in crate::mandoc::inline) use output::{INTERNAL_FIELD_WORD, INTERNAL_LINK_SPLIT};

pub(in crate::mandoc) use no_fill::{NoFillInlineState, lower_no_fill_fragment_with_formatter};

pub(super) use output::trailing_ascii_spaces;
pub(in crate::mandoc) use output::trim_trailing_breakable_spaces;
pub(in crate::mandoc) use output::{
    CompletedRowOrigin, OutputRowEnd, consume_one_row_ending, ends_with_executed_line_break,
    has_rendered_formatter_glyph, native_row_origin, prepare_inline_output,
    retain_inline_identities, split_row_origin, strip_native_projection_markers,
    take_definition_term_breaks, take_inline_layout, trailing_completed_row_origins,
};

/// A stable source-word range plus the device cells its IR owner already
/// represents. Padding never enters the native input buffer.
#[derive(Clone, Debug)]
pub(in crate::mandoc::inline) struct NativeWordAnchor {
    start: usize,
    owner: String,
    content: usize,
    projected_device_padding: usize,
    /// The word's generated field prefix was written before its content
    /// marker. This receipt owns positioning, not a native buffer scalar.
    projected_field_prefix: bool,
}

pub(in crate::mandoc) struct InlineBuilder {
    nodes: Vec<Inline>,
    // Only live annotation scopes have an address. A checkpoint's last
    // handle unregisters on return, including unused/early-return branches.
    output_positions: Option<Rc<OutputPositionRegistry>>,
    pub(in crate::mandoc) head_operand_capture:
        Option<std::rc::Rc<std::cell::RefCell<HeadOperandCapture>>>,
    // Macro handlers such as pre_alternate() call term_word() directly on
    // operands instead of visiting those TEXT nodes through print_man_node().
    direct_word_operands: bool,
    // An actual term_vspace() from empty TEXT asserted an empty output row.
    // The literal owner must distinguish it from a trailing term_newln().
    asserted_vertical_row: bool,
    // A cell executed by this builder, separate from an occupied formatter
    // row inherited from a detached HEAD or a prior output owner.
    produced_formatter_cell: CellProduction,
    // Semantic annotations begin after the first operand's executed auto
    // separator, independently of any authored leading blank glyphs.
    pending_output_scope_prefixes: Vec<String>,
    pub(in crate::mandoc) execution: InlineExecutionState,
    /// A detached definition HEAD occupies the native formatter row even
    /// though its term lives in a different IR output container.
    external_head_row_pending: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LeadingLineBoundary {
    None,
    AtVisibleCheckpoint(u64),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SourceLineObservation {
    Disabled,
    NoFill,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CellProduction {
    None,
    Produced,
}

/// Text execution registers have a different lifetime from an IR segment.
/// A paragraph, literal row, or nested output owner may drain `nodes` while
/// the formatter continues to execute the same source stream.
#[derive(Clone)]
// The formatter registers are independent native flags, not a state chart.
#[allow(clippy::struct_excessive_bools)]
pub(in crate::mandoc) struct InlineExecutionState {
    pub(in crate::mandoc) escape_coverage: crate::mandoc::escape_coverage::EscapeCoverage,
    pub(in crate::mandoc) boundary: PendingBoundary,
    spacing: SpacingMode,
    last_visible_character: Option<char>,
    has_printable_content: bool,
    // Accepted native visible-glyph execution advances independently of generated
    // padding, formatter cells, and IR owner drains. Definition BODY checks
    // the increment made by its current source node.
    visible_glyph_epoch: u64,
    // Unlike `has_printable_content`, this is reset at each real formatter
    // row boundary and does not count a pending zero-advance glyph.
    formatter_column: FormatterColumn,
    empty_word: bool,
    trailing_output: TrailingOutput,
    pending_breakable_spaces: usize,
    pending_field_spaces: usize,
    pending_field_gap_origin: definition::PendingFieldGapOrigin,
    pending_line_indent: usize,
    word_end_break: WordEndBreak,
    /// The pending `\p` was separated from the last graph by a breakable
    /// blank: the restarted pass rejects at the next word's automatic
    /// separator (term.c:143-146) even after the accepted prefix closed its
    /// own row.
    word_end_break_separated: bool,
    /// The first observed line request and its accepted glyph checkpoint.
    /// A BODY handler may execute both a glyph and a subsequent boundary;
    /// this receipt keeps their order when its output owner returns.
    leading_line_boundary: LeadingLineBoundary,
    pub(in crate::mandoc) vertical_space_debt: u16,
    // A cached execution hint for completed empty rows. The active output
    // owner carries their actual private receipts: this shared hint cannot
    // assign a LiteralFlow row to an otherwise empty ParagraphFlow drain.
    completed_vertical_rows: u16,
    pub(in crate::mandoc) keep: KeepState,
    pub(in crate::mandoc) font: FontState,
    pub(in crate::mandoc) macro_set: MacroSet,
    pub(in crate::mandoc) zero_advance: ZeroAdvanceState,
    // A nested inline scope can resolve a `\\z` glyph that was armed by its
    // parent.  Preserve that boundary fact when the scope returns its nodes:
    // otherwise the parent would invent a word separator before the
    // overwriting glyph.
    zero_advance_joined: bool,
    // The formatter's final next-word decision differs from the physical
    // source-line state: `\\c` may keep reading the input line while a
    // generated delimiter or container close releases the next formatter
    // word.  ParagraphFlow consumes only this
    // post-execution result.
    final_word_join: Option<bool>,
    final_source_continuation: Option<bool>,
    execution_epoch: u64,
    author_execution: Option<AuthorExecution>,
    /// Native tag/hang field geometry exists only in a definition head.
    /// Ordinary paragraphs keep word and row events without field widths.
    definition: Option<DefinitionFieldState>,
    /// Device columns survive a detached HEAD's output owner, while its
    /// scoped field flags and margin do not (mdoc_term.c:437-439,961-963).
    /// A real plain-row flush consumes this handoff before another owner.
    detached_device_row: Option<DetachedDeviceRow>,
    /// Active terminal tab settings persist across output owners and fields.
    tab_stops: Arc<TabStops>,
    last_tab_source_node: Option<u32>,
    last_executed_source_line: Option<u32>,
    /// Detached definition HEADs use the same `NODE_LINE` entry rule as the
    /// block driver while their output is collected in a term field.
    observe_no_fill_source_lines: SourceLineObservation,
    /// `TERMP_BRNEVER` mirror (term.c:143-144): a `NODE_NOFILL` subtree is
    /// printing, so `term_fill()` runs with an infinite target and no pass
    /// can end a device row.
    pub(in crate::mandoc) no_fill_word_active: bool,
    /// A HANG head that filled its capacity under a cleared
    /// `TERMP_NOBREAK` reaches the body column with no trailspace
    /// (term.c:250-253 with 205-207): the next word concatenates directly,
    /// like the `TERMP_NOSPACE` left by the request's own `term_newln()`
    /// (`roff_term.c:78`).
    pub(in crate::mandoc) native_word_writes: Option<Vec<field_buffer::FieldWrite>>,
    pub(in crate::mandoc) native_word_boundary: Option<PendingBoundary>,
    pub(in crate::mandoc) native_word_owner: Option<String>,
    native_owner_serial: u64,
    pub(in crate::mandoc) concat_next_word: bool,
    /// A `\p` marker met a surviving breakable blank before this flush
    /// unit recorded any graph (term.c:143-146 with 233-237): the whole
    /// unprinted remainder of the unit is wiped. Words appended while
    /// set contribute no projection until the next real row retirement.
    pub(in crate::mandoc) wipe_remainder: bool,
    /// The plain-flow native flush unit: the same ordered cell buffer the
    /// definition-field path feeds (`tcol->buf`, term.c). Ordinary paragraphs
    /// have no author field, but their `term_fill()` pass arithmetic —
    /// marker breaks, blank consumption, and the `nbr == 0` rejection
    /// (term.c:143-146 with 233-237) — runs over exactly this buffer.
    pub(in crate::mandoc::inline) flush_unit: field_buffer::FieldBuffer,
    /// Word anchors of the plain flush unit (cell start, IR marker, content
    /// start), mirroring `DefinitionFieldState::field_word_anchors`.
    pub(in crate::mandoc::inline) flush_unit_anchors: Vec<NativeWordAnchor>,
    /// IR index where the plain flush unit's unprinted suffix starts; the
    /// rejection interval trim operates from here (the plain analogue of
    /// `AuthorExecution::field_output_start`).
    pub(in crate::mandoc::inline) flush_unit_output_start: usize,
    /// A zero-width graph (`\&`, NBRZW recovery) occupied the current row;
    /// it arms `graph` in `term_fill()` without printing (term.c:349).
    pub(in crate::mandoc) row_zero_graph: bool,
    /// Whether `concat_next_word` was armed by a filled cleared field
    /// (term.c:250-253) rather than the request's `TERMP_NOSPACE`
    /// (`roff_term.c:78`); the two arm the same no-separator word but
    /// carry different `HeadBodyRelation` semantics.
    pub(in crate::mandoc) concat_flush_source: bool,
    /// The most recent word consumed `concat_next_word` while a definition
    /// BODY was waiting for its first visible content.
    pub(in crate::mandoc) concat_consumed_for_body: bool,
    /// The most recent word consumed a flush-armed `concat_next_word`
    /// (`term.c:250-253`) while a definition BODY waited for content.
    pub(in crate::mandoc) flush_consumed_for_body: bool,
    pub(in crate::mandoc) scope_posts: crate::mandoc::containers::ScopePostState,
    /// A column list's native device state outlives its cell/display IR
    /// owners. Only Bl return retires this scope, not a paragraph drain.
    column_output_depth: usize,
}

impl std::ops::Deref for InlineBuilder {
    type Target = InlineExecutionState;

    fn deref(&self) -> &Self::Target {
        &self.execution
    }
}

impl std::ops::DerefMut for InlineBuilder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.execution
    }
}

#[derive(Clone, Copy)]
struct AuthorExecution {
    flow: crate::mandoc::formatter::AuthorFlow,
    authors_section: bool,
    break_effect: AuthorBreakEffect,
    field_output_start: usize,
}

#[derive(Clone, Copy)]
struct DetachedDeviceRow {
    viscol: usize,
    minbl: usize,
    page_origin_printed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum AuthorBreakEffect {
    Line,
    Field {
        gap_cells: u8,
        body_width_columns: u16,
        /// The head field's content capacity `rmargin - offset`
        /// (mdoc_term.c:846-856): the `vtarget` term.c:134-136 selects for
        /// every pass once a request cleared `TERMP_NOBREAK`. Run-in styles
        /// never shorten the right margin, so they carry `u16::MAX`.
        field_width_columns: u16,
        /// Upstream pad/break flags for the HEAD field, set per list kind
        /// exactly as `mdoc_term.c::termp_it_pre()` does; the row decisions
        /// in `native_field` consume them.
        flags: native_field::FieldFlags,
    },
}

/// Start of an output slice whose source still executes in the live formatter.
/// Semantic annotation records this boundary without saving execution state.
#[derive(Clone)]
pub(in crate::mandoc) struct OutputCheckpoint {
    position: Rc<Cell<usize>>,
    registry: Weak<OutputPositionRegistry>,
}

type OutputPositionRegistry = RefCell<Vec<Weak<Cell<usize>>>>;

impl Drop for OutputCheckpoint {
    fn drop(&mut self) {
        if Rc::strong_count(&self.position) == 1
            && let Some(registry) = self.registry.upgrade()
        {
            let position = Rc::downgrade(&self.position);
            registry
                .borrow_mut()
                .retain(|registered| !registered.ptr_eq(&position));
        }
    }
}

/// Formatter execution state that crosses a private presentation scope.
/// Keeping these facts together prevents an atomic wrapper from preserving
/// zero-advance state while silently dropping a word-end break or final
/// source-line decision.
pub(in crate::mandoc) struct PreservedInlineState {
    pub(in crate::mandoc) zero_advance: ZeroAdvanceState,
    pub(in crate::mandoc) word_end_break: bool,
    /// A HEAD field still open at the ownership split (NOBREAK run-in
    /// heads, `mdoc_term.c::termp_it_pre()`). The BODY session continues
    /// this field instead of starting an unconfigured stream.
    pub(in crate::mandoc) definition_field: Option<definition::PreservedDefinitionField>,
    pub(in crate::mandoc) source_continuation: Option<bool>,
    pub(in crate::mandoc) formatter_cell_occupied: bool,
    pub(in crate::mandoc) pending_line_indent: usize,
    pub(in crate::mandoc) pending_definition_indent: Option<usize>,
    pub(in crate::mandoc) last_executed_source_line: Option<u32>,
}

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct SourceFragmentState {
    final_word_join: Option<bool>,
    final_source_continuation: Option<bool>,
    execution_epoch: u64,
}

mod font_state;
pub(in crate::mandoc) use font_state::{FontScope, FontState};

#[cfg(test)]
mod font_state_tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum PendingBoundary {
    Ordinary,
    Tight,
    /// A committed device field already projects its separator. The next
    /// IR word joins that padding, but native MC cleared NOSPACE and still
    /// buffers `term_word()`'s automatic blank (roff_term.c:147-151).
    CommittedField,
    PrefixJoin,
    Preserved,
    Continued,
    Kept,
}

impl PendingBoundary {
    const fn is_tight(self) -> bool {
        matches!(self, Self::Tight | Self::PrefixJoin | Self::CommittedField)
    }

    const fn is_native_tight(self) -> bool {
        matches!(self, Self::Tight | Self::PrefixJoin)
    }

    const fn is_nonbreaking(self) -> bool {
        self.is_tight() || matches!(self, Self::Kept)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpacingMode {
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::inline::flow) enum WordEndBreak {
    Clear,
    Pending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FormatterColumn {
    Origin,
    Advanced,
}

/// The formatter meaning of the final projected cell.
///
/// These states deliberately do not derive meaning from the final Unicode
/// character.  CVS distinguishes ordinary word padding (trimmed by
/// `term_field()`), a boundary already supplied by field geometry, and a
/// non-breaking blank glyph that is followed by the next word's own
/// automatic boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum TrailingOutput {
    None,
    NonBlank,
    BreakableBlank(usize),
    BoundaryBlank,
    FieldBlank(usize),
    FixedBlank,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct KeepState {
    phase: KeepPhase,
}

impl KeepState {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            phase: KeepPhase::Inactive,
        }
    }

    pub(in crate::mandoc) fn enter(&mut self) {
        self.phase = KeepPhase::PreKeep;
    }

    pub(in crate::mandoc) fn exit(&mut self) {
        self.phase = KeepPhase::Inactive;
    }

    const fn keeping(self) -> bool {
        matches!(self.phase, KeepPhase::Keep)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeepPhase {
    Inactive,
    PreKeep,
    Keep,
}

impl SpacingMode {
    const fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    const fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

impl From<bool> for SpacingMode {
    fn from(enabled: bool) -> Self {
        Self::from_enabled(enabled)
    }
}

/// Semantic boundary between two inline fragments in filled roff mode.
///
/// Roff distinguishes ordinary source wrapping from an input line whose first
/// text character is whitespace.  The former fills as a word boundary; the
/// latter starts a new output line.  Keeping that distinction here prevents
/// renderers from having to rediscover formatter semantics from flattened
/// text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum FilledBoundary {
    SameLine,
    Word,
    LineBreak,
}
