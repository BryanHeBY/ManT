//! Filled flow preserves font, spacing and pending boundaries across scopes.
use super::{Font, ZeroAdvanceState, needs_boundary_space, push_text, updated_spacing};
use mant_ir::Inline;
use mant_ir::{first_visible_character, has_printable_character, last_visible_character};

mod execution;
mod field;
mod no_fill;
mod output;

pub(in crate::mandoc) use no_fill::{NoFillInlineState, lower_no_fill_fragment_with_formatter};

pub(super) use output::trailing_ascii_spaces;
use output::trim_trailing_breakable_spaces;

pub(in crate::mandoc) struct InlineBuilder {
    nodes: Vec<Inline>,
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

/// Text execution registers have a different lifetime from an IR segment.
/// A paragraph, literal row, or nested output owner may drain `nodes` while
/// the formatter continues to execute the same source stream.
#[derive(Clone)]
pub(in crate::mandoc) struct InlineExecutionState {
    boundary: PendingBoundary,
    spacing: SpacingMode,
    last_visible_character: Option<char>,
    has_printable_content: bool,
    // Unlike `has_printable_content`, this is reset at each real formatter
    // row boundary and does not count a pending zero-advance glyph.
    formatter_column: FormatterColumn,
    empty_word: bool,
    trailing_output: TrailingOutput,
    pending_breakable_spaces: usize,
    pending_field_spaces: usize,
    pending_line_indent: usize,
    word_end_break: WordEndBreak,
    /// An executed line request before the first visible word of this IR
    /// segment. Definition BODY checkpoints consume this source-order fact.
    leading_line_boundary: LeadingLineBoundary,
    pub(in crate::mandoc) vertical_space_debt: u16,
    pub(in crate::mandoc) keep: KeepState,
    pub(in crate::mandoc) font: FontState,
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
    last_executed_source_line: Option<u32>,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DefinitionOutcome(u8);

#[derive(Clone, Default)]
struct DefinitionFieldState {
    pending_indent: Option<usize>,
    outcome: DefinitionOutcome,
    no_break: Option<NoBreakField>,
}

impl InlineBuilder {
    fn definition_state_mut(&mut self) -> &mut DefinitionFieldState {
        self.definition
            .get_or_insert_with(DefinitionFieldState::default)
    }

    fn pending_definition_indent(&self) -> Option<usize> {
        self.definition
            .as_ref()
            .and_then(|state| state.pending_indent)
    }

    fn set_pending_definition_indent(&mut self, indent: Option<usize>) {
        self.definition_state_mut().pending_indent = indent;
    }
}

impl DefinitionOutcome {
    const FIELD_EXITED: u8 = 1;
    const BODY_GAP_CONSUMED: u8 = 2;

    fn mark_field_exited(&mut self) {
        self.0 |= Self::FIELD_EXITED;
    }

    fn mark_body_gap_consumed(&mut self) {
        self.0 |= Self::BODY_GAP_CONSUMED;
    }

    fn clear_body_gap_consumed(&mut self) {
        self.0 &= !Self::BODY_GAP_CONSUMED;
    }

    const fn field_exited(self) -> bool {
        self.0 & Self::FIELD_EXITED != 0
    }

