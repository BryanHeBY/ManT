//! Stateful roff font decoding shared by prose, macro operands and table cells.
use super::super::reference::trailing_sphinx_manual_reference;
use super::{
    Font, FontState, Inline, InlineBuilder, Node, RoffInlineEvent, append_inline_nodes, decode,
    is_formatter_word_blank,
};
use crate::mandoc::roff_escape::ZeroAdvanceMachine;

/// Bounded semantic projection of CVS mandoc's `TERMP_BACKAFTER`/
/// `TERMP_BACKBEFORE` state for `\\z`.
///
/// The terminal formatter carries that state across `term_word()` calls.  A
/// pending glyph therefore belongs to the surrounding inline stream rather
/// than to the one text node that happened to contain the escape.
pub(in crate::mandoc) struct ZeroAdvanceState {
    machine: ZeroAdvanceMachine<Inline>,
    fragment_started_pending: bool,
    resolved_preexisting: bool,
}

impl ZeroAdvanceState {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            machine: ZeroAdvanceMachine::new(),
            fragment_started_pending: false,
            resolved_preexisting: false,
        }
    }

    fn begin_fragment(&mut self) {
        self.fragment_started_pending = self.machine.has_pending();
        self.resolved_preexisting = false;
    }

    fn arm(&mut self) {
        // CVS permits TERMP_BACKAFTER and TERMP_BACKBEFORE at the same time.
        // A second `\\z` arms the next glyph without prematurely discarding
        // the completed zero-advance glyph at the current output position.
        self.machine.arm();
    }

    /// Resolve a pending zero-advance glyph at a formatter-inserted word
    /// boundary. CVS `term_word()` writes that virtual blank before the next
    /// glyph; the blank consumes the backtracking position, so the glyph
    /// survives and the next word joins it without a visible space.
    pub(in crate::mandoc) fn resolve_at_word_boundary(&mut self) -> Option<Inline> {
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
        self.machine.resolve_word_boundary()
    }

    /// A pending zero-advance glyph is visible formatter state even before a
    /// previous ordinary word has committed an IR node.  In particular,
    /// `\\zX` at the beginning of a source line must survive the implicit
    /// boundary before the next word rather than be mistaken for an empty
    /// stream.
    pub(in crate::mandoc) const fn has_pending_glyph(&self) -> bool {
        self.machine.has_pending() && !self.machine.is_armed()
    }

    /// Discard a completed glyph emitted by an operand whose compact output
    /// is suppressed. A bare `\\z` remains armed: CVS carries that request
    /// into the next formatter word, whereas `\\zX` has already produced the
    /// hidden glyph `X` and must not lend it to a later visible operand.
    pub(in crate::mandoc) fn discard_hidden_pending_glyph(&mut self) {
        self.machine.discard_pending();
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
    }

    /// Execute CVS `ESCAPE_NOSPACE` against the pending `\\z` state.
    ///
    /// `term_word()` clears only `TERMP_BACKAFTER` before it considers a
    /// trailing `\\c` a request to join the following input line. A completed
    /// zero-advance glyph is `TERMP_BACKBEFORE` state and remains pending.
    fn cancel_armed_for_no_space(&mut self) -> bool {
        self.machine.cancel_armed()
    }

    /// The output-free recovery path can only carry a bare armed `\\z`.
    /// Keep its state transition encapsulated instead of letting consumers
    /// treat the representation of pending glyphs as public behavior.
    /// Feed formatter-generated text through the same projection as authored
    /// glyphs. Brackets from `.OP`, generated declaration punctuation, and
    /// implicit wrapper text can overwrite a pending `\z` glyph just like a
    /// source glyph in CVS `term_word()`.
    pub(in crate::mandoc) fn append_generated_text(
        &mut self,
        value: &str,
        output: &mut Vec<Inline>,
        font: Font,
    ) {
        let mut buffer = String::new();
        for character in value.chars() {
            if matches!(character, '\n' | '\r') {
                flush_segment(output, &mut buffer, font, None);
                if let Some(glyph) = self.machine.take_pending() {
                    output.push(glyph);
                }
                buffer.push(character);
                continue;
            }
            if self.machine.is_armed() {
                let _ = self
                    .machine
                    .project_glyph(styled_segment(character.to_string(), font));
                continue;
            }
            if self.machine.has_pending() {
                if is_formatter_word_blank(character) {
                    flush_segment(output, &mut buffer, font, None);
                    if let Some(glyph) = self.machine.take_pending() {
                        output.push(glyph);
                    }
                    continue;
                }
                let Some((_, replaced)) = self
                    .machine
                    .project_glyph(styled_segment(character.to_string(), font))
                else {
                    continue;
                };
                if replaced && self.fragment_started_pending {
                    self.resolved_preexisting = true;
                }
            }
            buffer.push(character);
        }
        flush_segment(output, &mut buffer, font, None);
    }

    fn append_text(
        &mut self,
        value: &str,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        for character in value.chars() {
            if matches!(character, '\n' | '\r') {
                self.flush(output, buffer, font, link);
                buffer.push(character);
                continue;
            }
            if self.machine.is_armed() {
                let _ = self
                    .machine
                    .project_glyph(styled_link(character.to_string(), font, link));
                continue;
            }
            if self.machine.has_pending() {
                if is_formatter_word_blank(character) {
                    // The first intervening formatter blank consumes the
                    // backtracking position but does not become document
                    // content.  This is why `TOKEN\\zX END` renders as
                    // `TOKENXEND` in both pinned reference formatters.
                    self.flush(output, buffer, font, link);
                    continue;
                }
                let Some((_, replaced)) =
                    self.machine
                        .project_glyph(styled_link(character.to_string(), font, link))
                else {
                    continue;
                };
                if replaced && self.fragment_started_pending {
                    self.resolved_preexisting = true;
                }
            }
            buffer.push(character);
        }
    }

    fn append_glyph(&mut self, value: &str, buffer: &mut String, font: Font, link: Option<&str>) {
        let Some((_, replaced)) =
            self.machine
                .project_glyph(styled_link(value.to_owned(), font, link))
        else {
            return;
        };
        if replaced && self.fragment_started_pending {
            self.resolved_preexisting = true;
        }
        buffer.push_str(value);
        // A fallback spelling is one roff glyph even though it takes several
        // Unicode scalar values to present. Do not let a following source
        // character overstrike its interior.
    }

    fn append_fallback_glyph(
        &mut self,
        value: &str,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) -> bool {
        let Some((_, replaced)) =
            self.machine
                .project_fallback(styled_link(value.to_owned(), font, link))
        else {
            return false;
        };
        if replaced && self.fragment_started_pending {
            self.resolved_preexisting = true;
        }
        if !value.is_empty() {
            buffer.push_str(value);
        }
        true
    }

    fn flush(
        &mut self,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        flush_segment(output, buffer, font, link);
        if let Some(glyph) = self.machine.take_pending() {
            if self.fragment_started_pending {
                self.resolved_preexisting = true;
            }
            output.push(glyph);
        }
    }

    pub(in crate::mandoc) fn finish_into(&mut self, output: &mut Vec<Inline>) {
        if let Some(glyph) = self.machine.take_pending() {
            output.push(glyph);
        }
        self.machine.clear();
    }

    fn take_preceding_join(&mut self) -> bool {
        let joined = self.fragment_started_pending && self.resolved_preexisting;
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
        joined
    }
}

