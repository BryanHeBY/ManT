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
    /// A formatter blank followed `\p` before this word supplied a graph.
    /// The HANG field owner combines this with graphs from earlier words.
    pub(in crate::mandoc) break_before_graph: bool,
    pub(in crate::mandoc) trailing_output: TrailingOutput,
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
    break_before_graph: bool,
}

impl TextEventState {
    const fn new(pending_word_end_break: bool) -> Self {
        Self {
            pending_word_end_break,
            suppress_break_whitespace: false,
            graph_seen: false,
            break_before_graph: false,
        }
    }
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
            state.break_before_graph |= !state.graph_seen;
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
        state.graph_seen |= !is_formatter_word_blank(character) && character != '\n';
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
    )
}

pub(in crate::mandoc) fn parse_formatter_word_parts_with_zero_advance(
    parts: &[FormatterWordPart<'_>],
    state: &mut FontState,
    recognize_generated_references: bool,
    zero_advance: &mut ZeroAdvanceState,
    pending_word_end_break: bool,
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
    )
}

// Keep the decoded escape events in their native term_word() order.
#[allow(clippy::too_many_lines)]
fn execute_formatter_word_events(
    events: &[FormatterWordEvent],
    state: &mut FontState,
    recognize_generated_references: bool,
    zero_advance: &mut ZeroAdvanceState,
    pending_word_end_break: bool,
) -> TextExecution {
    let (mut output, mut buffer) = (Vec::new(), String::new());
    let mut font = state.display_current();
    let mut link: Option<String> = None;
    let mut explicit_line_continuation = None;
    let mut text_state = TextEventState::new(pending_word_end_break);
    zero_advance.begin_fragment();
    for (index, event) in events.iter().enumerate() {
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
                zero_advance.append_glyph(value, &mut buffer, font, link.as_deref());
            }
            FormatterWordEvent::Source(RoffInlineEvent::FallbackGlyph(value)) => {
                if zero_advance.append_fallback_glyph(value, &mut buffer, font, link.as_deref()) {
                    text_state.suppress_break_whitespace = false;
                    text_state.graph_seen = true;
                }
            }
            FormatterWordEvent::Source(RoffInlineEvent::DeviceName) => {
                text_state.suppress_break_whitespace = false;
                text_state.graph_seen = true;
                zero_advance.append_text("utf8", &mut output, &mut buffer, font, link.as_deref());
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
                append_empty_destination(
                    &mut output,
                    &mut buffer,
                    font,
                    link.as_deref(),
                    recognize_generated_references,
                    &mut text_state,
                );
            }
            FormatterWordEvent::Source(RoffInlineEvent::LineBreak) => {
                text_state.pending_word_end_break = true;
            }
            FormatterWordEvent::Source(RoffInlineEvent::ZeroWidthGlyph) => {
                text_state.graph_seen = true;
            }
            FormatterWordEvent::Source(
                RoffInlineEvent::Presentation { .. }
                | RoffInlineEvent::Overstrike { terminal: None, .. },
            ) => {}
        }
    }
    flush_segment(&mut output, &mut buffer, font, link.as_deref());
    finish_text_execution(
        events,
        output,
        zero_advance,
        explicit_line_continuation,
        text_state.pending_word_end_break,
        text_state.break_before_graph,
    )
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
    output: Vec<Inline>,
    zero_advance: &mut ZeroAdvanceState,
    source_continuation: Option<bool>,
    pending_word_end_break: bool,
    break_before_graph: bool,
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
        joins_preceding_node: zero_advance.take_preceding_join(),
        source_continuation,
        pending_word_end_break,
        break_before_graph,
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
