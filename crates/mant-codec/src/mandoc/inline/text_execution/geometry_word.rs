//! Keep native edit operations before pass acceptance and reading projection.

use super::{Font, FontState, RoffInlineEvent, TextExecutionContext, WordExecution};
use crate::mandoc::inline::flow::{
    field_buffer::{FieldCell, FieldWrite, isolated_passes},
    native_slots::{FillSlot, NativeSlots, PriorSlotEdit},
};
use crate::mandoc::roff_escape::PresentationKind;

pub(super) fn applies(events: &[RoffInlineEvent], context: &TextExecutionContext<'_>) -> bool {
    if events.len() < 2
        || !context.policy.record_native_cells
        // The inherited BACK pair may own an earlier operand or its entry
        // blank. Leave that receipt with the live word executor.
        || context.native_field_has_input && context.zero_advance.is_armed()
        || context.zero_advance.has_pending_glyph()
    {
        return false;
    }
    let mut marker = false;
    let mut edits = false;
    let mut authored_blank = false;
    for event in events {
        match event {
            RoffInlineEvent::LineBreak => marker = true,
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } => {
                let relative = value.strip_prefix('|');
                edits |=
                    libmandoc_rs::TableColumnMetrics::motion_argument(relative.unwrap_or(value))
                        .is_some_and(|(units, _)| relative.is_some() || units < 0);
            }
            RoffInlineEvent::Overstrike { buffered, .. } => edits |= *buffered || authored_blank,
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                ..
            } => edits = true,
            RoffInlineEvent::Text(value) => authored_blank |= value.contains([' ', '\t']),
            RoffInlineEvent::Link(_) | RoffInlineEvent::EmptyDestination => return false,
            _ => {}
        }
    }
    marker && edits
}

#[derive(Clone, Copy)]
enum ReadingGlyph<'a> {
    Scalar(char),
    Spelling(&'a str),
}

impl ReadingGlyph<'_> {
    fn append(self, text: &mut String) {
        match self {
            Self::Scalar(value) => text.push(value),
            Self::Spelling(value) => text.push_str(value),
        }
    }

    fn scalars(self) -> usize {
        match self {
            Self::Scalar(_) => 1,
            Self::Spelling(text) => text.chars().count(),
        }
    }
}

#[derive(Clone, Copy)]
struct ReadingCell<'a> {
    glyph: ReadingGlyph<'a>,
    font: Font,
    authored_separator: bool,
    preserve_separator: bool,
}

struct Placement<'a> {
    slots: NativeSlots,
    reading: Vec<ReadingCell<'a>>,
    marker_boundary: MarkerBoundary,
}

#[derive(Clone, Copy)]
enum MarkerBoundary {
    Clear,
    Pending,
    AuthoredBlank,
}

impl<'a> Placement<'a> {
    fn mark(&mut self, glyph: ReadingGlyph<'a>, font: Font) -> usize {
        let owner = self.reading.len();
        self.reading.push(ReadingCell {
            glyph,
            font,
            authored_separator: false,
            preserve_separator: false,
        });
        owner
    }

