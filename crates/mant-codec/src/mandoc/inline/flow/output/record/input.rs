//! Native word input, rewind execution and field-row observations.

use super::super::super::field_buffer::{EntryRewind, FieldBuffer, FieldWrite, WordWriteReceipt};
use super::super::source_blanks::{remove_rewound_glyph, remove_rewound_source_blanks};
use super::{Inline, PendingBoundary, WordPassProjection, WordProjectionPolicy};

pub(super) struct ExecutedWordEntry {
    pub(super) receipt: WordWriteReceipt,
    rewind: EntryRewind,
    pub(super) surviving_separator: usize,
}

impl ExecutedWordEntry {
    pub(super) fn execute(
        buffer: &mut FieldBuffer,
        mut writes: Vec<FieldWrite>,
        start: usize,
        separator: usize,
        earlier: &[super::super::super::NativeWordAnchor],
    ) -> Self {
        let rewind = buffer.rewind_word_entry(&mut writes, start, |cell| {
            earlier
                .get(earlier_word_after_cell(earlier, cell))
                .map_or(start, |next| next.start.min(next.content))
        });
        let mut receipt = buffer.apply_writes(&writes);
        let retired_separator = match rewind {
            EntryRewind::Executed {
                retired_separator, ..
            } => retired_separator,
            EntryRewind::Absent | EntryRewind::Unmodeled => 0,
        };
        let surviving_separator =
            buffer.surviving_word_separator(start, separator - retired_separator);
        if matches!(rewind, EntryRewind::Executed { .. })
            && surviving_separator > 0
            && receipt.first_content_cell < start
            && buffer.projection_length(receipt.first_content_cell, start) == 0
        {
            // A rewound control can precede the surviving entry blank.
            // It still participates in fill, but it does not consume any
            // scalar of the incoming word before that blank prints.
            receipt.first_content_cell = start + surviving_separator;
        }
        Self {
            receipt,
            rewind,
            surviving_separator,
        }
    }

    pub(super) fn project_rewind(
        &self,
        nodes: &mut [Inline],
        incoming: &mut [Inline],
        owner: &str,
        earlier: &[super::super::super::NativeWordAnchor],
    ) {
        let EntryRewind::Executed {
            ref traversed_source_blanks,
            ref overwritten_glyph,
            ..
        } = self.rewind
        else {
            return;
        };
        remove_rewound_source_blanks(incoming, owner, traversed_source_blanks);
        if let Some(glyph) = overwritten_glyph
            && let Some(index) = earlier_word_after_cell(earlier, glyph.cell).checked_sub(1)
        {
            remove_rewound_glyph(nodes, incoming, &earlier[index].owner, glyph);
        }
    }

    pub(super) fn record_projection(&self, projection: &mut WordPassProjection) {
        if let EntryRewind::Executed {
            retired_separator, ..
        } = self.rewind
        {
            projection.preserve_native_separator = true;
            projection.consumed_initial_separator |=
                retired_separator > 0 && self.surviving_separator == 0;
        }
    }

    pub(super) fn rewound(&self) -> bool {
        matches!(self.rewind, EntryRewind::Executed { .. })
    }
}

fn earlier_word_after_cell(
    earlier: &[super::super::super::NativeWordAnchor],
    cell: usize,
) -> usize {
    earlier.partition_point(|anchor| anchor.content <= cell)
}

pub(super) struct RecordedWordInput {
    pub(super) native_writes: Option<Vec<FieldWrite>>,
    pub(super) native_boundary: PendingBoundary,
    pub(super) sentence: bool,
    pub(super) projected_keep: bool,
    pub(super) projection: WordProjectionPolicy,
}

impl RecordedWordInput {
    pub(super) fn take(
        execution: &mut super::super::super::InlineExecutionState,
        boundary: PendingBoundary,
        projection: WordProjectionPolicy,
    ) -> Self {
        let native_writes = execution.native_word_writes.take();
        let native_boundary = execution.native_word_boundary.take().unwrap_or(boundary);
        execution.observe_table_word_writes(
            native_writes.as_deref().unwrap_or_default(),
            !native_boundary.is_native_tight(),
        );
        Self {
            native_writes,
            native_boundary,
            sentence: std::mem::take(&mut execution.native_word_sentence),
            projected_keep: std::mem::take(&mut execution.projected_keep_separator),
            projection,
        }
    }
}

pub(super) fn observe_definition_word(
    definition: &mut super::super::super::definition::DefinitionFieldState,
    incoming: &[Inline],
    separator: usize,
) {
    let row = &mut definition.hang_row;
    let projected = super::super::super::super::plain_text(incoming);
    let trimmed = projected.trim_end_matches(' ');
    let trailing_spaces = projected.len().saturating_sub(trimmed.len());
    let width = mant_ir::geometry::text_width(trimmed);
    row.field_breakable |= trimmed.contains(' ');
    row.field_unproven_break |= trimmed.contains('\n');
    // term_fill() discards a field containing only ordinary breakable
    // blanks. A nonbreaking blank from \~ or \0 is a printable cell.
    let printable = trimmed
        .chars()
        .any(|ch| !super::super::super::super::is_formatter_word_blank(ch) && ch != '\n');
    row.word(separator, width, trailing_spaces, printable);
    if printable {
        row.field_last_unbreakable_width = trimmed
            .rsplit([' ', '\n'])
            .find(|part| !part.is_empty())
            .map_or(0, mant_ir::geometry::text_width);
    }
}
