//! Filled flow preserves font, spacing and pending boundaries across scopes.
use super::{Font, ZeroAdvanceState, needs_boundary_space, push_text, updated_spacing};
use mant_ir::Inline;
use mant_ir::{first_visible_character, has_printable_character, last_visible_character};

mod execution;
mod field;
mod line;
mod output;

pub(in crate::mandoc) use line::FormatterMachine;
use line::{FlushOutcome, PendingSeparators, SeparatorKind};

pub(super) use output::trailing_ascii_spaces;
use output::trim_trailing_breakable_spaces;

pub(in crate::mandoc) struct InlineBuilder {
    nodes: Vec<Inline>,
    /// Stable formatter-field ownership parallel to `nodes`.  The owner token,
    /// rather than a mutable vector index, identifies projection that may need
    /// geometry applied when CVS eventually flushes the field.
    node_owners: Vec<u64>,
    projection_owner: u64,
    next_projection_owner: u64,
    deferred_indent_owner: Option<u64>,
    geometry_stack: Vec<(usize, usize)>,
    boundary: PendingBoundary,
    spacing: SpacingMode,
    last_visible_character: Option<char>,
    has_printable_content: bool,
    // Unlike `has_printable_content`, this is reset at each real formatter
    // row boundary and does not count a pending zero-advance glyph.
    formatter_line: FormatterMachine,
    empty_word: bool,
    trailing_output: TrailingOutput,
    pending_breakable_spaces: usize,
    pending_line_indent: usize,
    pending_definition_indent: Option<usize>,
    word_end_break: WordEndBreak,
    vertical_space_debt: u16,
    keep: KeepState,
    pub(in crate::mandoc) font: FontState,
    pub(in crate::mandoc) zero_advance: ZeroAdvanceState,
    // A nested inline scope can resolve a `\\z` glyph that was armed by its
    // parent.  Preserve that boundary fact when the scope returns its nodes:
    // otherwise the parent would invent a word separator before the
    // overwriting glyph.
    zero_advance_joined: bool,
    source_cursor: Option<super::source_cursor::SourceCursor>,
    // The formatter's final next-word decision.  This is deliberately *not*
    // the physical source-line state held by `SourceCursor`: `\\c` may keep
    // reading the input line while a generated delimiter or container close
    // releases the next formatter word.  ParagraphFlow consumes only this
    // post-execution result.
    final_word_join: Option<bool>,
    final_source_continuation: Option<bool>,
    execution_epoch: u64,
    author_execution: Option<AuthorExecution>,
    definition_outcome: DefinitionOutcome,
    no_break_field: Option<NoBreakField>,
    last_executed_source_line: Option<u32>,
}
#[derive(Clone, Copy)]
struct AuthorExecution {
    flow: crate::mandoc::formatter::AuthorFlow,
    authors_section: bool,
    break_effect: AuthorBreakEffect,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DefinitionOutcome(u8);

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
    field_width: usize,
    body_width: usize,
    trailspace_cells: usize,
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
    pending_separators: PendingSeparators,
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
    formatter_line: FormatterMachine,
    pub(in crate::mandoc) pending_line_indent: usize,
    pub(in crate::mandoc) pending_definition_indent: Option<usize>,
    pub(in crate::mandoc) last_executed_source_line: Option<u32>,
    executed_tail: ExecutedTail,
}

/// Native layout already executed at the end of an IR-only fragment.
///
/// A definition-head `.br` or `.sp` affects the following body, but is not a
/// second semantic term.  Keep that execution result outside the fragment's
/// nodes and transfer it to the next projection owner without re-executing
/// the request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::mandoc) struct ExecutedTail {
    line_breaks: usize,
}

impl ExecutedTail {
    pub(in crate::mandoc) const fn line_breaks(self) -> usize {
        self.line_breaks
    }
}

impl PreservedInlineState {
    pub(in crate::mandoc) fn formatter_cell_occupied(&self) -> bool {
        self.formatter_line.buffer_is_occupied()
    }
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
    pub(super) current: Font,
    pub(super) previous: Font,
    heading_bold_italic: bool,
}

