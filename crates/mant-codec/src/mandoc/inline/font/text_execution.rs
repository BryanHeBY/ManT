use super::super::super::reference::trailing_sphinx_manual_reference;
use super::style::{flush_segment, styled_segment};
use super::{
    Font, FontState, Inline, RoffInlineEvent, TrailingOutput, ZeroAdvanceState,
    is_formatter_word_blank,
};

/// Independent text-word policies, chosen by the caller's native sink.
/// Recording does not select another executor, and a field receipt may own
/// acceptance while generated-reference recognition remains independently on.
#[derive(Clone, Copy)]
pub(in crate::mandoc) struct TextExecutionPolicy {
    pub(in crate::mandoc) recognize_generated_references: bool,
    pub(in crate::mandoc) record_native_cells: bool,
    pub(in crate::mandoc) field_authoritative: bool,
}

/// Borrow the caller's registers for one word; output ownership never creates
/// a second font or zero-advance execution state.
pub(in crate::mandoc) struct TextExecutionContext<'state> {
    pub(in crate::mandoc) font: &'state mut FontState,
    pub(in crate::mandoc) zero_advance: &'state mut ZeroAdvanceState,
    pub(in crate::mandoc) pending_word_end_break: bool,
    pub(in crate::mandoc) policy: TextExecutionPolicy,
}

/// A presentation write borrows one word's output and the caller's pending
/// glyph state. Code can change this display font without changing registers.
struct WordOutput<'word> {
    output: &'word mut Vec<Inline>,
    buffer: &'word mut String,
    font: Font,
    link: Option<&'word str>,
    zero_advance: &'word mut ZeroAdvanceState,
}

pub(in crate::mandoc) fn parse_roff_text_with_state(
    source: &str,
    state: &mut FontState,
    recognize_generated_references: bool,
) -> Vec<Inline> {
    parse_roff_text_with_scan_status(source, state, recognize_generated_references).0
}

pub(in crate::mandoc) fn parse_roff_text_with_scan_status(
    source: &str,
    state: &mut FontState,
    recognize_generated_references: bool,
) -> (
    Vec<Inline>,
    crate::mandoc::escape_coverage::EscapeScanStatus,
) {
    let mut zero_advance = ZeroAdvanceState::default();
    let execution = parse_roff_text_with_zero_advance(
        source,
        TextExecutionContext {
            font: state,
            zero_advance: &mut zero_advance,
            pending_word_end_break: false,
            policy: TextExecutionPolicy {
                recognize_generated_references,
                record_native_cells: false,
                field_authoritative: false,
            },
        },
    );
    let mut output = execution.output;
    if execution.pending_word_end_break {
        output.push(Inline::line_break());
    }
    zero_advance.finish_into(&mut output);
    (output, execution.escape_scan)
}

// Native joining, continuation, breaks and acceptance are independent results.
#[allow(clippy::struct_excessive_bools)]
pub(in crate::mandoc) struct TextExecution {
    pub(in crate::mandoc) escape_scan: crate::mandoc::escape_coverage::EscapeScanStatus,
    pub(in crate::mandoc) output: Vec<Inline>,
    /// Native writes in decode order; semantic wrappers never supply cells.
    pub(in crate::mandoc) native_writes: Vec<super::super::flow::field_buffer::FieldWrite>,
    pub(in crate::mandoc) joins_preceding_node: bool,
    pub(in crate::mandoc) source_continuation: Option<bool>,
    pub(in crate::mandoc) pending_word_end_break: bool,
    pub(in crate::mandoc) trailing_output: TrailingOutput,
    /// The trailing `\p` marker was separated from the last graph by a
    /// breakable blank: its restarted pass rejects at the next word's
    /// automatic separator (term.c:143-146).
    pub(in crate::mandoc) pending_word_end_break_separated: bool,
    /// A `\p` marker met a surviving breakable blank before this flush unit
    /// recorded any graph: the remainder of the unit is unprinted input
    /// (term.c:143-146 with 233-237).
    pub(in crate::mandoc) definitive_reject: bool,
    /// This word armed `graph` only through zero-width cells (NBRZW).
    pub(in crate::mandoc) word_zero_graph: bool,
}

