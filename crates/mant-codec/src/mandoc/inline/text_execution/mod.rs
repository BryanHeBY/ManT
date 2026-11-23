//! Execute decoded word events against borrowed formatter registers.
//!
//! Native writes are recorded before presentation; output ownership never
//! selects another decoder or resets the caller's persistent text state.

mod event_state;
mod geometry_word;
mod native_writes;
mod references;
mod word;
mod zero_advance;
pub(in crate::mandoc) use zero_advance::ZeroAdvanceState;

use super::flow::{
    field_buffer::FieldWrite,
    native_slots::{PriorSlotEdit, SlotEntry},
};
use event_state::append_text_event;
pub(in crate::mandoc::inline) use native_writes::record_native_event;
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
    pub(in crate::mandoc) defer_leading_blanks: bool,
    /// A still buffered marker can first reach `ASCII_BREAK` in this word.
    /// Native right margin minus offset, for lengthless drawing operands.
    pub(in crate::mandoc) line_width_units: usize,
    /// The live field already has native input to which this word can refer.
    pub(in crate::mandoc) slot_entry: SlotEntry,
    /// Native tbl field bound for words that explicitly mutate device slots.
    pub(in crate::mandoc) table_field_units: Option<usize>,
    /// Non-breaking slots written by numeric tbl alignment before this word.
    pub(in crate::mandoc) table_leading_blanks: usize,
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
            defer_leading_blanks: false,
            line_width_units: 78 * 24,
            slot_entry: SlotEntry::default(),
            table_field_units: None,
            table_leading_blanks: 0,
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
    /// Hard rows already resolved from the accepted tbl device passes.
    pub(in crate::mandoc) table_reading_breaks: Option<usize>,
    /// Basic units of a proved relative `HORIZ` still left at raw column zero.
    pub(in crate::mandoc) native_origin_retreat_units: usize,
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
    execution.escape_scan = decoded.budget_exhausted().into();
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
    drawing_probe: Option<Box<crate::mandoc::inline::flow::field_buffer::DrawingProbe>>,
    table_device_buffer_written: bool,
    table_reading_breaks: Option<usize>,
    native_origin_retreat_units: usize,
}

// Record each decoded native write before its presentation action, in the
// same term_word() order. Final flush precedes taking the joining receipt.
fn execute_formatter_word_events(
    events: &[RoffInlineEvent],
    mut context: TextExecutionContext<'_>,
) -> TextExecution {
    let mut word = WordExecution::new(&context);
    context.zero_advance.begin_fragment();
    let prior_edit = if geometry_word::applies(events, &context) {
        match geometry_word::try_execute(events, &mut context, &mut word) {
            Ok(()) => return finish_text_execution(events, context.zero_advance, word),
            Err(edit) => Some(edit),
        }
    } else {
        None
    };
    let entry_decorated = native_writes::has_native_decoration(context.font.current);
    let mut non_font_opaque = false;
    let mut wrote_decorated = false;
    let retained_previous_glyph =
        context.slot_entry.retreat_blanks > 0 || context.zero_advance.has_pending_glyph();
    let can_record_last_font =
        !retained_previous_glyph && !context.zero_advance.is_armed() && prior_edit.is_none();
    let mut last_font = None;
    let mut appended_fonts = (can_record_last_font
        && context.policy.field_authoritative
        && (entry_decorated
            || events.iter().any(|event| {
                matches!(
                    event,
                    RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont
                )
            })))
    .then(native_writes::AppendedFonts::default);
    let mut unproved_motion = false;
    for (index, event) in events.iter().enumerate() {
        let armed_before = context.zero_advance.is_armed();
        unproved_motion |= context.policy.field_authoritative
            && retained_previous_glyph
            && !armed_before
            && geometry_word::absolute_motion(event);
        non_font_opaque |= needs_native_slot_proof(event, armed_before);
        let write_start = word.native_writes.len();
        // These native writes have no reading-text glyph. Keep their receipt
        // separate from FieldBuffer's graph/width model. Once any column has
        // buffered input its later flush retires BACKAFTER for the whole row.
        word.table_device_buffer_written |= event_writes_table_device_buffer(event, armed_before);
        if context.policy.record_native_cells
            && (!context.policy.field_authoritative || !matches!(event, RoffInlineEvent::Text(_)))
        {
            record_native_event(
                event,
                &mut word.native_writes,
                context.zero_advance.fallback_is_projected(),
            );
        }
        word.append_source_event(event, &mut context, index + 1 == events.len());
        native_writes::qualify_rewound_graphs(
            &mut word.native_writes,
            write_start,
            context.font.current,
        );
        if word.native_writes.len() > write_start {
            wrote_decorated |= native_writes::writes_decorated_graph(
                &word.native_writes[write_start..],
                context.font.current,
            );
            last_font = match last_font {
                Some((font, start)) if font == context.font.current => Some((font, start)),
                _ => Some((context.font.current, write_start)),
            };
            if appended_fonts.as_mut().is_some_and(|append| {
                !append.record(&word.native_writes[write_start..], context.font.current)
            }) {
                appended_fonts = None;
            }
        }
    }
    if unproved_motion
        || prior_edit.is_some_and(|edit| {
            edit == PriorSlotEdit::Unresolved
                || !matches!(
                    word.native_writes.first(),
                    Some(FieldWrite::RewindWordEntry(_))
                )
        })
    {
        word.native_writes.push(FieldWrite::UnprovedMotion);
    }
    if (non_font_opaque
        || wrote_decorated
        || retained_previous_glyph
            && context.zero_advance.has_annotation_output_owner()
            && !word.native_writes.is_empty())
        && context.policy.record_native_cells
        && context.policy.field_authoritative
    {
        record_word_slot_proof(
            events,
            &mut word.native_writes,
            appended_fonts.filter(|_| !unproved_motion),
            last_font.filter(|_| prior_edit.is_none()),
        );
    }
    word.finish_content_prefix();
    word.flush_segment();
    finish_text_execution(events, context.zero_advance, word)
}