impl Default for ZeroAdvanceState {
    fn default() -> Self {
        Self::new()
    }
}

pub(in crate::mandoc) fn parse_roff_text(source: &str) -> Vec<Inline> {
    parse_roff_text_with_font(source, Font::Regular, true)
}

/// Decode one roff text run using the font selected by its enclosing macro.
/// Explicit `\\f` escapes change `font` while the run is scanned, so a reset
/// to regular text remains visible even inside an alternating `.BI` argument.
pub(super) fn parse_roff_text_with_font(
    source: &str,
    initial_font: Font,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    let mut state = FontState::new();
    state.select(initial_font);
    parse_roff_text_with_state(source, &mut state, recognize_generated_references)
}

pub(super) fn lower_man_font_scope(
    output: &mut InlineBuilder,
    node: &Node,
    default_name: Option<&str>,
) {
    output.font.select(Font::Regular);
    if let Some((first, second)) = super::alternating_font_pair(node.macro_name.as_deref()) {
        for (index, child) in node.children.iter().enumerate() {
            output
                .font
                .select(if index % 2 == 0 { first } else { second });
            // Alternating man macro operands are emitted as one formatter
            // word.  This is also the scope in which CVS preserves the
            // TERMP_BACK* state from one argument to the next.
            if index > 0 {
                output.tighten_next_boundary();
            }
            super::append_inline_node_with_next(
                output,
                child,
                node.children.get(index + 1),
                default_name,
            );
        }
        output.font.select(Font::Regular);
        return;
    }
    if node.macro_name.as_deref() == Some("OP") {
        // CVS man_term.c::pre_OP() emits both brackets with term_word().
        // Keep them in the caller's formatter stream so pending `\z`/`\p`
        // state crosses authored operands and generated punctuation in order.
        output.append_text("[");
        output.tighten_next_boundary();
        output.enter_keep_words();
        for (index, child) in node.children.iter().enumerate() {
            output.font.select(if index == 0 {
                Font::Strong
            } else {
                Font::Emphasis
            });
            super::append_inline_node_with_next(
                output,
                child,
                node.children.get(index + 1),
                default_name,
            );
        }
        // OP resets for its closing bracket, then the man macro scope resets
        // again. Consequently a following fP selects regular, not its operand.
        output.font.select(Font::Regular);
        output.exit_keep_words();
        output.tighten_next_boundary();
        output.append_text("]");
        output.font.select(Font::Regular);
        return;
    }
    match node.macro_name.as_deref() {
        Some("B" | "SB") => output.font.select(Font::Strong),
        Some("I") => output.font.select(Font::Emphasis),
        _ => {}
    }
    super::append_inline_nodes(output, &node.children, default_name);
    output.font.select(Font::Regular);
}