impl FontState {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            current: Font::Regular,
            previous: Font::Regular,
            heading_bold_italic: false,
        }
    }

    pub(super) fn select(&mut self, font: Font) {
        self.previous = self.current;
        self.current = if self.heading_bold_italic && font == Font::Emphasis {
            Font::StrongEmphasis
        } else {
            font
        };
    }

    pub(super) fn restore(&mut self) {
        std::mem::swap(&mut self.current, &mut self.previous);
    }

    /// mdoc font scopes push a selection. Popping restores the saved current
    /// font, not the previous-selection register used by `\\fP` and `.ft P`.
    pub(in crate::mandoc) fn push_scope(&mut self, font: Font) -> Font {
        let saved = self.current;
        self.select(font);
        saved
    }

    pub(in crate::mandoc) fn pop_scope(&mut self, saved: Font) {
        self.current = saved;
    }

    /// Enter CVS `termp_sh_pre()`/`termp_ss_pre()` font execution.
    ///
    /// Heading presentation is structural in the IR, but the terminal
    /// formatter still pushes bold and enables `fontibi`: an inner `\fI`
    /// selects bold-emphasis and updates the independent previous-font
    /// register.  The returned pair restores only the current stack and the
    /// mode flag; `previous` deliberately survives the scope.
    pub(in crate::mandoc) fn push_heading_scope(&mut self) -> (Font, bool) {
        let saved_mode = self.heading_bold_italic;
        let saved_font = self.push_scope(Font::Strong);
        self.heading_bold_italic = true;
        (saved_font, saved_mode)
    }

    pub(in crate::mandoc) fn pop_heading_scope(&mut self, saved: (Font, bool)) {
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
enum PendingBoundary {
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
    FixedBlank(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KeepState {
    phase: KeepPhase,
}

impl KeepState {
    const fn new() -> Self {
        Self {
            phase: KeepPhase::Inactive,
        }
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
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_owners: Vec::new(),
            projection_owner: 1,
            next_projection_owner: 2,
            deferred_indent_owner: None,
            geometry_stack: Vec::new(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::Enabled,
            last_visible_character: None,
            has_printable_content: false,
            formatter_line: FormatterMachine::new(),
            empty_word: false,
            trailing_output: TrailingOutput::None,
            pending_breakable_spaces: 0,
            pending_line_indent: 0,
            pending_definition_indent: None,
            word_end_break: WordEndBreak::Clear,
            vertical_space_debt: 0,
            keep: KeepState::new(),
            font: FontState::new(),
            zero_advance: ZeroAdvanceState::new(),
            zero_advance_joined: false,
            source_cursor: None,
            final_word_join: None,
            final_source_continuation: None,
            execution_epoch: 0,
            author_execution: None,
            definition_outcome: DefinitionOutcome(0),
            no_break_field: None,
            last_executed_source_line: None,
        }
    }

    pub(in crate::mandoc) const fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            nodes: Vec::new(),
            node_owners: Vec::new(),
            projection_owner: 1,
            next_projection_owner: 2,
            deferred_indent_owner: None,
            geometry_stack: Vec::new(),
            boundary: PendingBoundary::Ordinary,
            spacing: SpacingMode::from_enabled(spacing_enabled),
            last_visible_character: None,
            has_printable_content: false,
            formatter_line: FormatterMachine::new(),
            empty_word: false,
            trailing_output: TrailingOutput::None,
            pending_breakable_spaces: 0,
            pending_line_indent: 0,
            pending_definition_indent: None,
            word_end_break: WordEndBreak::Clear,
            vertical_space_debt: 0,
            keep: KeepState::new(),
            font: FontState::new(),
            zero_advance: ZeroAdvanceState::new(),
            zero_advance_joined: false,
            source_cursor: None,
            final_word_join: None,
            final_source_continuation: None,
            execution_epoch: 0,
            author_execution: None,
            definition_outcome: DefinitionOutcome(0),
            no_break_field: None,
            last_executed_source_line: None,
        }
    }
}