/// One presentation segment inside a single native formatter word.
/// Source segments execute roff controls; Code segments contribute already
/// normalized glyphs without mutating the source font registers.
pub(in crate::mandoc) enum FormatterWordPart<'a> {
    Source(&'a str),
    Code(String),
}

enum FormatterWordEvent {
    Source(RoffInlineEvent),
    Code(String),
}

// Native graph, break, blank and rejection flags can coexist during one word.
#[allow(clippy::struct_excessive_bools)]
struct TextEventState {
    pending_word_end_break: bool,
    suppress_break_whitespace: bool,
    graph_seen: bool,
    last_breakable_blank: bool,
    trailing_breakable_blanks: usize,
    break_started_after_blank: bool,
    break_trailing_blanks: usize,
    graph_since_break: bool,
    graph_count: usize,
    /// A graph cell occurred since the last breakable blank (term.c `graph`
    /// flag): at a marker's break blank it selects the tail-acceptance arm
    /// (term.c:362-366) over the nbr==0 rejection.
    graph_since_blank: bool,
    /// Any source cell (graph or blank) passed through this fragment; a
    /// marker-only fragment carries no adjacency fact of its own.
    saw_source_cell: bool,
    /// A `\p` marker met a surviving breakable blank: `term_fill()` stopped
    /// the pass with `nbr == 0` (term.c:143-146) and `term_flushln()` wiped
    /// the unprinted remainder of the flush unit (term.c:233-237).
    wiped: bool,
    /// A zero-width graph class cell (NBRZW) armed `graph` this word.
    zero_graph_seen: bool,
}

impl TextEventState {
    fn note_graph(&mut self) {
        self.graph_seen = true;
        self.graph_since_blank = true;
        self.graph_since_break |= self.pending_word_end_break;
        self.last_breakable_blank = false;
        self.trailing_breakable_blanks = 0;
    }

    const fn new(pending_word_end_break: bool) -> Self {
        Self {
            pending_word_end_break,
            suppress_break_whitespace: false,
            graph_seen: false,
            last_breakable_blank: false,
            saw_source_cell: false,
            trailing_breakable_blanks: 0,
            graph_since_blank: false,
            break_started_after_blank: false,
            break_trailing_blanks: 0,
            graph_since_break: false,
            graph_count: 0,
            wiped: false,
            zero_graph_seen: false,
        }
    }
}