fn builder_with_zero_advance(
    spacing: bool,
    state: FontState,
    zero_advance: &mut ZeroAdvanceState,
) -> InlineBuilder {
    let mut builder = InlineBuilder::with_spacing(spacing);
    builder.font = state;
    builder.zero_advance = std::mem::take(zero_advance);
    builder
}

fn finish_with_zero_advance(
    builder: InlineBuilder,
    _zero_advance: &mut ZeroAdvanceState,
) -> (Vec<Inline>, super::flow::PreservedInlineState) {
    builder.finish_preserving_execution()
}

pub(in crate::mandoc) fn lower_inline_nodes_with_font_state(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
) -> Vec<Inline> {
    let mut zero_advance = ZeroAdvanceState::new();
    let (mut output, execution) = lower_inline_nodes_with_font_state_and_zero_advance(
        nodes,
        default_name,
        spacing,
        state,
        &mut zero_advance,
    );
    if execution.word_end_break {
        output.push(Inline::LineBreak);
    }
    zero_advance = execution.zero_advance;
    zero_advance.finish_into(&mut output);
    output
}

/// Lower one executed no-fill input row.
///
/// A deferred `\p` at the end of the formatter word is settled by the
/// physical input-row boundary, not emitted as a second inline break.  Any
/// completed `\z` glyph still belongs to the row and must be committed before
/// the outer literal flow appends that boundary.
pub(in crate::mandoc) struct NoFillInlineState {
    zero_advance: ZeroAdvanceState,
    pending_word_end_break: bool,
    continued: bool,
}

impl NoFillInlineState {
    pub(in crate::mandoc) fn new() -> Self {
        Self {
            zero_advance: ZeroAdvanceState::new(),
            pending_word_end_break: false,
            continued: false,
        }
    }

    pub(in crate::mandoc) fn finish_row(&mut self, output: &mut Vec<Inline>) {
        self.zero_advance.finish_into(output);
        self.pending_word_end_break = false;
        self.continued = false;
    }

    pub(in crate::mandoc) fn take_settled_row(&mut self) -> Vec<Inline> {
        let mut output = Vec::new();
        self.finish_row(&mut output);
        output
    }
}

pub(in crate::mandoc) fn lower_no_fill_line_with_font_state(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
    inline_state: &mut NoFillInlineState,
    source_continuation_fallback: bool,
) -> (Vec<Inline>, bool) {
    let mut builder = builder_with_zero_advance(spacing, *state, &mut inline_state.zero_advance);
    if inline_state.continued {
        builder.continue_source_line(true);
        builder.tighten_next_boundary();
    }
    if inline_state.pending_word_end_break {
        builder.request_word_end_break();
    }
    append_inline_nodes(&mut builder, nodes, default_name);
    *state = builder.font;
    let (mut output, execution) = builder.finish_preserving_execution();
    let continues_line = execution
        .source_continuation
        .unwrap_or(source_continuation_fallback);
    inline_state.zero_advance = execution.zero_advance;
    inline_state.pending_word_end_break = execution.word_end_break;
    inline_state.continued = continues_line;
    if !continues_line {
        inline_state.finish_row(&mut output);
    }
    (output, continues_line)
}

