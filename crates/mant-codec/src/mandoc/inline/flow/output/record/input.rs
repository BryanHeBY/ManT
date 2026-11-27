//! Native word input, rewind execution and field-row observations.

use std::{cell::RefCell, rc::Rc};

use super::super::super::definition::head_presentation::DeferredHeadProjection;
use super::super::super::field_buffer::{EntryRewind, FieldBuffer, FieldWrite, WordWriteReceipt};
use super::super::super::{HeadOperandCapture, NativeWordAnchor};
use super::super::source_blanks::{remove_rewound_glyph, remove_rewound_source_blanks};
use super::{
    Inline, PendingBoundary, SeparatorDisposition, WordPassProjection, WordProjectionPolicy,
};

mod live_prefix;

pub(super) struct ExecutedWordEntry {
    pub(super) receipt: WordWriteReceipt,
    rewind: EntryRewind,
    prefix: live_prefix::PrefixEdits,
    placed_entry_blanks: Option<usize>,
    pub(super) surviving_separator: usize,
    written_separator: usize,
}

impl ExecutedWordEntry {
    pub(super) fn execute(
        buffer: &mut FieldBuffer,
        mut writes: Vec<FieldWrite>,
        start: usize,
        separator: usize,
        earlier: &mut [NativeWordAnchor],
    ) -> Self {
        let prefix = live_prefix::PrefixEdits::prepare(buffer, &writes, earlier);
        let (placed_entry_blanks, placed_first) = match writes.first() {
            Some(FieldWrite::ReplacePlacedTail {
                cells,
                entry_blanks,
                first_incoming,
            }) => (
                *entry_blanks,
                first_incoming.map(|first| buffer.cells().len() - cells + first),
            ),
            _ => (None, None),
        };
        let rewind = buffer.rewind_word_entry(&mut writes, start, |cell| {
            earlier
                .get(earlier_word_after_cell(earlier, cell))
                .map_or(start, |next| next.start.min(next.content))
        });
        let mut receipt = buffer.apply_writes(&writes);
        buffer.hold_pending_glyph_passes(&writes);
        match rewind {
            EntryRewind::Positioned(retreat)
            | EntryRewind::Executed {
                cursor_retreat: retreat @ 1..,
                ..
            } => {
                buffer.retain_word_cursor(retreat);
            }
            _ => {}
        }
        if let Some(first) = prefix.first_incoming().or(placed_first) {
            receipt.first_content_cell = first;
        }
        let retired_separator = match rewind {
            EntryRewind::Executed {
                retired_separator, ..
            } => retired_separator,
            EntryRewind::Absent | EntryRewind::Positioned(_) | EntryRewind::Unmodeled => 0,
        };
        let surviving_separator = if prefix.first_incoming().is_some() {
            prefix.entry_blanks
        } else {
            placed_entry_blanks.unwrap_or_else(|| {
                buffer.surviving_word_separator(start, separator - retired_separator)
            })
        };
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
            prefix,
            placed_entry_blanks,
            surviving_separator,
            written_separator: separator,
        }
    }

    pub(super) fn transfer_positioning_slots(&self, buffer: &mut FieldBuffer) {
        self.prefix.transfer_positioning_slots(buffer);
    }

    pub(super) fn clipped_ink_owners<'a>(
        &'a self,
        earlier: &'a [NativeWordAnchor],
    ) -> impl Iterator<Item = &'a str> {
        let rewind = match &self.rewind {
            EntryRewind::Executed {
                overwritten_glyphs, ..
            } => overwritten_glyphs.as_slice(),
            _ => &[],
        };
        self.prefix
            .clipped_ink_owners()
            .chain(rewind.iter().filter_map(|glyph| {
                (!glyph.scalar.is_whitespace())
                    .then_some(earlier_word_after_cell(earlier, glyph.cell))
                    .and_then(|index| index.checked_sub(1))
            }))
            .map(|index| earlier[index].owner.as_str())
    }

    pub(super) fn project_rewind(
        &mut self,
        nodes: &mut [Inline],
        incoming: &mut Vec<Inline>,
        owner: &str,
        earlier: &[super::super::super::NativeWordAnchor],
        head: Option<&DeferredHeadProjection>,
        capture: Option<&Rc<RefCell<HeadOperandCapture>>>,
    ) {
        // Entry replay can retire an earlier glyph before term_fill sees
        // either owner. Use the same write map as the reading projection.
        if let Some(capture) = capture
            .cloned()
            .or_else(|| head.and_then(DeferredHeadProjection::operands))
        {
            capture
                .borrow_mut()
                .record_rewound_ink(self.clipped_ink_owners(earlier), owner);
        }
        self.prefix.project(nodes, incoming, owner, earlier, head);
        remove_rewound_source_blanks(incoming, owner, &self.receipt.consumed_motion_padding);
        let EntryRewind::Executed {
            ref traversed_source_blanks,
            ref overwritten_glyphs,
            ..
        } = self.rewind
        else {
            return;
        };
        remove_rewound_source_blanks(incoming, owner, traversed_source_blanks);
        for glyph in overwritten_glyphs {
            if let Some(index) = earlier_word_after_cell(earlier, glyph.cell).checked_sub(1) {
                remove_rewound_glyph(nodes, incoming, &earlier[index].owner, glyph);
            }
        }
    }

    pub(super) fn record_projection(&self, projection: &mut WordPassProjection) {
        projection.first_payload_cell = Some(self.receipt.first_content_cell);
        projection.automatic_separator_slot = self.prefix.entry_gap;
        projection.replayed_entry_gap = self
            .prefix
            .entry_gap
            .filter(|&gap| self.prefix.word_start.is_some_and(|start| start < gap));
        // Do not overwrite an already-consumed separator when replay has no
        // surviving entry gap (term.c:389-439, 901-908).
        if (self.prefix.first_incoming().is_some() || self.placed_entry_blanks.is_some())
            && !(projection.separator == SeparatorDisposition::Consumed
                && projection.replayed_entry_gap.is_none())
        {
            projection.native_separator = self.surviving_separator;
            projection.separator = if self.surviving_separator > 0 {
                SeparatorDisposition::Keep
            } else if self.written_separator > 0 {
                SeparatorDisposition::Consumed
            } else {
                SeparatorDisposition::Automatic
            };
        }
        if let EntryRewind::Executed {
            retired_separator, ..
        } = self.rewind
        {
            projection.separator = if projection.separator == SeparatorDisposition::Consumed
                || retired_separator > 0 && self.surviving_separator == 0
            {
                SeparatorDisposition::Consumed
            } else {
                SeparatorDisposition::Keep
            };
        }
    }

    pub(super) fn rewound(&self) -> bool {
        matches!(self.rewind, EntryRewind::Executed { .. })
    }

    pub(super) fn relocated_cells(&self) -> &[(usize, usize)] {
        &self.prefix.moved
    }

    pub(super) fn changed_owner_receipts_from(&self, initial: usize) -> Option<usize> {
        self.prefix.changed_owner_receipts_from(initial)
    }

    pub(super) fn append_owner_intervals(&self, anchors: &mut Vec<NativeWordAnchor>) {
        self.prefix.append_owner_intervals(anchors);
    }

    pub(super) fn word_start(&self, original: usize) -> usize {
        self.prefix.word_start.unwrap_or(original)
    }

    pub(super) fn has_replayed_slots(&self) -> bool {
        self.prefix.first_incoming().is_some() || self.placed_entry_blanks.is_some()
    }

    pub(super) fn incoming_ink(&self) -> Option<usize> {
        self.prefix
            .first_incoming_ink
            .or_else(|| self.rewound().then_some(self.receipt.first_content_cell))
    }

    pub(super) fn retains_blank(&self, cell: usize) -> bool {
        self.prefix.retains_blank(cell)
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