    fn input(
        &mut self,
        event: &'a RoffInlineEvent,
        state: &mut FontState,
        line_width_units: usize,
    ) {
        use crate::mandoc::roff_escape::RoffFont;
        let font = state.current;
        let display = state.display_current();
        match event {
            RoffInlineEvent::Font(next) => state.select(*next),
            RoffInlineEvent::PreviousFont => state.restore(),
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value) => {
                self.text(value, display, font);
            }
            RoffInlineEvent::Overstrike {
                source, terminal, ..
            } => {
                let owner = terminal
                    .as_deref()
                    .map(|glyph| self.mark(ReadingGlyph::Spelling(glyph), display));
                self.slots.overstrike(source, font, owner);
            }
            RoffInlineEvent::HorizontalMotion { argument, .. } => {
                self.slots.motion(argument.as_deref());
            }
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                argument: Some(argument),
            } => self.slots.line(argument, line_width_units, font),
            RoffInlineEvent::BreakableHyphen => self.place(FieldCell::Hyphen, '-', display, font),
            RoffInlineEvent::NonBreakingWordBlank => {
                self.place(FieldCell::NonBreakingBlank, ' ', display, font);
            }
            RoffInlineEvent::DeviceName => {
                for character in ['u', 't', 'f', '8'] {
                    self.place(
                        FieldCell::Graph {
                            text: character,
                            width: 1,
                        },
                        character,
                        display,
                        font,
                    );
                }
            }
            RoffInlineEvent::FallbackGlyph(value) => {
                let owner = (!self.slots.backafter())
                    .then(|| self.mark(ReadingGlyph::Spelling(value), display));
                self.slots.native_write(
                    FieldWrite::Cell(FieldCell::ZeroWidthGraph),
                    RoffFont::Regular,
                    owner,
                );
            }
            _ => {
                let cell = match event {
                    RoffInlineEvent::LineBreak => {
                        self.marker_boundary = MarkerBoundary::Pending;
                        FieldWrite::Cell(FieldCell::BreakMarker)
                    }
                    RoffInlineEvent::ZeroWidthGlyph => FieldWrite::Cell(FieldCell::ZeroWidthGraph),
                    RoffInlineEvent::Breakpoint => FieldWrite::Cell(FieldCell::Breakpoint),
                    RoffInlineEvent::ZeroAdvance => FieldWrite::ArmBackafter,
                    RoffInlineEvent::NoSpace => FieldWrite::CancelBackafter,
                    _ => return,
                };
                self.slots.native_write(cell, font, None);
            }
        }
    }

    fn place(
        &mut self,
        cell: FieldCell,
        character: char,
        display: Font,
        font: crate::mandoc::roff_escape::RoffFont,
    ) {
        let owner = self.mark(ReadingGlyph::Scalar(character), display);
        self.slots
            .native_write(FieldWrite::Cell(cell), font, Some(owner));
    }

    fn text(&mut self, value: &str, display: Font, font: crate::mandoc::roff_escape::RoffFont) {
        for character in value.chars() {
            let owner = self.mark(ReadingGlyph::Scalar(character), display);
            if character == ' ' {
                if matches!(self.marker_boundary, MarkerBoundary::Pending) {
                    self.marker_boundary = MarkerBoundary::AuthoredBlank;
                }
            } else {
                self.reading[owner].authored_separator =
                    matches!(self.marker_boundary, MarkerBoundary::AuthoredBlank);
                self.marker_boundary = MarkerBoundary::Clear;
            }
            self.slots
                .native_write(FieldWrite::literal_cell(character), font, Some(owner));
        }
    }
}

pub(super) fn try_execute(
    events: &[RoffInlineEvent],
    context: &mut TextExecutionContext<'_>,
    word: &mut WordExecution,
) -> Result<(), PriorSlotEdit> {
    let mut placement = Placement {
        slots: NativeSlots::after_entry(context.native_field_has_input),
        reading: Vec::new(),
        marker_boundary: MarkerBoundary::Clear,
    };
    // Only publish state once all edits stay within this word's bytes.
    // A rewind into the earlier field must run from the unmodified state.
    let mut font = context.font.clone();
    let mut continuation = None;
    if context.zero_advance.is_armed() {
        placement.slots.arm_backafter();
    }
    for (index, event) in events.iter().enumerate() {
        if matches!(event, RoffInlineEvent::NoSpace) && index + 1 == events.len() {
            continuation = Some(!placement.slots.backafter());
        }
        placement.input(event, &mut font, context.line_width_units);
        if let Some(edit) = placement.slots.prior_edit() {
            return Err(edit);
        }
    }
    let armed = placement.slots.backafter();
    *context.font = font;
    context.zero_advance.take_armed();
    context.zero_advance.inherit_armed(armed);
    word.source_continuation = continuation;
    let mut slots = placement.slots.into_fill_slots();
    retire_overstruck_chars(&mut slots);
    preserve_author_separators(&slots, &mut placement.reading);
    word.table_device_buffer_written = !slots.is_empty();
    word.native_writes = slots
        .iter()
        .map(|slot| slot_write(slot, &placement.reading))
        .collect();
    if context.policy.field_authoritative {
        append_range(word, &slots, &placement.reading);
    } else {
        for (range, closes) in isolated_passes(&word.native_writes, context.table_field_units) {
            append_range(word, &slots[range], &placement.reading);
            if closes {
                word.flush_segment();
                word.output.push(mant_ir::Inline::line_break());
            }
        }
    }
    word.flush_segment();
    if armed {
        word.native_writes.push(FieldWrite::ArmBackafter);
    }
    Ok(())
}