fn event_writes_table_device_buffer(event: &RoffInlineEvent, armed: bool) -> bool {
    match event {
        RoffInlineEvent::HorizontalMotion {
            argument: Some(argument),
            ..
        } if !armed => libmandoc_rs::TableColumnMetrics::horizontal_motion_has_input(argument),
        RoffInlineEvent::Overstrike { buffered, .. } => *buffered,
        _ => false,
    }
}

fn needs_native_slot_proof(event: &RoffInlineEvent, armed: bool) -> bool {
    match event {
        RoffInlineEvent::Overstrike { .. } => true,
        // ESCAPE_HORIZ clears BACKAFTER without buffering or moving when
        // armed (term.c:678-682), and a parsed zero relative motion writes
        // no raw slots (term.c:678-699). Keep ordinary word execution.
        RoffInlineEvent::HorizontalMotion { argument, .. } => {
            !armed && !geometry_word::relative_zero_motion(argument.as_deref())
        }
        _ => false,
    }
}

fn record_word_slot_proof(
    events: &[RoffInlineEvent],
    writes: &mut Vec<FieldWrite>,
    appended: Option<native_writes::AppendedFonts>,
    last_font: Option<(Font, usize)>,
) {
    // A preceding pending glyph can change the leading bytes, but
    // encode1 still writes exact copies of the final styled glyphs.
    let styled_suffix = last_font.and_then(|(font, start)| {
        native_writes::proved_decorated_tail(events, &writes[start..], font)
    });
    if let Some(proof) = appended.and_then(|append| append.proof(styled_suffix)) {
        writes.push(proof);
    } else {
        writes.push(FieldWrite::OpaqueRawSlots {
            styled_suffix,
            keeps_prior_slots: false,
        });
    }
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
        table_reading_breaks,
        native_origin_retreat_units,
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
        table_reading_breaks,
        native_origin_retreat_units,
        joins_preceding_node: zero_advance.take_preceding_join(),
        source_continuation,
        pending_word_end_break: text_state.pending_word_end_break,
        pending_word_end_break_separated: text_state.pending_word_end_break
            && text_state.saw_source_cell
            && !text_state.graph_since_blank,
        definitive_reject: text_state.wiped,
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

            RoffInlineEvent::Glyph(..)
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
