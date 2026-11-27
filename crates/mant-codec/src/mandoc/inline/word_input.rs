//! Execution and projection of an input formatter word.
use super::flow::native_slots::{EntryCursor, PrefixExtension, ReplayDomain};
use libmandoc_rs::Node;
use mant_ir::Inline;

use super::{InlineBuilder, flow::WordProjectionPolicy, font};
use crate::mandoc::roff_escape::{RoffInlineEvent, decode_with_status, is_formatter_word_blank};

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
    let decoded = decode_with_status(source);
    let events = &decoded.events;
    // `term_word()` consumes its inter-word boundary even for an explicit
    // empty operand. That word event can resolve a preceding `\\z` glyph
    // before generated enclosure punctuation is emitted. Control *nodes* are
    // routed separately and therefore do not gain this behavior.
    let has_glyph = events.iter().any(|event| match event {
        RoffInlineEvent::Text(value)
        | RoffInlineEvent::Glyph(value, _)
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
        leading_event_needs_native_retreat(events, builder.execution.zero_advance.is_armed()),
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
    if let WordSource::Text(node) = origin {
        builder.record_current_native_operand(Some((node, &decoded)));
    }
    let recognize_generated_references = match origin {
        WordSource::Text(node) => !node.flags.no_fill,
        WordSource::NormalizedOperand => !builder.execution.no_fill_word_active,
        WordSource::EquationOperand => false,
    };
    let execution = execute_roff_word_entry(
        builder,
        source,
        events,
        origin,
        font::TextExecutionPolicy {
            recognize_generated_references,
            record_native_cells: true,
            field_authoritative,
            word_space,
        },
        defer_leading_blanks,
    );
    receive_word_effects(builder, &execution);
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

fn execute_roff_word_entry(
    builder: &mut InlineBuilder,
    source: &str,
    events: &[RoffInlineEvent],
    origin: WordSource<'_>,
    policy: font::TextExecutionPolicy,
    defer_leading_blanks: bool,
) -> super::text_execution::TextExecution {
    let line_width_units = builder.execution.horizontal_line_width_units(source);
    let slot_entry = builder.execution.native_slot_entry_for_word(events);
    let producer_proof = run_in_head_entry_proof(builder, &slot_entry, origin);
    let mut execution = font::parse_roff_text_with_zero_advance(
        source,
        font::TextExecutionContext {
            font: &mut builder.execution.font,
            zero_advance: &mut builder.execution.zero_advance,
            defer_leading_blanks,
            line_width_units,
            slot_entry,
            table_field_units: None,
            table_leading_blanks: 0,
            policy,
        },
    );
    if let Some(font) = producer_proof {
        super::text_execution::retain_pending_run_in_head(
            events,
            &mut execution.native_writes,
            font,
        );
    }
    execution
}

fn run_in_head_entry_proof(
    builder: &InlineBuilder,
    slot: &super::SlotEntry,
    origin: WordSource<'_>,
) -> Option<super::Font> {
    let head = builder.run_in_head_word_proof?;
    let domain = match head {
        super::RunInHeadWordProof::Inset => ReplayDomain::DefinitionLine,
        // Diagnostic HEAD keeps its own native field flags until It post.
        super::RunInHeadWordProof::Diagonal => ReplayDomain::Scoped,
    };
    (matches!(origin, WordSource::Text(node) if !node.flags.no_fill && !node.flags.generated)
        && slot.domain == domain
        && slot.cursor == EntryCursor::Empty
        && slot.automatic_blanks == 0
        && slot.device_tail.is_none()
        && !builder.zero_advance.is_armed()
        && !builder.zero_advance.has_buffered_glyph()
        && !builder.zero_advance.has_annotation_output_owner())
    .then_some(builder.execution.font.current)
}

fn receive_word_effects(
    builder: &mut InlineBuilder,
    execution: &super::text_execution::TextExecution,
) {
    builder.ensure_definition_field_session();
    builder
        .execution
        .apply_native_origin_retreat(execution.native_origin_retreat_units);
    builder.escape_coverage.record(execution.escape_scan);
    if let Some(cell) = &mut builder.table_cell {
        cell.buffered |= execution.table_device_buffer_written;
    }
}

impl super::InlineExecutionState {
    fn native_slot_entry_for_word(&self, events: &[RoffInlineEvent]) -> super::SlotEntry {
        let pending = self.zero_advance.has_pending_glyph();
        let capture_prefix = events.iter().any(|event| match event {
            RoffInlineEvent::HorizontalMotion {
                argument: Some(argument),
                ..
            } => argument.starts_with('|') || negative_entry_columns(Some(argument)).is_some(),
            RoffInlineEvent::Overstrike { .. }
            | RoffInlineEvent::Presentation {
                kind: crate::mandoc::roff_escape::PresentationKind::HorizontalLine,
                ..
            } => true,
            _ => false,
        });
        // Any non-canceled relative negative or absolute HORIZ can reposition
        // p->col inside the live field suffix (term.c:678-682, 683-718).
        // Positive non-canceled relative HORIZ extends a filled prefix.
        let mut armed = self.zero_advance.is_armed();
        let mut has_cursor_reposition = false;
        let mut has_positive_motion = false;
        for event in events {
            match event {
                RoffInlineEvent::ZeroAdvance => armed = true,
                RoffInlineEvent::NoSpace
                | RoffInlineEvent::BreakableHyphen
                | RoffInlineEvent::DeviceName
                | RoffInlineEvent::Overstrike { .. } => armed = false,
                RoffInlineEvent::HorizontalMotion { argument, .. } => {
                    if std::mem::take(&mut armed) {
                        continue;
                    }
                    if argument.as_deref().is_some_and(|arg| arg.starts_with('|'))
                        || negative_entry_columns(argument.as_deref()).is_some()
                    {
                        has_cursor_reposition = true;
                    } else if let Some(argument) = argument.as_deref()
                        && !argument.starts_with('|')
                        && libmandoc_rs::TableColumnMetrics::relative_motion_columns(argument) > 0
                    {
                        has_positive_motion = true;
                    }
                }
                RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _)
                    if value
                        .chars()
                        .any(|ch| !is_formatter_word_blank(ch) && ch != '\n') =>
                {
                    armed = false;
                }
                _ => {}
            }
        }
        let leading_marker = matches!(
            events.iter().find(|event| !matches!(
                event,
                RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont | RoffInlineEvent::Link(_)
            )),
            Some(RoffInlineEvent::LineBreak)
        );
        // The annotation escape writes a zero-width native slot, while its
        // styled label can expand into multiple raw bytes (term.c:620-638,
        // 911-925). Import the proved prefix before placing the label so
        // a later absolute HORIZ can still address the earlier word.
        let annotated = events
            .iter()
            .any(|event| matches!(event, RoffInlineEvent::Link(Some(_))));
        let extend = if leading_marker {
            PrefixExtension::LeadingMarker
        } else if events.iter().any(|event| {
            matches!(
                event,
                RoffInlineEvent::LineBreak | RoffInlineEvent::ZeroAdvance
            )
        }) {
            PrefixExtension::Word
        } else if has_positive_motion || annotated {
            PrefixExtension::FilledWord
        } else {
            PrefixExtension::None
        };
        self.native_slot_entry(capture_prefix || pending, extend, has_cursor_reposition)
    }

    pub(in crate::mandoc) fn horizontal_line_width_units(&self, source: &str) -> usize {
        if !source.contains("\\l") {
            return 0;
        }
        if let Some(width) = self.table_cell.and_then(|cell| cell.line_width_units) {
            return width;
        }
        let (offset, margin) = self.table_source_bounds(0);
        margin.saturating_sub(offset)
    }
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
            RoffInlineEvent::Glyph(..)
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
