//! Filled flow preserves font, spacing and pending boundaries across scopes.
use super::{Font, ZeroAdvanceState, needs_boundary_space, push_text, updated_spacing};
use libmandoc_rs::MacroSet;
use mant_ir::Inline;
use mant_ir::{first_visible_character, has_printable_character, last_visible_character};

mod definition;
pub(in crate::mandoc) use definition::DefinitionGeometryCheckpoint;
mod execution;
pub(in crate::mandoc) mod field_buffer;
use definition::DefinitionFieldState;
mod native_field;
mod tab_stops;
use std::sync::Arc;
use tab_stops::TabStops;
mod no_fill;
pub(in crate::mandoc) use native_field::{FieldFlag, FieldFlags};
mod output;
pub(in crate::mandoc::inline) use output::INTERNAL_FIELD_WORD;

pub(in crate::mandoc) use no_fill::{NoFillInlineState, lower_no_fill_fragment_with_formatter};

pub(super) use output::trailing_ascii_spaces;
pub(in crate::mandoc) use output::trim_trailing_breakable_spaces;

pub(in crate::mandoc) struct InlineBuilder {
    nodes: Vec<Inline>,
    // Macro handlers such as pre_alternate() call term_word() directly on
    // operands instead of visiting those TEXT nodes through print_man_node().
    direct_word_operands: bool,
    // An actual term_vspace() from empty TEXT asserted an empty output row.
    // The literal owner must distinguish it from a trailing term_newln().
    asserted_vertical_row: bool,
    // A cell executed by this builder, separate from an occupied formatter
    // row inherited from a detached HEAD or a prior output owner.
    produced_formatter_cell: CellProduction,
    definition_term_breaks: Vec<usize>,
    pub(in crate::mandoc) execution: InlineExecutionState,
    /// A detached definition HEAD occupies the native formatter row even
    /// though its term lives in a different IR output container.
    external_head_row_pending: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LeadingLineBoundary {
    None,
    BeforeVisibleWord,
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
    pub(in crate::mandoc) boundary: PendingBoundary,
    spacing: SpacingMode,
    last_visible_character: Option<char>,
    has_printable_content: bool,
    // Native visible-glyph execution advances independently of generated
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
    pending_line_indent: usize,
    word_end_break: WordEndBreak,
    /// The pending `\p` was separated from the last graph by a breakable
    /// blank: the restarted pass rejects at the next word's automatic
    /// separator (term.c:143-146) even after the accepted prefix closed its
    /// own row.
    word_end_break_separated: bool,
    /// An executed line request before the first visible word of this IR
    /// segment. Definition BODY checkpoints consume this source-order fact.
    leading_line_boundary: LeadingLineBoundary,
    pub(in crate::mandoc) vertical_space_debt: u16,
    // Rows already emitted by an empty TEXT's term_vspace(), still at the
    // tail of the current IR segment. A structural drain must project these
    // rows even though ordinary terminal line endings are trim-eligible.
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
    pub(super) flush_unit: field_buffer::FieldBuffer,
    /// Word anchors of the plain flush unit (cell start, IR marker, content
    /// start), mirroring `DefinitionFieldState::field_word_anchors`.
    pub(in crate::mandoc) flush_unit_anchors: Vec<(usize, String, usize)>,
    /// IR index where the plain flush unit's unprinted suffix starts; the
    /// rejection interval trim operates from here (the plain analogue of
    /// `AuthorExecution::field_output_start`).
    pub(in crate::mandoc) flush_unit_output_start: usize,
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

/// An output transaction around source that must execute before its compact
/// presentation is chosen.
#[derive(Clone)]
pub(in crate::mandoc) struct OutputTransaction {
    rollback: OutputRollback,
    inbound: InboundExecution,
}

/// State owned by the compacted output rather than the continuing formatter.
///
/// This includes queued presentation cells that must be rematerialized by a
/// fallback after the hidden spelling is removed.  Formatter registers not
/// listed here remain live by construction and cannot be accidentally reset
/// when a new execution field is added to [`InlineBuilder`].
#[derive(Clone)]
struct OutputRollback {
    node_count: usize,
    native_field_position: Option<(u64, usize)>,
    last_visible_character: Option<char>,
    has_printable_content: bool,
    trailing_output: TrailingOutput,
    /// Padding queued solely for the spelling being compacted. A fallback
    /// rematerializes this boundary explicitly; it is not persistent
    /// formatter execution state.
    pending_breakable_spaces: usize,
    pending_field_spaces: usize,
}

/// Execution facts at transaction entry.  Replacement output may consult
/// them, but rollback never restores them: hidden source remains executed.
#[derive(Clone, Copy)]
struct InboundExecution {
    boundary: PendingBoundary,
    final_source_continuation: Option<bool>,
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

/// Roff remembers the previous selection independently of the current font.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct FontState {
    /// Terminal execution registers used by `\\fP` and `.ft P`.
    pub(super) current: Font,
    pub(super) previous: Font,
    /// The HTML/IR spelling of a font can retain constant width even when
    /// the terminal renderer executes CR as roman, CB as bold, and CI as
    /// italic. Keep that presentation out of the execution registers.
    display_current: Font,
    display_previous: Font,
    /// `tbl_html.c` has an independent display selection for CR cells, but
    /// `tbl_term.c` does not push a terminal font for them. Keep the temporary
    /// HTML pair separate from both persistent register pairs.
    table_presentation: Option<TablePresentationFont>,
    heading_bold_italic: bool,
    /// CVS term.c keeps mdoc fonts in a stack.  An ordinary `.ft` changes
    /// the current slot without pushing it, so BODY unwinding must restore
    /// only scopes pushed after that BODY was entered.
    // Saved lower slots; `current` is the mutable active slot. A `.ft`
    // replaces that slot, and term_fontpopq() only changes the active depth.
    font_stack: Vec<(Font, Font)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TablePresentationFont {
    current: Font,
    previous: Font,
}

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct FontScope {
    depth: usize,
    table_presentation_current: Option<Font>,
}

impl FontState {
    pub(in crate::mandoc) fn new() -> Self {
        Self {
            current: Font::Regular,
            previous: Font::Regular,
            display_current: Font::Regular,
            display_previous: Font::Regular,
            table_presentation: None,
            heading_bold_italic: false,
            font_stack: Vec::new(),
        }
    }

    pub(in crate::mandoc) const fn display_current(&self) -> Font {
        match self.table_presentation {
            Some(presentation) => presentation.current,
            None => self.display_current,
        }
    }

    pub(in crate::mandoc) fn select(&mut self, font: Font) {
        let display = if self.heading_bold_italic && font == Font::Emphasis {
            Font::StrongEmphasis
        } else {
            font
        };
        self.previous = self.current;
        let terminal = match font {
            Font::Code => Font::Regular,
            Font::CodeStrong => Font::Strong,
            Font::CodeEmphasis => Font::Emphasis,
            font => font,
        };
        self.current = if self.heading_bold_italic && terminal == Font::Emphasis {
            Font::StrongEmphasis
        } else {
            terminal
        };
        self.display_previous = self.display_current;
        self.display_current = display;
        if let Some(presentation) = &mut self.table_presentation {
            presentation.previous = presentation.current;
            presentation.current = display;
        }
    }

    /// Every printable man(7) node enters and leaves through the generic
    /// `print_man_node()` font replacement, even when its macro has no post
    /// handler. Replacing Roman also updates the previous-font register.
    pub(in crate::mandoc) fn man_text_boundary(&mut self) {
        self.select(Font::Regular);
    }

    pub(super) fn restore(&mut self) {
        std::mem::swap(&mut self.current, &mut self.previous);
        std::mem::swap(&mut self.display_current, &mut self.display_previous);
        if let Some(presentation) = &mut self.table_presentation {
            std::mem::swap(&mut presentation.current, &mut presentation.previous);
        }
    }

    /// mdoc font scopes push a slot. The previous-font register is separate.
    pub(in crate::mandoc) fn push_scope(&mut self, font: Font) -> FontScope {
        let saved = self.checkpoint();
        self.font_stack.push((self.current, self.display_current));
        self.select(font);
        saved
    }

    pub(in crate::mandoc) fn checkpoint(&self) -> FontScope {
        FontScope {
            depth: self.font_stack.len(),
            table_presentation_current: self.table_presentation.map(|font| font.current),
        }
    }

    pub(in crate::mandoc) fn pop_scope(&mut self, saved: FontScope) {
        // term.c::term_fontpopq() is a no-op if a crossed BODY has already
        // unwound this font depth.  A later return must not revive it.
        if self.font_stack.len() <= saved.depth {
            return;
        }
        (self.current, self.display_current) = self.font_stack[saved.depth];
        self.font_stack.truncate(saved.depth);
        if let (Some(presentation), Some(current)) = (
            &mut self.table_presentation,
            saved.table_presentation_current,
        ) {
            presentation.current = current;
        }
    }

    /// CVS `tbl_term.c::tbl_word()` leaves terminal registers untouched for
    /// a CR layout cell, while `tbl_html.c::print_tbl()` still displays it in
    /// constant width. The subsequent in-cell `\\f` commands update both
    /// projections independently.
    pub(in crate::mandoc) fn begin_code_table_cell(&mut self) {
        debug_assert!(self.table_presentation.is_none());
        self.table_presentation = Some(TablePresentationFont {
            current: Font::Code,
            previous: self.display_current,
        });
    }

    pub(in crate::mandoc) fn end_code_table_cell(&mut self) {
        debug_assert!(self.table_presentation.is_some());
        self.table_presentation = None;
    }

    /// Enter CVS `termp_sh_pre()`/`termp_ss_pre()` font execution.
    ///
    /// Heading presentation is structural in the IR, but the terminal
    /// formatter still pushes bold and enables `fontibi`: an inner `\fI`
    /// selects bold-emphasis and updates the independent previous-font
    /// register.  The returned pair restores only the current stack and the
    /// mode flag; `previous` deliberately survives the scope.
    pub(in crate::mandoc) fn push_heading_scope(&mut self) -> (FontScope, bool) {
        let saved_mode = self.heading_bold_italic;
        let saved_font = self.push_scope(Font::Strong);
        self.heading_bold_italic = true;
        (saved_font, saved_mode)
    }

    pub(in crate::mandoc) fn pop_heading_scope(&mut self, saved: (FontScope, bool)) {
        self.pop_scope(saved.0);
        self.heading_bold_italic = saved.1;
    }

    /// Execute the replacement-font lifecycle used by man(7) SH/SS nodes.
    ///
    /// Unlike mdoc's font stack, `print_man_node()` replaces the font before
    /// and after each structural block/head/body node.  The two final resets
    /// leave both the current and previous-font registers at regular.
    pub(in crate::mandoc) fn begin_man_heading(&mut self) {
        self.select(Font::Regular);
        self.select(Font::Strong);
        self.heading_bold_italic = true;
    }

    pub(in crate::mandoc) fn end_man_heading(&mut self) {
        self.heading_bold_italic = false;
        self.select(Font::Regular);
        self.select(Font::Regular);
    }
}

#[cfg(test)]
mod font_state_tests {
    use super::{Font, FontState};

    #[test]
    fn crossed_body_pop_uses_live_lower_slot_and_keeps_previous_register() {
        // The exact Ao/.ft B/Bf/.Ac source was checked with pinned CVS
        // -Tascii. term.c::term_fontrepl() changes the active fontq slot;
        // term_fontpopq() only changes the index at the original BODY close.
        let mut font = FontState::new();
        let outer = font.checkpoint();
        font.select(Font::Strong);
        let inner = font.push_scope(Font::Emphasis);
        assert_eq!(font.current, Font::Emphasis);
        font.pop_scope(outer);
        assert_eq!(font.current, Font::Strong);
        assert_eq!(font.previous, Font::Strong);
        font.pop_scope(inner);
        assert_eq!(font.current, Font::Strong);
    }
}

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

impl InlineBuilder {
    #[cfg(test)]
    pub(in crate::mandoc) fn new() -> Self {
        Self::with_spacing(true)
    }

    pub(in crate::mandoc) fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            nodes: Vec::new(),
            direct_word_operands: false,
            asserted_vertical_row: false,
            produced_formatter_cell: CellProduction::None,
            definition_term_breaks: Vec::new(),
            execution: InlineExecutionState::with_spacing(spacing_enabled),
            external_head_row_pending: false,
        }
    }

    pub(in crate::mandoc) fn from_parts(
        nodes: Vec<Inline>,
        execution: InlineExecutionState,
    ) -> Self {
        Self {
            nodes,
            direct_word_operands: false,
            asserted_vertical_row: false,
            produced_formatter_cell: CellProduction::None,
            definition_term_breaks: Vec::new(),
            execution,
            external_head_row_pending: false,
        }
    }

    pub(in crate::mandoc) fn inherit_external_head_row(&mut self, pending: bool) {
        self.external_head_row_pending = pending;
    }

    pub(in crate::mandoc) fn with_direct_word_operands<R>(
        &mut self,
        append: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.direct_word_operands, true);
        let result = append(self);
        self.direct_word_operands = previous;
        result
    }

