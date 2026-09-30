use super::super::super::reference::trailing_sphinx_manual_reference;
use super::style::{flush_segment, styled_segment};
use super::{
    Font, FontState, Inline, RoffInlineEvent, TrailingOutput, ZeroAdvanceState, decode,
    is_formatter_word_blank,
};

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
        false,
        false,
    );
    let mut output = execution.output;
    if execution.pending_word_end_break {
        output.push(Inline::line_break());
    }
    zero_advance.finish_into(&mut output);
    output
}

#[allow(clippy::struct_excessive_bools)]
pub(in crate::mandoc) struct TextExecution {
    pub(in crate::mandoc) output: Vec<Inline>,
    /// Native writes in decode order; semantic wrappers never supply cells.
    pub(in crate::mandoc) native_writes: Vec<super::super::flow::field_buffer::FieldWrite>,
    pub(in crate::mandoc) joins_preceding_node: bool,
    pub(in crate::mandoc) source_continuation: Option<bool>,
    pub(in crate::mandoc) pending_word_end_break: bool,
    pub(in crate::mandoc) trailing_output: TrailingOutput,
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

// Decoder controls and field-proof observations coexist during one word.
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
    /// A `\p` marker met a surviving breakable blank: `term_fill()` stopped
    /// the pass with `nbr == 0` (term.c:143-146) and `term_flushln()` wiped
    /// the unprinted remainder of the flush unit (term.c:233-237).
    wiped: bool,
    /// A zero-width graph class cell (NBRZW) armed `graph` this word.
    zero_graph_seen: bool,
}

