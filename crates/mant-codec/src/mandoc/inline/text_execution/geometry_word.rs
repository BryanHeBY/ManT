//! Keep native edit operations before pass acceptance and reading projection.

use super::{Font, FontState, RoffInlineEvent, TextExecutionContext, WordExecution};
use crate::mandoc::inline::flow::{
    field_buffer::{FieldCell, FieldWrite},
    native_slots::{NativeSlots, PriorSlotEdit},
};
use crate::mandoc::roff_escape::{PresentationKind, RoffFont};

mod projection;

use projection::PlacedWord;

pub(super) fn absolute_motion(event: &RoffInlineEvent) -> bool {
    matches!(event, RoffInlineEvent::HorizontalMotion { argument: Some(value), .. }
        if value.strip_prefix('|').and_then(libmandoc_rs::TableColumnMetrics::motion_argument).is_some())
}

pub(super) fn applies(events: &[RoffInlineEvent], context: &TextExecutionContext<'_>) -> bool {
    if !context.policy.record_native_cells
        // The inherited BACK pair may own an earlier operand or its entry
        // blank. Leave that receipt with the live word executor.
        || context.slot_entry.prior_input && context.zero_advance.is_armed()
            && context.zero_advance.has_buffered_glyph()
        || context.zero_advance.has_pending_glyph()
            && !(context.zero_advance.can_project_prior_slot()
                && context.slot_entry.prefix.as_ref().is_some_and(|prefix| prefix.pending_cell.is_some()))
    {
        return false;
    }
    let mut marker = false;
    let mut edits = false;
    let mut authored_blank = false;
    let mut draws = false;
    let mut moves_toward_prefix = false;
    let mut armed_overstrike = false;
    let mut armed_empty_overstrike = false;
    let mut armed = context.zero_advance.is_armed();
    for event in events {
        match event {
            RoffInlineEvent::LineBreak => marker = true,
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } => {
                let relative = value.strip_prefix('|');
                moves_toward_prefix |=
                    libmandoc_rs::TableColumnMetrics::motion_argument(relative.unwrap_or(value))
                        .is_some_and(|(units, _)| relative.is_some() || units < 0);
            }
            RoffInlineEvent::Overstrike {
                buffered, terminal, ..
            } => {
                edits |= *buffered || authored_blank;
                armed_overstrike |= armed && *buffered && terminal.is_none();
                armed_empty_overstrike |= armed && !buffered;
            }
            RoffInlineEvent::ZeroAdvance => armed = true,
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                ..
            } => {
                edits = true;
                draws = true;
            }
            RoffInlineEvent::Text(value) => authored_blank |= value.contains([' ', '\t']),
            _ => {}
        }
    }
    let replay_empty_overstrike =
        armed_empty_overstrike && context.slot_entry.trailing_tab_reference;
    if context.slot_entry.prior_input
        && context.zero_advance.is_armed()
        && context.slot_entry.prefix.is_none()
        // A blank overstrike may trim two slots. The native entry blank
        // and the overstrike's own write bound this edit to the new word.
        // An empty overstrike needs a proved trailing source-row reference.
        // Replay later motion in source order. If it needs earlier bytes,
        // the draft returns an unresolved edit before changing any owner.
        && !((armed_overstrike || replay_empty_overstrike)
            && context.slot_entry.automatic_blanks > 0)
    {
        return false;
    }
    edits |= moves_toward_prefix;
    marker && (edits || context.table_leading_blanks != 0)
        || context.policy.field_authoritative
            && (draws
                || armed_overstrike
                || replay_empty_overstrike
                || context
                    .slot_entry
                    .device_tail
                    .as_ref()
                    .is_some_and(|tail| edits || tail.has_retreated() && !events.is_empty()))
}

#[derive(Clone, Copy)]
enum ReadingGlyph<'a> {
    Scalar(char),
    Spelling(&'a str),
    Earlier(usize),
    EntryBlank,
}

impl ReadingGlyph<'_> {
    fn append(self, text: &mut String) {
        match self {
            Self::Scalar(value) => text.push(value),
            Self::Spelling(value) => text.push_str(value),
            Self::Earlier(_) | Self::EntryBlank => {}
        }
    }

    fn scalars(self) -> usize {
        match self {
            Self::Scalar(_) | Self::EntryBlank => 1,
            Self::Spelling(text) => text.chars().count(),
            Self::Earlier(count) => count,
        }
    }
}

#[derive(Clone, Copy)]
struct ReadingCell<'a> {
    glyph: ReadingGlyph<'a>,
    font: Font,
    link: Option<usize>,
    sphinx_marker: bool,
    authored_separator: bool,
    preserve_separator: bool,
}

struct Placement<'a> {
    slots: NativeSlots,
    reading: Vec<ReadingCell<'a>>,
    marker_boundary: MarkerBoundary,
    links: Vec<&'a str>,
    active_link: Option<usize>,
}

#[derive(Clone, Copy)]
enum MarkerBoundary {
    Clear,
    Pending,
    AuthoredBlank,
}

impl<'a> Placement<'a> {
    fn empty(slots: NativeSlots) -> Self {
        Self {
            slots,
            reading: Vec::new(),
            marker_boundary: MarkerBoundary::Clear,
            links: Vec::new(),
            active_link: None,
        }
    }