    pub(in crate::mandoc) const fn asserted_vertical_row(&self) -> bool {
        self.asserted_vertical_row
    }

    pub(in crate::mandoc) const fn produced_formatter_cell(&self) -> bool {
        matches!(self.produced_formatter_cell, CellProduction::Produced)
    }

    fn note_produced_formatter_cell(&mut self, produced: bool) {
        if produced {
            self.produced_formatter_cell = CellProduction::Produced;
        }
    }

    pub(in crate::mandoc) fn visits_empty_text_as_space(&self, node: &libmandoc_rs::Node) -> bool {
        if self.direct_word_operands {
            return false;
        }
        match self.execution.macro_set {
            MacroSet::Man => true,
            MacroSet::Mdoc => node.flags.line_start,
            MacroSet::None => false,
        }
    }

    pub(in crate::mandoc) fn mark_definition_term_break(&mut self) {
        if matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))
            && self.definition_term_breaks.last() != Some(&(self.nodes.len() - 1))
        {
            self.definition_term_breaks.push(self.nodes.len() - 1);
        }
    }

    pub(in crate::mandoc) fn take_definition_term_breaks(&mut self) -> Vec<usize> {
        // The native ownership ledger is private. Resolve the row markers
        // against the returned IR after private word anchors are removed.
        let private_positions: Vec<usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                matches!(node,
                Inline::Anchor { id, .. } if id.as_str().starts_with("\0mant:field-"))
                .then_some(index)
            })
            .collect();
        let mut private_cursor = 0;
        std::mem::take(&mut self.definition_term_breaks)
            .into_iter()
            .map(|index| {
                while private_positions
                    .get(private_cursor)
                    .is_some_and(|&marker| marker < index)
                {
                    private_cursor += 1;
                }
                index.saturating_sub(private_cursor)
            })
            .collect()
    }

    pub(in crate::mandoc) fn into_parts(self) -> (Vec<Inline>, InlineExecutionState) {
        (self.nodes, self.execution)
    }
}