fn lower_inline_nodes_with_font_state_and_zero_advance(
    nodes: &[Node],
    default_name: Option<&str>,
    spacing: bool,
    state: &mut FontState,
    zero_advance: &mut ZeroAdvanceState,
) -> (Vec<Inline>, super::flow::PreservedInlineState) {
    let mut builder = builder_with_zero_advance(spacing, *state, zero_advance);
    append_inline_nodes(&mut builder, nodes, default_name);
    *state = builder.font;
    finish_with_zero_advance(builder, zero_advance)
}

pub(in crate::mandoc) fn parse_roff_text_with_state(
    source: &str,
    state: &mut FontState,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    let mut zero_advance = ZeroAdvanceState::default();
    let execution = parse_roff_text_with_zero_advance(
        source,
        state,
        recognize_generated_references,
        &mut zero_advance,
        false,
    );
    let mut output = execution.output;
    if execution.pending_word_end_break {
        output.push(Inline::LineBreak);
    }
    zero_advance.finish_into(&mut output);
    output
}

pub(in crate::mandoc) struct TextExecution {
    pub(in crate::mandoc) output: Vec<Inline>,
    pub(in crate::mandoc) joins_preceding_node: bool,
    pub(in crate::mandoc) source_continuation: Option<bool>,
    pub(in crate::mandoc) pending_word_end_break: bool,
}

struct TextEventState {
    pending_word_end_break: bool,
    suppress_break_whitespace: bool,
}

fn append_text_event(
    value: &str,
    output: &mut Vec<Inline>,
    buffer: &mut String,
    font: Font,
    link: Option<&str>,
    zero_advance: &mut ZeroAdvanceState,
    state: &mut TextEventState,
) {
    let mut chunk = String::new();
    for character in value.chars() {
        if state.pending_word_end_break && is_formatter_word_blank(character) {
            zero_advance.append_text(&chunk, output, buffer, font, link);
            chunk.clear();
            if zero_advance.has_pending_glyph() {
                // CVS stores `\\p` in the same terminal buffer as a
                // completed `\\z` glyph.  The intervening word blank settles
                // that glyph first; the word-end break remains pending until
                // the next ordinary formatter boundary.
                zero_advance.flush(output, buffer, font, link);
                state.suppress_break_whitespace = true;
                continue;
            }
            zero_advance.flush(output, buffer, font, link);
            output.push(Inline::LineBreak);
            state.pending_word_end_break = false;
            state.suppress_break_whitespace = true;
            continue;
        }
        if state.suppress_break_whitespace && is_formatter_word_blank(character) {
            continue;
        }
        state.suppress_break_whitespace = false;
        chunk.push(character);
    }
    // The decoder has already classified controls. A backslash produced by
    // \e or \[rs] is literal author content.
    zero_advance.append_text(&chunk, output, buffer, font, link);
}

/// Decode a text node while retaining formatter state in its caller's inline
/// stream.  The result reports cross-node zero-advance joining, the final
/// physical-line decision, and a deferred word-end break independently.
pub(in crate::mandoc) fn parse_roff_text_with_zero_advance(
    source: &str,
    state: &mut FontState,
    recognize_generated_references: bool,
    zero_advance: &mut ZeroAdvanceState,
    pending_word_end_break: bool,
) -> TextExecution {
    let (mut output, mut buffer) = (Vec::new(), String::new());
    let mut font = state.current;
    let mut link: Option<String> = None;
    let events = decode(source);
    let mut explicit_line_continuation = None;
    let mut text_state = TextEventState {
        pending_word_end_break,
        suppress_break_whitespace: false,
    };
    zero_advance.begin_fragment();

    for (index, event) in events.iter().enumerate() {
        match event {
            RoffInlineEvent::Text(value) => {
                append_text_event(
                    value,
                    &mut output,
                    &mut buffer,
                    font,
                    link.as_deref(),
                    zero_advance,
                    &mut text_state,
                );
            }
            RoffInlineEvent::Glyph(value)
            | RoffInlineEvent::Overstrike {
                terminal: Some(value),
                ..
            } => {
                text_state.suppress_break_whitespace = false;
                zero_advance.append_glyph(value, &mut buffer, font, link.as_deref());
            }
            RoffInlineEvent::FallbackGlyph(value) => {
                if zero_advance.append_fallback_glyph(value, &mut buffer, font, link.as_deref()) {
                    text_state.suppress_break_whitespace = false;
                }
            }
            RoffInlineEvent::DeviceName => {
                text_state.suppress_break_whitespace = false;
                zero_advance.append_text("utf8", &mut output, &mut buffer, font, link.as_deref());
            }
            RoffInlineEvent::ZeroAdvance => zero_advance.arm(),
            RoffInlineEvent::NoSpace => {
                let canceled_armed = zero_advance.cancel_armed_for_no_space();
                if index + 1 == events.len() {
                    explicit_line_continuation = Some(!canceled_armed);
                    if !canceled_armed {
                        // A trailing `\c` keeps the current formatter word
                        // open on the next input row. Consequently a pending
                        // word-end `\p` has no boundary to realize here.
                        text_state.pending_word_end_break = false;
                    }
                }
            }
            RoffInlineEvent::Font(next_font) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                state.select(*next_font);
                font = state.current;
            }
            RoffInlineEvent::PreviousFont => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                state.restore();
                font = state.current;
            }
            RoffInlineEvent::Link(target) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                link.clone_from(target);
            }
            RoffInlineEvent::EmptyDestination => {
                text_state.suppress_break_whitespace = false;
                if !recognize_generated_references
                    || !promote_sphinx_manual_reference(
                        &mut output,
                        &mut buffer,
                        font,
                        link.as_deref(),
                    )
                {
                    buffer.push_str("<>");
                }
            }
            RoffInlineEvent::LineBreak => {
                text_state.pending_word_end_break = true;
            }
            RoffInlineEvent::Presentation { .. }
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Overstrike { terminal: None, .. } => {}
        }
    }
    flush_segment(&mut output, &mut buffer, font, link.as_deref());
    TextExecution {
        output,
        joins_preceding_node: zero_advance.take_preceding_join(),
        source_continuation: explicit_line_continuation,
        pending_word_end_break: text_state.pending_word_end_break,
    }
}