fn append_text_event(
    value: &str,
    destination: WordOutput<'_>,
    state: &mut TextEventState,
    field_authoritative: bool,
) {
    let WordOutput {
        output,
        buffer,
        font,
        link,
        zero_advance,
    } = destination;
    let mut chunk = String::new();
    for character in value.chars() {
        if state.wiped {
            // The flush unit's remainder is unprinted input (term.c:233-237
            // reached through 143-146); graphs and blanks alike die with it.
            continue;
        }
        if state.pending_word_end_break && is_formatter_word_blank(character) {
            zero_advance.append_text(&chunk, output, buffer, font, link);
            chunk.clear();
            // This is the marker's first following blank in either
            // projection arm. BACKBEFORE can consume that blank, but it
            // cannot make a later blank in the same word another marker
            // event (term.c:294-305,901-908). Native passes retain breakline
            // independently until they accept their ordered cell interval.
            state.pending_word_end_break = false;
            if zero_advance.has_buffered_glyph() {
                // CVS stores `\\p` in the same terminal buffer as a
                // completed `\\z` glyph.  The intervening word blank settles
                // that glyph first; the next graph's retreat consumes the
                // marker's own blank (term.c:901-908), so the blank held
                // before the marker — like the word separator of the `ph`
                // shape — survives and prints after the settled glyph
                // (term.c:573-576).  `breakline` stays armed: term_fill()
                // cannot stop at the eaten blank and defers the break to
                // the next surviving blank (term.c:294-295).
                zero_advance.flush(output, buffer, font, link);
                let held = zero_advance.take_held();
                buffer.push_str(&held);
                if zero_advance.take_marker_blank_separator() {
                    buffer.push(' ');
                }
                state.suppress_break_whitespace = true;
                continue;
            }
            zero_advance.flush(output, buffer, font, link);
            let rejected_after_accepted_blank =
                state.break_started_after_blank && !state.graph_since_break;
            if rejected_after_accepted_blank {
                // term_fill() records nbr before the breakable blank. That
                // blank is outside the accepted field as well as its suffix.
                crate::mandoc::inline::flow::trim_trailing_breakable_spaces(
                    output,
                    state.break_trailing_blanks,
                );
            }
            // The marker arms `breakline` and this blank stops the pass
            // (term.c:294-295). A definition-field session computes its own
            // pass arithmetic (`FieldBuffer::flush_receipt`); there the
            // committed row keeps its break exactly as before. In ordinary
            // flow, with the `graph` flag still armed — a graph occurred
            // since the last breakable blank — the stopped pass
            // tail-accepts through the marker (term.c:362-366): the row
            // commits, the blank is consumed with the break (205-207), and
            // the NEXT pass continues normally after it. Otherwise the next
            // pass resumes at the marker itself, immediately rejects with
            // `nbr == 0`, and term_flushln() wipes the whole unprinted
            // remainder of the flush unit (term.c:143-146 with 233-237); an
            // earlier graph keeps its committed row (term.c:220).
            if !field_authoritative {
                let tail_accepted = state.graph_since_blank;
                if tail_accepted || state.graph_seen {
                    output.push(Inline::line_break());
                }
                if !tail_accepted {
                    state.wiped = true;
                }
            }
            state.suppress_break_whitespace = true;
            state.graph_seen = false;
            state.graph_since_blank = false;
            state.break_started_after_blank = false;
            state.graph_since_break = false;
            state.last_breakable_blank = false;
            state.trailing_breakable_blanks = 0;
            state.break_trailing_blanks = 0;
            continue;
        }
        if state.suppress_break_whitespace && is_formatter_word_blank(character) {
            continue;
        }
        state.suppress_break_whitespace = false;
        let graph = !is_formatter_word_blank(character) && character != '\n';
        if graph {
            state.graph_count += 1;
        }
        state.graph_seen |= graph;
        state.graph_since_break |= state.pending_word_end_break && graph;
        state.graph_since_blank = graph;
        state.saw_source_cell = true;
        state.last_breakable_blank = is_formatter_word_blank(character);
        state.trailing_breakable_blanks = if state.last_breakable_blank {
            state.trailing_breakable_blanks.saturating_add(1)
        } else {
            0
        };
        chunk.push(character);
    }
    // The decoder has already classified controls. A backslash produced by
    // \e or \[rs] is literal author content.
    zero_advance.append_text(&chunk, output, buffer, font, link);
}

/// Decode a text node while retaining formatter state in its caller's inline
/// stream. Joining, source continuation and word-end breaks remain independent.
pub(in crate::mandoc) fn parse_roff_text_with_zero_advance(
    source: &str,
    context: TextExecutionContext<'_>,
) -> TextExecution {
    let decoded = crate::mandoc::roff_escape::decode_with_status(source);
    let events = decoded
        .events
        .into_iter()
        .map(FormatterWordEvent::Source)
        .collect::<Vec<_>>();
    let mut execution = execute_formatter_word_events(&events, context);
    execution.escape_scan = decoded.budget_exhausted.into();
    execution
}