impl InlineExecutionState {
    pub(in crate::mandoc) fn source_row_continues(&self) -> bool {
        self.final_source_continuation.unwrap_or(false)
    }

    pub(in crate::mandoc) fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::from_enabled(spacing_enabled),
            last_visible_character: None,
            has_printable_content: false,
            visible_glyph_epoch: 0,
            formatter_column: FormatterColumn::Origin,
            empty_word: false,
            trailing_output: TrailingOutput::None,
            pending_breakable_spaces: 0,
            pending_field_spaces: 0,
            pending_line_indent: 0,
            word_end_break: WordEndBreak::Clear,
            word_end_break_separated: false,
            leading_line_boundary: LeadingLineBoundary::None,
            vertical_space_debt: 0,
            completed_vertical_rows: 0,
            keep: KeepState::new(),
            font: FontState::new(),
            macro_set: MacroSet::None,
            zero_advance: ZeroAdvanceState::new(),
            zero_advance_joined: false,
            final_word_join: None,
            final_source_continuation: None,
            execution_epoch: 0,
            author_execution: None,
            definition: None,
            tab_stops: Arc::new(TabStops::default()),
            last_tab_source_node: None,
            last_executed_source_line: None,
            observe_no_fill_source_lines: SourceLineObservation::Disabled,
            no_fill_word_active: false,
            wipe_remainder: false,
            row_zero_graph: false,
            native_word_writes: None,
            native_word_boundary: None,
            flush_unit: field_buffer::FieldBuffer::default(),
            flush_unit_anchors: Vec::new(),
            flush_unit_output_start: 0,
            concat_next_word: false,
            concat_flush_source: false,
            concat_consumed_for_body: false,
            flush_consumed_for_body: false,
            scope_posts: crate::mandoc::containers::ScopePostState::default(),
        }
    }

    fn reset_paragraph_segment(&mut self, armed_zero_advance: bool) {
        self.boundary = PendingBoundary::Ordinary;
        self.last_visible_character = None;
        self.has_printable_content = false;
        self.formatter_column = FormatterColumn::Origin;
        self.empty_word = false;
        self.trailing_output = TrailingOutput::None;
        self.pending_breakable_spaces = 0;
        self.pending_field_spaces = 0;
        self.pending_line_indent = 0;
        self.word_end_break = WordEndBreak::Clear;
        self.wipe_remainder = false;
        self.row_zero_graph = false;
        self.leading_line_boundary = LeadingLineBoundary::None;
        self.zero_advance.reset_projection(armed_zero_advance);
        self.zero_advance_joined = false;
        self.final_word_join = None;
        // TERMP_NONEWLINE is a native execution register, not an IR segment
        // property. A paragraph drain does not consume a preceding \c.
        self.execution_epoch = 0;
        self.definition = None;
        self.last_executed_source_line = None;
        self.completed_vertical_rows = 0;
        if let Some(author) = &mut self.author_execution {
            // The author mode is a formatter register; this index belongs to
            // the drained IR segment and cannot cross its output boundary.
            author.field_output_start = 0;
        }
    }

    /// Retire only indices into a drained IR owner. The native word, row,
    /// and separator registers continue across an mdoc HEAD/BODY split.
    fn retire_output_owner(&mut self) {
        // These observations were already projected into the detached HEAD.
        // In particular, an An -split term_newln() cannot become a second
        // leading break before the BODY's first word.
        self.leading_line_boundary = LeadingLineBoundary::None;
        self.completed_vertical_rows = 0;
        self.pending_line_indent = 0;
        self.zero_advance_joined = false;
        self.final_word_join = None;
        self.execution_epoch = 0;
        self.definition = None;
        self.last_executed_source_line = None;
        if let Some(author) = &mut self.author_execution {
            author.field_output_start = 0;
        }
    }

    pub(in crate::mandoc) fn spacing_enabled(&self) -> bool {
        self.spacing == SpacingMode::Enabled
    }

    pub(in crate::mandoc) fn set_spacing_enabled(&mut self, enabled: bool) {
        self.spacing = SpacingMode::from_enabled(enabled);
    }

    pub(in crate::mandoc) fn has_formatter_cell(&self) -> bool {
        self.formatter_column == FormatterColumn::Advanced
            || self.zero_advance.has_buffered_glyph()
            || self.word_end_break == WordEndBreak::Pending
    }

    pub(in crate::mandoc) fn visible_content_checkpoint(&self) -> (u64, bool) {
        (
            self.visible_glyph_epoch,
            self.zero_advance.has_printable_pending_glyph(),
        )
    }

    pub(in crate::mandoc) fn has_visible_content_since(&self, before: (u64, bool)) -> bool {
        self.visible_glyph_epoch != before.0
            || (!before.1 && self.zero_advance.has_printable_pending_glyph())
    }

    pub(in crate::mandoc) fn has_printable_pending_zero_advance_glyph(&self) -> bool {
        self.zero_advance.has_printable_pending_glyph()
    }

    pub(in crate::mandoc) fn discard_zero_advance_at_row_end(&mut self) {
        self.zero_advance.discard_at_row_end();
    }

    pub(in crate::mandoc) fn take_leading_line_boundary(&mut self) -> bool {
        std::mem::replace(&mut self.leading_line_boundary, LeadingLineBoundary::None)
            == LeadingLineBoundary::BeforeVisibleWord
    }

    pub(in crate::mandoc) fn take_zero_advance_armed(&mut self) -> bool {
        self.zero_advance.take_armed()
    }

    pub(in crate::mandoc) fn inherit_zero_advance_armed(&mut self, armed: bool) {
        self.zero_advance.inherit_armed(armed);
    }

    pub(in crate::mandoc) fn author_flow(&self) -> Option<crate::mandoc::formatter::AuthorFlow> {
        self.author_execution.map(|execution| execution.flow)
    }

    pub(in crate::mandoc) fn set_author_flow(
        &mut self,
        flow: crate::mandoc::formatter::AuthorFlow,
    ) {
        if let Some(execution) = &mut self.author_execution {
            execution.flow = flow;
        } else {
            self.author_execution = Some(AuthorExecution {
                flow,
                authors_section: false,
                break_effect: AuthorBreakEffect::Line,
                field_output_start: 0,
            });
        }
    }
}

impl Default for InlineExecutionState {
    fn default() -> Self {
        Self::with_spacing(true)
    }
}
