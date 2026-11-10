//! Execution and projection of an input formatter word.
use libmandoc_rs::Node;
use mant_ir::Inline;

use super::{InlineBuilder, flow::WordProjectionPolicy, font};
use crate::mandoc::roff_escape::{RoffInlineEvent, decode, is_formatter_word_blank};

pub(super) fn append_text_node(builder: &mut InlineBuilder, node: &Node) {
    builder.begin_executed_node(node);
    let source = node.decoder_text().unwrap_or_default();
    if builder.execute_text_source_boundary(node, source) {
        return;
    }
    if node.flags.delimiter_close {
        builder.tighten_next_boundary();
    }
    append_roff_word(builder, source, WordSource::Text(node));
}

/// Es stores normalized operand spelling on En. Execute those bytes now,
/// at En's current source/owner, without re-entering the Es definition node.
pub(in crate::mandoc) fn append_normalized_word(builder: &mut InlineBuilder, source: &str) {
    append_roff_word(builder, source, WordSource::NormalizedOperand);
}

pub(super) fn append_equation_word(builder: &mut InlineBuilder, source: &str) -> bool {
    append_roff_word(builder, source, WordSource::EquationOperand)
}

#[derive(Clone, Copy)]
enum WordSource<'a> {
    Text(&'a Node),
    NormalizedOperand,
    EquationOperand,
}

impl WordSource<'_> {
    fn projection(self) -> WordProjectionPolicy {
        match self {
            Self::Text(node) if node.flags.no_fill && node.flags.line_start => {
                WordProjectionPolicy::SourceLiteralWhitespace
            }
            _ => WordProjectionPolicy::Printed,
        }
    }
}

fn append_roff_word(builder: &mut InlineBuilder, source: &str, origin: WordSource<'_>) -> bool {
    let events = decode(source);
    // `term_word()` consumes its inter-word boundary even for an explicit
    // empty operand. That word event can resolve a preceding `\\z` glyph
    // before generated enclosure punctuation is emitted. Control *nodes* are
    // routed separately and therefore do not gain this behavior.
    let has_glyph = events.iter().any(|event| match event {
        RoffInlineEvent::Text(value)
        | RoffInlineEvent::Glyph(value)
        | RoffInlineEvent::FallbackGlyph(value) => value
            .chars()
            .any(|character| !is_formatter_word_blank(character) && character != '\n'),
        RoffInlineEvent::BreakableHyphen
        | RoffInlineEvent::DeviceName
        | RoffInlineEvent::Overstrike { .. } => true,
        _ => false,
    });
    let settled_boundary_glyph = builder.begin_word_projection_with_break(
        true,
        !builder.in_definition_field() || has_glyph,
        leading_event_needs_native_retreat(&events, builder.execution.zero_advance.is_armed()),
    );
    builder.execution.settled_boundary_glyph =
        settled_boundary_glyph && matches!(origin, WordSource::Text(_));
    builder.clear_word_end_break();
    // term_word() stores a previous operand's \p in the native buffer.
    // Its actual automatic separator is consumed by term_fill(), before
    // this operand's internal blanks. Do not replay that older marker in
    // the new TEXT projection: the field's ordered cell consumer decides
    // its row boundary (term.c:573-580,287-306).
    // The plain flush unit owns the same pass arithmetic as a definition
    // field (term.c runs one term_fill() over tcol->buf regardless of
    // authorship), so its marker blank defers to the shared cell consumer
    // instead of a local wipe decision. No-fill borrows the live output sink
    // until its native row retires; isolated table words retain their
    // independent cell scope.
    let field_authoritative = true;
    let defer_leading_blanks = builder.defers_blanks_after_prior_breakpoint();
    let word_space = builder.word_operand_spaces();
    builder.begin_native_word_owner();
    // pre_alternate selects an operand role before this actual term_word;
    // internal font escapes remain within this same word identity.
    if matches!(origin, WordSource::Text(_)) {
        builder.record_current_native_operand(
            events
                .iter()
                .any(|event| matches!(event, RoffInlineEvent::Font(_))),
        );
    }
    let recognize_generated_references = match origin {
        WordSource::Text(node) => !node.flags.no_fill,
        WordSource::NormalizedOperand => !builder.execution.no_fill_word_active,
        WordSource::EquationOperand => false,
    };
    let execution = font::parse_roff_text_with_zero_advance(
        source,
        font::TextExecutionContext {
            font: &mut builder.execution.font,
            zero_advance: &mut builder.execution.zero_advance,
            defer_leading_blanks,
            policy: font::TextExecutionPolicy {
                recognize_generated_references,
                record_native_cells: true,
                field_authoritative,
                word_space,
            },
        },
    );
    builder.ensure_definition_field_session();
    builder.escape_coverage.record(execution.escape_scan);
    if let Some(buffered) = &mut builder.table_buffer_written {
        *buffered |= execution.table_device_buffer_written;
        builder.execution.table_line_needs_width |= execution.table_line_needs_width;
    }
    builder.native_word_writes = Some(execution.native_writes);
    // A visited empty source TEXT takes the vertical path before reaching
    // here. roff_term_pre_ce() executes its empty TEXT as a direct term_word:
    // its NODE_LINE is a grouping delimiter, not an occupied cell.
    let occupies_literal_row = !execution.output.is_empty()
        || builder.zero_advance.has_buffered_glyph()
        || events.iter().any(|event| {
            matches!(
                event,
                RoffInlineEvent::ZeroWidthGlyph | RoffInlineEvent::Breakpoint
            )
        });
    if execution.joins_preceding_node {
        builder.join_projected_boundary();
        builder.note_zero_advance_join();
    }
    let provisional_definition_break = execution.pending_word_end_break
        && matches!(execution.output.last(), Some(Inline::LineBreak { .. }));
    builder.append_word_with_literal_row(
        execution.output,
        occupies_literal_row,
        execution.trailing_output,
        origin.projection(),
    );
    if execution.definitive_reject {
        builder.note_definitive_word_rejection();
    }
    if provisional_definition_break {
        builder.note_provisional_definition_break();
    }
    if execution.pending_word_end_break {
        builder.request_word_end_break(execution.pending_word_end_break_separated);
    } else {
        builder.retain_buffered_field_word_end_break();
    }
    let continues_line = execution
        .source_continuation
        .unwrap_or(matches!(origin, WordSource::Text(node) if node.flags.line_continuation));
    if matches!(origin, WordSource::Text(node) if node.flags.delimiter_open) || continues_line {
        builder.tighten_next_boundary();
    }
    builder.continue_source_line(continues_line);
    !execution.definitive_reject && !builder.execution.wipe_remainder
}