fn retire_overstruck_chars(slots: &mut [FillSlot]) {
    // Decoration prefixes have no source owner. A native backing up
    // followed by an advancing graph covers the preceding source cell.
    for index in 1..slots.len().saturating_sub(1) {
        if slots[index].cell == FieldCell::Backline
            && matches!(
                slots[index + 1].cell,
                FieldCell::Graph { width: 1.., .. } | FieldCell::Hyphen
            )
        {
            slots[index - 1].owner = None;
        }
    }
}

fn slot_write(slot: &FillSlot, reading: &[ReadingCell<'_>]) -> FieldWrite {
    let count = slot.count;
    let scalars = slot.owner.map_or(0, |id| {
        reading[id].glyph.scalars() + usize::from(reading[id].preserve_separator)
    });
    let cell = match slot.cell {
        FieldCell::BreakableBlank if scalars == 0 => FieldCell::DrawnSpaces(count),
        FieldCell::NonBreakingBlank if scalars == 0 => FieldCell::FixedDrawingAdvance(count),
        FieldCell::Graph { text, width } => FieldCell::Graph {
            text,
            width: width.saturating_mul(count),
        },
        _ => slot.cell.clone(),
    };
    FieldWrite::Placed { cell, scalars }
}

fn preserve_author_separators(slots: &[FillSlot], reading: &mut [ReadingCell<'_>]) {
    // An authored blank after a marker can traverse an older glyph or
    // overstrike byte without writing native SP (term.c::bufferc). Keep
    // its reading boundary when that pass still contains both words.
    let mut after_author = false;
    let mut blank_between = false;
    let mut break_at_next_blank = false;
    for slot in slots {
        match slot.cell {
            FieldCell::BreakMarker => break_at_next_blank = true,
            FieldCell::BreakableBlank | FieldCell::Breakpoint if break_at_next_blank => {
                // term_fill() ends the current pass here, even when its
                // preceding graph or separator supplies no reading text.
                after_author = false;
                blank_between = false;
                break_at_next_blank = false;
            }
            FieldCell::BreakableBlank | FieldCell::NonBreakingBlank | FieldCell::Tab => {
                if slot.owner.is_some() {
                    after_author = false;
                    blank_between = false;
                } else {
                    // The retained device advance is omitted from the
                    // reading view. Preserve the boundary it establishes.
                    blank_between = true;
                }
            }
            _ => {
                if let Some(id) = slot.owner {
                    reading[id].preserve_separator =
                        after_author && (reading[id].authored_separator || blank_between);
                    after_author = true;
                    blank_between = false;
                }
            }
        }
    }
}

fn append_range(word: &mut WordExecution, slots: &[FillSlot], reading: &[ReadingCell<'_>]) {
    let mut wrote_source = false;
    for slot in slots {
        let Some(id) = slot.owner else { continue };
        let current = reading[id];
        if current.font != word.font {
            word.flush_segment();
            word.font = current.font;
        }
        if current.preserve_separator && wrote_source {
            word.buffer.push(' ');
        }
        current.glyph.append(&mut word.buffer);
        wrote_source = true;
    }
}