impl TextEventState {
    const fn new(pending_word_end_break: bool) -> Self {
        Self {
            pending_word_end_break,
            suppress_break_whitespace: false,
            graph_seen: false,
            last_breakable_blank: false,
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

#[allow(clippy::too_many_arguments)]
fn append_text_event(
    value: &str,
    output: &mut Vec<Inline>,
    buffer: &mut String,
    font: Font,
    link: Option<&str>,
    zero_advance: &mut ZeroAdvanceState,
    state: &mut TextEventState,
    field_authoritative: bool,
) {
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
            if zero_advance.has_pending_glyph() {
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
            if field_authoritative {
                output.push(Inline::line_break());
            } else {
                let tail_accepted = state.graph_since_blank;
                if tail_accepted || state.graph_seen {
                    output.push(Inline::line_break());
                }
                if !tail_accepted {
                    state.wiped = true;
                }
            }
            state.pending_word_end_break = false;
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
/// stream.  The result reports cross-node zero-advance joining, the final
/// physical-line decision, and a deferred word-end break independently.
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
pub(in crate::mandoc) fn parse_roff_text_with_zero_advance(
    source: &str,
    state: &mut FontState,
    recognize_generated_references: bool,
    zero_advance: &mut ZeroAdvanceState,
    pending_word_end_break: bool,
    record_native_cells: bool,
    field_authoritative: bool,
) -> TextExecution {
    let events = decode(source)
        .into_iter()
        .map(FormatterWordEvent::Source)
        .collect::<Vec<_>>();
    execute_formatter_word_events(
        &events,
        state,
        recognize_generated_references,
        zero_advance,
        pending_word_end_break,
        record_native_cells,
        field_authoritative,
    )
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
pub(in crate::mandoc) fn parse_formatter_word_parts_with_zero_advance(
    parts: &[FormatterWordPart<'_>],
    state: &mut FontState,
    recognize_generated_references: bool,
    zero_advance: &mut ZeroAdvanceState,
    pending_word_end_break: bool,
    record_native_cells: bool,
    field_authoritative: bool,
) -> TextExecution {
    let events = parts
        .iter()
        .flat_map(|part| match part {
            FormatterWordPart::Source(source) => decode(source)
                .into_iter()
                .map(FormatterWordEvent::Source)
                .collect::<Vec<_>>(),
            FormatterWordPart::Code(value) => {
                vec![FormatterWordEvent::Code(value.clone())]
            }
        })
        .collect::<Vec<_>>();
    execute_formatter_word_events(
        &events,
        state,
        recognize_generated_references,
        zero_advance,
        pending_word_end_break,
        record_native_cells,
        field_authoritative,
    )
}

// Keep the decoded escape events in their native term_word() order.
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools
)]
fn execute_formatter_word_events(
    events: &[FormatterWordEvent],
    state: &mut FontState,
    recognize_generated_references: bool,
    zero_advance: &mut ZeroAdvanceState,
    pending_word_end_break: bool,
    record_native_cells: bool,
    // The definition-field session owns the pass arithmetic for this word
    // (`FieldBuffer::flush_receipt`): a marker's break blank defers to it
    // instead of the ordinary-flow wipe decision here.
    field_authoritative: bool,
) -> TextExecution {
    let (mut output, mut buffer) = (Vec::new(), String::new());
    let mut font = state.display_current();
    let mut link: Option<String> = None;
    let mut explicit_line_continuation = None;
    let mut text_state = TextEventState::new(pending_word_end_break);
    let mut native_writes = Vec::new();
    zero_advance.begin_fragment();
    for (index, event) in events.iter().enumerate() {
        if record_native_cells {
            record_native_event(
                event,
                &mut native_writes,
                zero_advance.fallback_is_projected(),
            );
        }
        match event {
            FormatterWordEvent::Code(value) => {
                append_code_event(
                    value,
                    &mut output,
                    &mut buffer,
                    font,
                    link.as_deref(),
                    zero_advance,
                    &mut text_state,
                );
            }
            FormatterWordEvent::Source(RoffInlineEvent::Text(value)) => {
                append_text_event(
                    value,
                    &mut output,
                    &mut buffer,
                    font,
                    link.as_deref(),
                    zero_advance,
                    &mut text_state,
                    field_authoritative,
                );
            }
            FormatterWordEvent::Source(
                RoffInlineEvent::Glyph(value)
                | RoffInlineEvent::Overstrike {
                    terminal: Some(value),
                    ..
                },
            ) => {
                text_state.suppress_break_whitespace = false;
                text_state.graph_seen = true;
                text_state.graph_since_blank = true;
                text_state.graph_since_break |= text_state.pending_word_end_break;
                text_state.last_breakable_blank = false;
                text_state.trailing_breakable_blanks = 0;
                text_state.graph_count += 1;
                if !text_state.wiped {
                    zero_advance.append_glyph(
                        value,
                        &mut output,
                        &mut buffer,
                        font,
                        link.as_deref(),
                    );
                }
            }
            FormatterWordEvent::Source(RoffInlineEvent::FallbackGlyph(value)) => {
                if !text_state.wiped {
                    zero_advance.append_fallback_glyph(value, &mut buffer, font, link.as_deref());
                }
                text_state.graph_since_blank = true;
                // Native recovery is still a zero-width graph even when its
                // source spelling has no semantic contribution after \z.
                text_state.suppress_break_whitespace = false;
                text_state.graph_seen = true;
                text_state.graph_since_break |= text_state.pending_word_end_break;
                text_state.last_breakable_blank = false;
                text_state.trailing_breakable_blanks = 0;
                text_state.graph_count += 1;
            }
            FormatterWordEvent::Source(RoffInlineEvent::DeviceName) => {
                text_state.suppress_break_whitespace = false;
                text_state.graph_seen = true;
                text_state.graph_since_blank = true;
                text_state.graph_since_break |= text_state.pending_word_end_break;
                text_state.last_breakable_blank = false;
                text_state.trailing_breakable_blanks = 0;
                if !text_state.wiped {
                    zero_advance.append_text(
                        "utf8",
                        &mut output,
                        &mut buffer,
                        font,
                        link.as_deref(),
                    );
                }
            }
            FormatterWordEvent::Source(RoffInlineEvent::ZeroAdvance) => zero_advance.arm(),
            FormatterWordEvent::Source(RoffInlineEvent::NoSpace) => {
                let canceled_armed = zero_advance.cancel_armed_for_no_space();
                if index + 1 == events.len() {
                    explicit_line_continuation = Some(!canceled_armed);
                }
            }
            FormatterWordEvent::Source(RoffInlineEvent::Font(next_font)) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                state.select(*next_font);
                font = state.display_current();
            }
            FormatterWordEvent::Source(RoffInlineEvent::PreviousFont) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                state.restore();
                font = state.display_current();
            }
            FormatterWordEvent::Source(RoffInlineEvent::Link(target)) => {
                flush_segment(&mut output, &mut buffer, font, link.as_deref());
                link.clone_from(target);
            }
            FormatterWordEvent::Source(RoffInlineEvent::EmptyDestination) => {
                if !text_state.wiped {
                    append_empty_destination(
                        &mut output,
                        &mut buffer,
                        font,
                        link.as_deref(),
                        recognize_generated_references,
                        &mut text_state,
                    );
                }
            }
            FormatterWordEvent::Source(RoffInlineEvent::LineBreak) => {
                text_state.pending_word_end_break = true;
                text_state.break_started_after_blank =
                    text_state.last_breakable_blank && text_state.graph_seen;
                text_state.break_trailing_blanks = text_state.trailing_breakable_blanks;
                text_state.graph_since_break = false;
            }
            FormatterWordEvent::Source(RoffInlineEvent::ZeroWidthGlyph) => {
                text_state.graph_seen = true;
                text_state.graph_since_blank = true;
                text_state.graph_since_break |= text_state.pending_word_end_break;
                text_state.last_breakable_blank = false;
                text_state.trailing_breakable_blanks = 0;
                text_state.zero_graph_seen = true;
            }

            FormatterWordEvent::Source(RoffInlineEvent::Presentation {
                kind: crate::mandoc::roff_escape::PresentationKind::HorizontalMotion,
                ..
            }) => {
                // A positive `\h` consumed by TERMP_BACKAFTER clears the arm
                // and skips the advance entirely (term.c:677-680): the
                // decoder's semantic boundary space must not print either.
                if zero_advance.take_armed() && buffer.ends_with(' ') {
                    buffer.pop();
                }
            }
            FormatterWordEvent::Source(
                RoffInlineEvent::Presentation { .. }
                | RoffInlineEvent::Overstrike { terminal: None, .. },
            ) => {}
        }
    }
    flush_segment(&mut output, &mut buffer, font, link.as_deref());
    zero_advance.clear_marker_blank_separator();
    finish_text_execution(
        events,
        output,
        zero_advance,
        explicit_line_continuation,
        text_state.pending_word_end_break,
        text_state.wiped,
        text_state.zero_graph_seen,
        native_writes,
    )
}

/// `term.c::term_word()` buffers controls and invisible cells before
/// `term_field()` projects printable output. Record these facts independently
/// of the semantic IR and the zero-advance presentation machine.
fn record_native_event(
    event: &FormatterWordEvent,
    writes: &mut Vec<super::super::flow::field_buffer::FieldWrite>,
    fallback_projected: bool,
) {
    use super::super::flow::field_buffer::{FieldCell, FieldWrite};
    match event {
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

#[allow(clippy::too_many_arguments)]
fn finish_text_execution(
    events: &[FormatterWordEvent],
    output: Vec<Inline>,
    zero_advance: &mut ZeroAdvanceState,
    source_continuation: Option<bool>,
    pending_word_end_break: bool,
    definitive_reject: bool,
    word_zero_graph: bool,
    native_writes: Vec<super::super::flow::field_buffer::FieldWrite>,
) -> TextExecution {
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
        output,
        native_writes,
        joins_preceding_node: zero_advance.take_preceding_join(),
        source_continuation,
        pending_word_end_break,
        definitive_reject,
        word_zero_graph,
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
    output: &mut Vec<Inline>,
    buffer: &mut String,
    current_font: Font,
    link: Option<&str>,
    zero_advance: &mut ZeroAdvanceState,
    text_state: &mut TextEventState,
) {
    flush_segment(output, buffer, current_font, link);
    append_text_event(
        value,
        output,
        buffer,
        Font::Code,
        link,
        zero_advance,
        text_state,
        false,
    );
    flush_segment(output, buffer, Font::Code, link);
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
                "X\\p\\&", &mut font, false, &mut zero, false, record, false,
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
