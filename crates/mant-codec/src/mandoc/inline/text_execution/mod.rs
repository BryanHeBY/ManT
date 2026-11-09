//! Execute decoded word events against borrowed formatter registers.
//!
//! Native writes are recorded before presentation; output ownership never
//! selects another decoder or resets the caller's persistent text state.

mod event_state;
mod native_writes;
mod references;
mod word;
mod zero_advance;
pub(in crate::mandoc) use zero_advance::ZeroAdvanceState;

use event_state::append_text_event;
use native_writes::record_native_event;
use references::append_empty_destination;

use super::flow::TrailingOutput;
use super::font::style::flush_segment;
use super::{Font, FontState, Inline, RoffInlineEvent, is_formatter_word_blank};

/// Independent text-word policies, chosen by the caller's native sink.
/// Recording does not select another executor, and a field receipt may own
/// acceptance while generated-reference recognition remains independently on.
#[derive(Clone, Copy)]
pub(in crate::mandoc) struct TextExecutionPolicy {
    pub(in crate::mandoc) recognize_generated_references: bool,
    pub(in crate::mandoc) record_native_cells: bool,
    pub(in crate::mandoc) field_authoritative: bool,
    pub(in crate::mandoc) word_space: crate::mandoc::roff_escape::WordSpace,
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
                word_space: crate::mandoc::roff_escape::WordSpace::Breakable,
            },
        },
    );
    let mut output = execution.output;
    if execution.pending_word_end_break {
        output.push(Inline::line_break());
    }
    zero_advance.finish_into(&mut output);
    if let Some(annotations) = &zero_advance.annotations {
        annotations.finish_local(&mut output);
    }
    (output, execution.escape_scan)
}

// Native joining, continuation, breaks and acceptance are independent results.
#[allow(clippy::struct_excessive_bools)]
pub(in crate::mandoc) struct TextExecution {
    pub(in crate::mandoc) escape_scan: crate::mandoc::escape_coverage::EscapeScanStatus,
    pub(in crate::mandoc) output: Vec<Inline>,
    /// Native writes in decode order; semantic wrappers never supply cells.
    pub(in crate::mandoc) native_writes: Vec<crate::mandoc::inline::flow::field_buffer::FieldWrite>,
    /// Device motion, lines and blank overstrikes can buffer without text.
    pub(in crate::mandoc) table_device_buffer_written: bool,
    pub(in crate::mandoc) table_line_needs_width: bool,
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

/// Decode a text node while retaining formatter state in its caller's inline
/// stream. Joining, source continuation and word-end breaks remain independent.
pub(in crate::mandoc) fn parse_roff_text_with_zero_advance(
    source: &str,
    context: TextExecutionContext<'_>,
) -> TextExecution {
    let decoded =
        crate::mandoc::roff_escape::decode_word_with_status(source, context.policy.word_space);
    let mut execution = execute_formatter_word_events(&decoded.events, context);
    execution.escape_scan = decoded.budget_exhausted.into();
    execution
}

/// A typed source event separates generated leading motion from payload.
pub(in crate::mandoc) const CONTENT_AFTER_MOTION: &str = "\0mant:output-scope:content-after-motion";

#[derive(Clone, Copy, PartialEq)]
enum ContentPrefix {
    Empty,
    Motion,
    Authored,
}

/// One word's presentation and native writes borrow persistent registers.
struct WordExecution {
    content_prefix: ContentPrefix,
    output: Vec<Inline>,
    buffer: String,
    font: Font,
    link: Option<String>,
    source_continuation: Option<bool>,
    text_state: TextEventState,
    native_writes: Vec<crate::mandoc::inline::flow::field_buffer::FieldWrite>,
    table_device_buffer_written: bool,
    table_line_needs_width: bool,
}

// Record each decoded native write before its presentation action, in the
// same term_word() order. Final flush precedes taking the joining receipt.
fn execute_formatter_word_events(
    events: &[RoffInlineEvent],
    mut context: TextExecutionContext<'_>,
) -> TextExecution {
    let mut word = WordExecution::new(&context);
    context.zero_advance.begin_fragment();
    for (index, event) in events.iter().enumerate() {
        // These native writes have no reading-text glyph. Keep their receipt
        // separate from FieldBuffer's graph/width model. Once any column has
        // buffered input its later flush retires BACKAFTER for the whole row.
        word.table_device_buffer_written |= match event {
            RoffInlineEvent::Presentation {
                kind: crate::mandoc::roff_escape::PresentationKind::HorizontalLine,
                argument: Some(argument),
            } => {
                match libmandoc_rs::TableColumnMetrics::horizontal_line_needs_column_width(argument)
                {
                    Some(false) => true,
                    Some(true) => {
                        word.table_line_needs_width = true;
                        false
                    }
                    None => false,
                }
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(argument),
                ..
            } if !context.zero_advance.is_armed() => {
                libmandoc_rs::TableColumnMetrics::horizontal_motion_has_input(argument)
            }
            RoffInlineEvent::Overstrike { buffered, .. } => *buffered,
            _ => false,
        };
        if context.policy.record_native_cells {
            if let RoffInlineEvent::HorizontalMotion {
                reading_blank: true,
                argument,
            } = event
                && !context.zero_advance.is_armed()
            {
                use crate::mandoc::inline::flow::field_buffer::{FieldCell, FieldWrite};
                let pending = context.zero_advance.has_pending_glyph();
                if pending {
                    // ESCAPE_HORIZ consumes the first advance by clearing
                    // BACKBEFORE, preserving the previous graph (term.c:692).
                    word.native_writes.push(FieldWrite::CancelBackbefore);
                }
                if !pending || argument.as_deref().is_some_and(
                    libmandoc_rs::TableColumnMetrics::horizontal_motion_has_input_after_backbefore,
                ) {
                    word.native_writes.push(FieldWrite::Cell(FieldCell::NonBreakingBlank));
                }
            }
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
        word.append_source_event(event, &mut context, index + 1 == events.len());
    }
    word.finish_content_prefix();
    word.flush_segment();
    finish_text_execution(events, context.zero_advance, word)
}

fn finish_text_execution(
    events: &[RoffInlineEvent],
    zero_advance: &mut ZeroAdvanceState,
    word: WordExecution,
) -> TextExecution {
    let WordExecution {
        output,
        native_writes,
        table_device_buffer_written,
        table_line_needs_width,
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
        table_device_buffer_written,
        table_line_needs_width,
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
fn trailing_breakable_spaces(events: &[RoffInlineEvent]) -> usize {
    let mut count = 0;
    for event in events.iter().rev() {
        match event {
            RoffInlineEvent::Text(value) => {
                if value.is_empty() {
                    continue;
                }
                let trailing = value.chars().rev().take_while(|&ch| ch == ' ').count();
                count += trailing;
                if trailing != value.chars().count() {
                    return count;
                }
            }

            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::ZeroAdvance
            | RoffInlineEvent::NoSpace
            | RoffInlineEvent::HorizontalMotion { .. }
            | RoffInlineEvent::Presentation { .. }
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint
            | RoffInlineEvent::LineBreak => {}

            RoffInlineEvent::Glyph(_)
            | RoffInlineEvent::NonBreakingWordBlank
            | RoffInlineEvent::BreakableHyphen
            | RoffInlineEvent::FallbackGlyph(_)
            | RoffInlineEvent::DeviceName
            | RoffInlineEvent::Overstrike { .. }
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::EmptyDestination => return count,
        }
    }
    count
}

#[cfg(test)]
mod tests;