    const fn body_gap_consumed(self) -> bool {
        self.0 & Self::BODY_GAP_CONSUMED != 0
    }
}

/// Native field state left behind by `roff_term_pre_mc()`.
///
/// CVS clears `NOBREAK` and `NOSPACE` after flushing, but deliberately keeps
/// `BRIND`, `HANG`, and the list field geometry for a following request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NoBreakField {
    output_end_before_separator: usize,
    resumed_output_start: usize,
    resumed_execution_epoch: u64,
    field_width: usize,
    body_width: usize,
    trailspace_cells: usize,
    separator_cells: usize,
    style: DefinitionFieldStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DefinitionFieldStyle {
    Tag,
    Hang,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum AuthorBreakEffect {
    Line,
    Field {
        gap_cells: u8,
        body_width_columns: u16,
        wraps: bool,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    scope_depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TablePresentationFont {
    current: Font,
    previous: Font,
}

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct FontScope {
    current: Font,
    display_current: Font,
    table_presentation_current: Option<Font>,
    depth: usize,
}

impl FontState {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            current: Font::Regular,
            previous: Font::Regular,
            display_current: Font::Regular,
            display_previous: Font::Regular,
            table_presentation: None,
            heading_bold_italic: false,
            scope_depth: 0,
        }
    }

    pub(in crate::mandoc) const fn display_current(self) -> Font {
        match self.table_presentation {
            Some(presentation) => presentation.current,
            None => self.display_current,
        }
    }

    pub(super) fn select(&mut self, font: Font) {
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

    pub(super) fn restore(&mut self) {
        std::mem::swap(&mut self.current, &mut self.previous);
        std::mem::swap(&mut self.display_current, &mut self.display_previous);
        if let Some(presentation) = &mut self.table_presentation {
            std::mem::swap(&mut presentation.current, &mut presentation.previous);
        }
    }

    /// mdoc font scopes push a selection. Popping restores the saved current
    /// font, not the previous-selection register used by `\\fP` and `.ft P`.
    pub(in crate::mandoc) fn push_scope(&mut self, font: Font) -> FontScope {
        let saved = self.checkpoint();
        self.scope_depth += 1;
        self.select(font);
        saved
    }

    pub(in crate::mandoc) fn checkpoint(&self) -> FontScope {
        FontScope {
            current: self.current,
            display_current: self.display_current,
            table_presentation_current: self.table_presentation.map(|font| font.current),
            depth: self.scope_depth,
        }
    }

    pub(in crate::mandoc) fn pop_scope(&mut self, saved: FontScope) {
        // term.c::term_fontpopq() is a no-op if a crossed BODY has already
        // unwound this font depth.  A later return must not revive it.
        if self.scope_depth <= saved.depth {
            return;
        }
        self.scope_depth = saved.depth;
        self.current = saved.current;
        self.display_current = saved.display_current;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum PendingBoundary {
    Ordinary,
    Tight,
    PrefixJoin,
    Preserved,
    Continued,
    Kept,
}

impl PendingBoundary {
    const fn is_tight(self) -> bool {
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
enum WordEndBreak {
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
            execution,
            external_head_row_pending: false,
        }
    }

    pub(in crate::mandoc) fn inherit_external_head_row(&mut self, pending: bool) {
        self.external_head_row_pending = pending;
    }

    pub(in crate::mandoc) fn into_parts(self) -> (Vec<Inline>, InlineExecutionState) {
        (self.nodes, self.execution)
    }
}

impl InlineExecutionState {
    pub(in crate::mandoc) fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::from_enabled(spacing_enabled),
            last_visible_character: None,
            has_printable_content: false,
            formatter_column: FormatterColumn::Origin,
            empty_word: false,
            trailing_output: TrailingOutput::None,
            pending_breakable_spaces: 0,
            pending_field_spaces: 0,
            pending_line_indent: 0,
            word_end_break: WordEndBreak::Clear,
            leading_line_boundary: LeadingLineBoundary::None,
            vertical_space_debt: 0,
            keep: KeepState::new(),
            font: FontState::new(),
            zero_advance: ZeroAdvanceState::new(),
            zero_advance_joined: false,
            final_word_join: None,
            final_source_continuation: None,
            execution_epoch: 0,
            author_execution: None,
            definition: None,
            last_executed_source_line: None,
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
        self.leading_line_boundary = LeadingLineBoundary::None;
        self.zero_advance = ZeroAdvanceState::new();
        self.zero_advance.inherit_armed(armed_zero_advance);
        self.zero_advance_joined = false;
        self.final_word_join = None;
        self.final_source_continuation = None;
        self.execution_epoch = 0;
        self.definition = None;
        self.last_executed_source_line = None;
        if let Some(author) = &mut self.author_execution {
            // The author mode is a formatter register; this index belongs to
            // the drained IR segment and cannot cross its output boundary.
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

    pub(in crate::mandoc) fn has_executed_visible_content(&self) -> bool {
        self.has_printable_content || self.zero_advance.has_printable_pending_glyph()
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