/// Whether a leading motion or marker needs the live BACKBEFORE glyph.
/// HORIZ can consume it without writing a space, while a marker can
/// redirect its retreat to an authored blank (term.c:678–705, 901–908).
fn leading_event_needs_native_retreat(events: &[RoffInlineEvent], backafter: bool) -> bool {
    let mut seen_marker = false;
    for event in events {
        match event {
            RoffInlineEvent::LineBreak => seen_marker = true,
            RoffInlineEvent::HorizontalMotion { argument, .. }
                if !backafter
                    && !seen_marker
                    && negative_entry_columns(argument.as_deref()).is_some() =>
            {
                // HORIZ clears BACKBEFORE before any following marker.
                // Keep the old glyph's semantic owner until the checked
                // cursor plan proves whether a write actually replaces it.
                return false;
            }
            RoffInlineEvent::HorizontalMotion {
                reading_blank: true,
                argument,
            } if argument.as_deref().is_some_and(|argument| {
                !libmandoc_rs::TableColumnMetrics::horizontal_motion_has_input_after_backbefore(
                    argument,
                )
            }) =>
            {
                return true;
            }
            RoffInlineEvent::Text(value) => {
                for character in value.chars() {
                    if is_formatter_word_blank(character) {
                        return seen_marker;
                    }
                    if character != '\n' {
                        return false;
                    }
                }
            }
            RoffInlineEvent::Glyph(_)
            | RoffInlineEvent::BreakableHyphen
            | RoffInlineEvent::DeviceName
            | RoffInlineEvent::Overstrike { .. } => return false,
            // These use bufferc(ASCII_NBRZW), not encode1(), so they do
            // not consume BACKBEFORE before the later source blank
            // (term.c:610-638). Their graph role in term_fill is separate.
            _ => {}
        }
    }
    false
}

pub(super) fn negative_entry_columns(argument: Option<&str>) -> Option<usize> {
    let units = argument
        .and_then(crate::mandoc::layout::Distance::parse)?
        .basic_units();
    (units < 0).then(|| units.unsigned_abs() as usize / 24)
}