fn promote_sphinx_manual_reference(
    output: &mut Vec<Inline>,
    buffer: &mut String,
    font: Font,
    external_link: Option<&str>,
) -> bool {
    if external_link.is_some() || matches!(font, Font::Code | Font::CodeStrong | Font::CodeEmphasis)
    {
        return false;
    }
    let Some(reference) = trailing_sphinx_manual_reference(buffer) else {
        return false;
    };
    let prefix = reference.prefix.to_owned();
    let display = reference.display.to_owned();
    let name = reference.name.to_owned();
    let manual_section = reference.manual_section.to_owned();
    *buffer = prefix;
    flush_segment(output, buffer, font, None);
    output.push(Inline::Link {
        target: mant_ir::LinkTarget::Manual {
            name,
            manual_section: Some(manual_section),
        },
        title: None,
        children: vec![styled_segment(display, font)],
    });
    true
}

fn flush_segment(output: &mut Vec<Inline>, buffer: &mut String, font: Font, link: Option<&str>) {
    if buffer.is_empty() {
        return;
    }
    let value = std::mem::take(buffer);
    output.push(styled_link(value, font, link));
}

fn styled_link(value: String, font: Font, link: Option<&str>) -> Inline {
    let styled = styled_segment(value, font);
    if let Some(target) = link {
        Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: target.to_owned(),
            },
            title: None,
            children: vec![styled],
        }
    } else {
        styled
    }
}

pub(super) fn styled_segment(value: String, font: Font) -> Inline {
    match font {
        Font::Regular => Inline::Text { value },
        Font::Strong => Inline::Strong {
            children: vec![Inline::Text { value }],
        },
        Font::Emphasis => Inline::Emphasis {
            children: vec![Inline::Text { value }],
        },
        Font::StrongEmphasis => Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![Inline::Text { value }],
            }],
        },
        Font::Code => Inline::Code { value },
        Font::CodeStrong => Inline::Strong {
            children: vec![Inline::Code { value }],
        },
        Font::CodeEmphasis => Inline::Emphasis {
            children: vec![Inline::Code { value }],
        },
    }
}

/// Keep a generated prefix and equally styled operands in a single visible
/// run, without wrapping font overrides in an additional, additive style.
pub(super) fn coalesce_font_runs(nodes: Vec<Inline>) -> Vec<Inline> {
    let mut output: Vec<Inline> = Vec::new();
    for node in nodes {
        match (output.last_mut(), node) {
            (Some(Inline::Strong { children: previous }), Inline::Strong { mut children })
            | (Some(Inline::Emphasis { children: previous }), Inline::Emphasis { mut children }) => {
                previous.append(&mut children);
            }
            (Some(Inline::Text { value: previous }), Inline::Text { value }) => {
                previous.push_str(&value);
            }
            (_, node) => output.push(node),
        }
    }
    output
}