pub(in crate::mandoc) fn parse_formatter_word_parts_with_zero_advance(
    parts: &[FormatterWordPart<'_>],
    context: TextExecutionContext<'_>,
) -> TextExecution {
    let mut budget_exhausted = false;
    let events = parts
        .iter()
        .flat_map(|part| match part {
            FormatterWordPart::Source(source) => {
                let decoded = crate::mandoc::roff_escape::decode_with_status(source);
                budget_exhausted |= decoded.budget_exhausted;
                decoded
                    .events
                    .into_iter()
                    .map(FormatterWordEvent::Source)
                    .collect::<Vec<_>>()
            }
            FormatterWordPart::Code(value) => vec![FormatterWordEvent::Code(value.clone())],
        })
        .collect::<Vec<_>>();
    let mut execution = execute_formatter_word_events(&events, context);
    execution.escape_scan = budget_exhausted.into();
    execution
}

/// One word's presentation and native-write sink, before the caller receives
/// its outcome. The borrowed context continues to own persistent registers.
struct WordExecution {
    output: Vec<Inline>,
    buffer: String,
    font: Font,
    link: Option<String>,
    source_continuation: Option<bool>,
    text_state: TextEventState,
    native_writes: Vec<super::super::flow::field_buffer::FieldWrite>,
}

impl WordExecution {
    fn new(context: &TextExecutionContext<'_>) -> Self {
        Self {
            output: Vec::new(),
            buffer: String::new(),
            font: context.font.display_current(),
            link: None,
            source_continuation: None,
            text_state: TextEventState::new(context.pending_word_end_break),
            native_writes: Vec::new(),
        }
    }

    fn append_event(
        &mut self,
        event: &FormatterWordEvent,
        context: &mut TextExecutionContext<'_>,
        is_last: bool,
    ) {
        match event {
            FormatterWordEvent::Code(value) => append_code_event(
                value,
                WordOutput {
                    output: &mut self.output,
                    buffer: &mut self.buffer,
                    font: self.font,
                    link: self.link.as_deref(),
                    zero_advance: context.zero_advance,
                },
                &mut self.text_state,
            ),
            FormatterWordEvent::Source(event) => self.append_source_event(event, context, is_last),
        }
    }

    fn append_source_event(
        &mut self,
        event: &RoffInlineEvent,
        context: &mut TextExecutionContext<'_>,
        is_last: bool,
    ) {
        match event {
            RoffInlineEvent::Text(value) => append_text_event(
                value,
                WordOutput {
                    output: &mut self.output,
                    buffer: &mut self.buffer,
                    font: self.font,
                    link: self.link.as_deref(),
                    zero_advance: context.zero_advance,
                },
                &mut self.text_state,
                context.policy.field_authoritative,
            ),
            RoffInlineEvent::Glyph(value)
            | RoffInlineEvent::Overstrike {
                terminal: Some(value),
                ..
            } => {
                self.append_glyph(value, context.zero_advance);
            }
            RoffInlineEvent::BreakableHyphen => self.append_glyph("-", context.zero_advance),
            RoffInlineEvent::FallbackGlyph(value) => {
                self.append_fallback_glyph(value, context.zero_advance);
            }
            RoffInlineEvent::DeviceName => self.append_device_name(context.zero_advance),
            RoffInlineEvent::ZeroAdvance => context.zero_advance.arm(),
            RoffInlineEvent::NoSpace => {
                let canceled_armed = context.zero_advance.cancel_armed_for_no_space();
                if is_last {
                    self.source_continuation = Some(!canceled_armed);
                }
            }
            RoffInlineEvent::Font(next_font) => {
                self.flush_segment();
                context.font.select(*next_font);
                self.font = context.font.display_current();
            }
            RoffInlineEvent::PreviousFont => {
                self.flush_segment();
                context.font.restore();
                self.font = context.font.display_current();
            }
            RoffInlineEvent::Link(target) => {
                self.flush_segment();
                self.link.clone_from(target);
            }
            RoffInlineEvent::EmptyDestination => {
                if !self.text_state.wiped {
                    append_empty_destination(
                        &mut self.output,
                        &mut self.buffer,
                        self.font,
                        self.link.as_deref(),
                        context.policy.recognize_generated_references,
                        &mut self.text_state,
                    );
                }
            }
            RoffInlineEvent::LineBreak => {
                self.text_state.pending_word_end_break = true;
                self.text_state.break_started_after_blank =
                    self.text_state.last_breakable_blank && self.text_state.graph_seen;
                self.text_state.break_trailing_blanks = self.text_state.trailing_breakable_blanks;
                self.text_state.graph_since_break = false;
            }
            RoffInlineEvent::ZeroWidthGlyph => {
                self.text_state.note_graph();
                self.text_state.zero_graph_seen = true;
            }
            RoffInlineEvent::Presentation {
                kind: crate::mandoc::roff_escape::PresentationKind::HorizontalMotion,
                ..
            } => {
                // Positive \h under BACKAFTER clears the arm and skips the
                // advance (term.c:677-680), including its projected blank.
                if context.zero_advance.take_armed() && self.buffer.ends_with(' ') {
                    self.buffer.pop();
                }
            }
            RoffInlineEvent::Presentation { .. }
            | RoffInlineEvent::Overstrike { terminal: None, .. } => {}
        }
    }

    fn append_glyph(&mut self, value: &str, zero_advance: &mut ZeroAdvanceState) {
        self.text_state.suppress_break_whitespace = false;
        self.text_state.note_graph();
        self.text_state.graph_count += 1;
        if !self.text_state.wiped {
            zero_advance.append_glyph(
                value,
                &mut self.output,
                &mut self.buffer,
                self.font,
                self.link.as_deref(),
            );
        }
    }

    fn append_fallback_glyph(&mut self, value: &str, zero_advance: &mut ZeroAdvanceState) {
        if !self.text_state.wiped {
            zero_advance.append_fallback_glyph(
                value,
                &mut self.buffer,
                self.font,
                self.link.as_deref(),
            );
        }
        // Native recovery is graph even when \z erases its source spelling.
        self.text_state.suppress_break_whitespace = false;
        self.text_state.note_graph();
        self.text_state.graph_count += 1;
    }

    fn append_device_name(&mut self, zero_advance: &mut ZeroAdvanceState) {
        self.text_state.suppress_break_whitespace = false;
        self.text_state.note_graph();
        if !self.text_state.wiped {
            zero_advance.append_text(
                "utf8",
                &mut self.output,
                &mut self.buffer,
                self.font,
                self.link.as_deref(),
            );
        }
    }

    fn flush_segment(&mut self) {
        flush_segment(
            &mut self.output,
            &mut self.buffer,
            self.font,
            self.link.as_deref(),
        );
    }
}

// Record each decoded native write before its presentation action, in the
// same term_word() order. Final flush precedes taking the joining receipt.
fn execute_formatter_word_events(
    events: &[FormatterWordEvent],
    mut context: TextExecutionContext<'_>,
) -> TextExecution {
    let mut word = WordExecution::new(&context);
    context.zero_advance.begin_fragment();
    for (index, event) in events.iter().enumerate() {
        if context.policy.record_native_cells {
            record_native_event(
                event,
                &mut word.native_writes,
                context.zero_advance.fallback_is_projected(),
                context
                    .policy
                    .field_authoritative
                    .then_some(&word.text_state),
            );
        }
        word.append_event(event, &mut context, index + 1 == events.len());
    }
    word.flush_segment();
    context.zero_advance.clear_marker_blank_separator();
    finish_text_execution(events, context.zero_advance, word)
}

/// `term.c::term_word()` buffers controls and invisible cells before
/// `term_field()` projects printable output. Record these facts independently
/// of the semantic IR and the zero-advance presentation machine.
fn record_native_event(
    event: &FormatterWordEvent,
    writes: &mut Vec<super::super::flow::field_buffer::FieldWrite>,
    fallback_projected: bool,
    authoritative_state: Option<&TextEventState>,
) {
    use super::super::flow::field_buffer::{FieldCell, FieldWrite};
    match event {
        FormatterWordEvent::Source(RoffInlineEvent::Text(value))
            if authoritative_state.is_some() =>
        {
            let state = authoritative_state.expect("authoritative text state");
            let mut pending = state.pending_word_end_break;
            let mut suppress = state.suppress_break_whitespace;
            for character in value.chars() {
                if is_formatter_word_blank(character) && (pending || suppress) {
                    writes.push(FieldWrite::UnprojectedBlank);
                    pending = false;
                    suppress = true;
                } else {
                    suppress = false;
                    FieldWrite::append_literal(writes, character.encode_utf8(&mut [0; 4]));
                }
            }
        }
        FormatterWordEvent::Code(value)
        | FormatterWordEvent::Source(
            RoffInlineEvent::Text(value)
            | RoffInlineEvent::Glyph(value)
            | RoffInlineEvent::Overstrike {
                terminal: Some(value),
                ..
            },
        ) => FieldWrite::append_literal(writes, value),
        FormatterWordEvent::Source(RoffInlineEvent::FallbackGlyph(value)) => {
            writes.push(FieldWrite::RecoveryGlyph {
                projected_scalars: if fallback_projected {
                    value.chars().count()
                } else {
                    0
                },
            });
        }
        FormatterWordEvent::Source(RoffInlineEvent::DeviceName) => {
            FieldWrite::append_literal(writes, "utf8");
        }
        FormatterWordEvent::Source(RoffInlineEvent::BreakableHyphen) => {
            writes.push(FieldWrite::Cell(FieldCell::Hyphen));
        }
        FormatterWordEvent::Source(RoffInlineEvent::ZeroAdvance) => {
            writes.push(FieldWrite::ArmBackafter);
        }
        FormatterWordEvent::Source(RoffInlineEvent::NoSpace) => {
            writes.push(FieldWrite::CancelBackafter);
        }
        FormatterWordEvent::Source(RoffInlineEvent::LineBreak) => {
            writes.push(FieldWrite::Cell(FieldCell::BreakMarker));
        }
        FormatterWordEvent::Source(RoffInlineEvent::ZeroWidthGlyph) => {
            writes.push(FieldWrite::Cell(FieldCell::ZeroWidthGraph));
        }
        FormatterWordEvent::Source(RoffInlineEvent::EmptyDestination) => {
            FieldWrite::append_literal(writes, "<>");
        }
        FormatterWordEvent::Source(_) => {}
    }
}

fn append_empty_destination(
    output: &mut Vec<Inline>,
    buffer: &mut String,
    font: Font,
    link: Option<&str>,
    recognize_generated_references: bool,
    text_state: &mut TextEventState,
) {
    text_state.suppress_break_whitespace = false;
    if !recognize_generated_references
        || !promote_sphinx_manual_reference(output, buffer, font, link)
    {
        buffer.push_str("<>");
    }
}

fn finish_text_execution(
    events: &[FormatterWordEvent],
    zero_advance: &mut ZeroAdvanceState,
    word: WordExecution,
) -> TextExecution {
    let WordExecution {
        output,
        native_writes,
        source_continuation,
        text_state,
        ..
    } = word;
    let trailing_output = match mant_ir::last_visible_character(&output) {
        None | Some('\n') => TrailingOutput::None,
        Some(character) if !character.is_whitespace() => TrailingOutput::NonBlank,
        Some(_) => {
            let count = trailing_breakable_spaces(events)
                .min(crate::mandoc::inline::flow::trailing_ascii_spaces(&output));
            if count == 0 {
                TrailingOutput::FixedBlank
            } else {
                TrailingOutput::BreakableBlank(count)
            }
        }
    };
    TextExecution {
        escape_scan: crate::mandoc::escape_coverage::EscapeScanStatus::Complete,
        output,
        native_writes,
        joins_preceding_node: zero_advance.take_preceding_join(),
        source_continuation,
        pending_word_end_break: text_state.pending_word_end_break,
        pending_word_end_break_separated: text_state.pending_word_end_break
            && text_state.saw_source_cell
            && !text_state.graph_since_blank,
        definitive_reject: text_state.wiped,
        word_zero_graph: text_state.zero_graph_seen,
        trailing_output,
    }
}

/// Count only ordinary source blanks eligible for CVS `term_field()` trim.
/// Glyph events spelling a space originate from `\~`, `\0`, or mandoc's
/// internal `ASCII_NBRSP` and remain formatter graph rather than word padding.
fn trailing_breakable_spaces(events: &[FormatterWordEvent]) -> usize {
    let mut count = 0;
    for event in events.iter().rev() {
        match event {
            FormatterWordEvent::Source(RoffInlineEvent::Text(value)) => {
                if value.is_empty() {
                    continue;
                }
                let trailing = value.chars().rev().take_while(|&ch| ch == ' ').count();
                count += trailing;
                if trailing != value.chars().count() {
                    return count;
                }
            }
            FormatterWordEvent::Source(
                RoffInlineEvent::Font(_)
                | RoffInlineEvent::PreviousFont
                | RoffInlineEvent::ZeroAdvance
                | RoffInlineEvent::NoSpace
                | RoffInlineEvent::Presentation { .. }
                | RoffInlineEvent::ZeroWidthGlyph
                | RoffInlineEvent::LineBreak,
            ) => {}
            FormatterWordEvent::Code(_)
            | FormatterWordEvent::Source(
                RoffInlineEvent::Glyph(_)
                | RoffInlineEvent::BreakableHyphen
                | RoffInlineEvent::FallbackGlyph(_)
                | RoffInlineEvent::DeviceName
                | RoffInlineEvent::Overstrike { .. }
                | RoffInlineEvent::Link(_)
                | RoffInlineEvent::EmptyDestination,
            ) => return count,
        }
    }
    count
}

fn append_code_event(
    value: &str,
    mut destination: WordOutput<'_>,
    text_state: &mut TextEventState,
) {
    flush_segment(
        destination.output,
        destination.buffer,
        destination.font,
        destination.link,
    );
    let WordOutput {
        output,
        buffer,
        link,
        zero_advance,
        ..
    } = &mut destination;
    // Code contributes generated glyphs, without changing source registers or
    // delegating its presentation break decision to a definition field.
    append_text_event(
        value,
        WordOutput {
            output,
            buffer,
            font: Font::Code,
            link: *link,
            zero_advance,
        },
        text_state,
        false,
    );
    flush_segment(
        destination.output,
        destination.buffer,
        Font::Code,
        destination.link,
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_recording_does_not_select_another_text_executor() {
        // The exact recording-mode.roff input passed pristine CVS ASCII,
        // UTF-8 and lint. term_word() buffers X, ESCAPE_BREAK and
        // ASCII_NBRZW in order; recording is independent of presentation.
        let run = |record| {
            let mut font = FontState::new();
            let mut zero = ZeroAdvanceState::new();
            parse_roff_text_with_zero_advance(
                "X\\p\\&",
                TextExecutionContext {
                    font: &mut font,
                    zero_advance: &mut zero,
                    pending_word_end_break: false,
                    policy: TextExecutionPolicy {
                        recognize_generated_references: false,
                        record_native_cells: record,
                        field_authoritative: false,
                    },
                },
            )
        };
        let ordinary = run(false);
        let field = run(true);
        assert_eq!(ordinary.output, field.output);
        assert_eq!(
            ordinary.pending_word_end_break,
            field.pending_word_end_break
        );
        assert_eq!(ordinary.source_continuation, field.source_continuation);
        assert!(ordinary.native_writes.is_empty());
        assert_eq!(field.native_writes.len(), 3);
    }

    #[test]
    fn unknown_recovery_preserves_source_order_without_consuming_zero_advance() {
        // Exact complete roff fixtures first verified with pristine CVS.
        // term.c:620-638 writes NBRZW directly; the following encode1 glyph
        // alone consumes BACKAFTER/BACKBEFORE. Recovery spelling remains
        // visible under ManT's reading contract and retains its source style.
        for (source, expected) in [
            (r"\z\[unknownname]YZ", "Z"),
            (r"\z\N'256'YZ", "Z"),
            (r"\zX\[unknownname]", r"X\[unknownname]"),
            (r"\zX\[unknownname]Y", r"\[unknownname]Y"),
            (r"\zX\[unknownname]\[u03B1]", "\\[unknownname]α"),
            (r"\zX\[unknownname] Y", r"X\[unknownname]Y"),
            (r"X\[unknownname]Y", r"X\[unknownname]Y"),
        ] {
            let output = parse_roff_text_with_state(source, &mut FontState::new(), false);
            assert_eq!(mant_ir::inline_plain_text(&output), expected, "{source}");
        }
        let output =
            parse_roff_text_with_state(r"\zX\[unknownname]\fBY", &mut FontState::new(), false);
        assert_eq!(mant_ir::inline_plain_text(&output), r"\[unknownname]Y");
        assert!(matches!(output.last(), Some(Inline::Strong { children })
            if mant_ir::inline_plain_text(children) == "Y"));
        assert!(matches!(output.first(), Some(Inline::Text { value })
            if value == r"\[unknownname]"));
    }
}