    fn mark(&mut self, glyph: ReadingGlyph<'a>, font: Font) -> usize {
        let owner = self.reading.len();
        self.reading.push(ReadingCell {
            glyph,
            font,
            link: self.active_link,
            sphinx_marker: false,
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
        let font = state.current;
        let display = state.display_current();
        match event {
            RoffInlineEvent::Font(next) => state.select(*next),
            RoffInlineEvent::PreviousFont => state.restore(),
            RoffInlineEvent::Link(target) => {
                self.active_link = target.as_deref().map(|target| {
                    let id = self.links.len();
                    self.links.push(target);
                    id
                });
                // The decoder's neighboring NBRZW event accounts for X's
                // ESCAPE_IGNORE cell (roff_escape.c and term_word()).
            }
            RoffInlineEvent::EmptyDestination => {
                let eligible = !self.slots.backafter() && self.slots.pending_owner().is_none();
                self.write_invisible_cell();
                self.text("<>", display, font);
                self.reading
                    .last_mut()
                    .expect("Sphinx marker end")
                    .sphinx_marker = eligible;
            }
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

    fn write_invisible_cell(&mut self) {
        self.slots.native_write(
            FieldWrite::Cell(FieldCell::ZeroWidthGraph),
            RoffFont::Regular,
            None,
        );
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

struct GeometrySeed<'a> {
    placement: Placement<'a>,
    replace_tail: usize,
    replayed_entry: bool,
    earlier: usize,
    unpublished: Option<usize>,
}

impl GeometrySeed<'_> {
    fn begin(context: &mut TextExecutionContext<'_>) -> Self {
        let mut seed = if let Some(prefix) = context
            .slot_entry
            .prefix
            .take()
            .filter(|_| context.slot_entry.device_tail.is_none())
        {
            let mut placement = Placement::empty(NativeSlots::with_field_prefix(&prefix));
            let earlier = prefix.cells.len();
            for (_, scalars) in prefix.cells {
                placement.mark(ReadingGlyph::Earlier(scalars), Font::Regular);
            }
            for _ in 0..prefix.automatic {
                placement.mark(ReadingGlyph::EntryBlank, Font::Regular);
            }
            Self {
                placement,
                earlier,
                unpublished: context
                    .zero_advance
                    .has_pending_glyph()
                    .then_some(prefix.pending_cell)
                    .flatten(),
                replace_tail: 0,
                replayed_entry: false,
            }
        } else if let Some(tail) = context.slot_entry.device_tail.take() {
            let replace_tail = tail.fill_cells;
            Self {
                placement: Placement::empty(tail.into_slots()),
                replace_tail,
                replayed_entry: false,
                earlier: 0,
                unpublished: None,
            }
        } else {
            let mut placement =
                Placement::empty(NativeSlots::after_entry(context.slot_entry.prior_input));
            let mut replace_tail = context.slot_entry.retreat_blanks;
            let replayed_entry = replace_tail == 0
                && context.slot_entry.prior_input
                && context.zero_advance.is_armed();
            if replace_tail > 0 {
                placement.slots.enter_with_retreat(replace_tail);
            } else if replayed_entry {
                replace_tail = context.slot_entry.automatic_blanks;
                let blank = if context.slot_entry.kept_blanks {
                    FieldCell::NonBreakingBlank
                } else {
                    FieldCell::BreakableBlank
                };
                if context.slot_entry.trailing_tab_reference {
                    placement.slots.native_write(
                        FieldWrite::Cell(FieldCell::TabReference),
                        RoffFont::Regular,
                        None,
                    );
                    replace_tail += 1;
                }
                for _ in 0..context.slot_entry.automatic_blanks {
                    let owner = placement.mark(ReadingGlyph::EntryBlank, Font::Regular);
                    placement.slots.native_write(
                        FieldWrite::Cell(blank.clone()),
                        RoffFont::Regular,
                        Some(owner),
                    );
                }
            }
            Self {
                placement,
                replace_tail,
                replayed_entry,
                earlier: 0,
                unpublished: None,
            }
        };
        seed.placement
            .slots
            .prepend_fixed_blanks(context.table_leading_blanks);
        seed
    }
}

pub(super) fn try_execute(
    events: &[RoffInlineEvent],
    context: &mut TextExecutionContext<'_>,
    word: &mut WordExecution,
) -> Result<(), PriorSlotEdit> {
    let seed = GeometrySeed::begin(context);
    execute_seed(seed, events, context, word)
}

fn execute_seed<'a>(
    mut seed: GeometrySeed<'a>,
    events: &'a [RoffInlineEvent],
    context: &mut TextExecutionContext<'_>,
    word: &mut WordExecution,
) -> Result<(), PriorSlotEdit> {
    // Font and reading owners only commit after all addressed bytes can be
    // placed. Falling back to the live word executor must not run them twice.
    let mut font = context.font.clone();
    let mut continuation = None;
    if context.zero_advance.is_armed() {
        seed.placement.slots.arm_backafter();
    }
    for (index, event) in events.iter().enumerate() {
        if matches!(event, RoffInlineEvent::NoSpace) && index + 1 == events.len() {
            continuation = Some(!seed.placement.slots.backafter());
        }
        seed.placement
            .input(event, &mut font, context.line_width_units);
        if let Some(edit) = seed.placement.slots.prior_edit() {
            return Err(edit);
        }
    }
    let retain_suffix = context.policy.field_authoritative
        && !events
            .iter()
            .any(|event| matches!(event, RoffInlineEvent::LineBreak));
    let placed = PlacedWord::prepare(seed, retain_suffix)?;
    *context.font = font;
    context.zero_advance.take_armed();
    word.source_continuation = continuation;
    placed.project(context, word);
    Ok(())
}
