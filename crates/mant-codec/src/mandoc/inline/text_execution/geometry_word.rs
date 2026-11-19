//! Keep native edit operations before pass acceptance and reading projection.

use super::{Font, FontState, RoffInlineEvent, TextExecutionContext, WordExecution};
use crate::mandoc::inline::flow::{
    field_buffer::{FieldCell, FieldWrite},
    native_slots::{NativeSlots, PriorSlotEdit, ReplayDomain},
};
use crate::mandoc::roff_escape::{PresentationKind, RoffFont};

mod overprint;
mod projection;

use projection::PlacedWord;

pub(super) fn absolute_motion(event: &RoffInlineEvent) -> bool {
    matches!(event, RoffInlineEvent::HorizontalMotion { argument: Some(value), .. }
        if value.strip_prefix('|').and_then(libmandoc_rs::TableColumnMetrics::motion_argument).is_some())
}

pub(super) fn applies(events: &[RoffInlineEvent], context: &TextExecutionContext<'_>) -> bool {
    if !can_place_word(context) {
        return false;
    }
    let field = context.policy.field_authoritative;
    let mut marker = false;
    let mut edits = false;
    let mut authored_blank = false;
    let mut draws = false;
    let mut overstrike_edit = false;
    let mut absolute_edit = false;
    let mut relative_retreat = false;
    let mut relative_advance = false;
    let mut armed_overstrike = false;
    let mut armed_empty_overstrike = false;
    let mut has_zero_advance = false;
    let mut isolated_needs_accepted_retreat = false;
    let mut has_overstrike = false;
    let mut has_link = false;
    let mut armed = context.zero_advance.is_armed();
    for event in events {
        match event {
            RoffInlineEvent::LineBreak => marker = true,
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } => {
                if let Some((absolute, units)) = motion_columns(value) {
                    absolute_edit |= absolute;
                    relative_retreat |= !absolute && units < 0;
                    relative_advance |= !absolute && units > 0;
                }
            }
            RoffInlineEvent::Overstrike {
                buffered, terminal, ..
            } => {
                // Even an empty overstrike can trim the preceding native
                // slots. Raw tbl cells have a complete local word buffer.
                has_overstrike = true;
                edits |= *buffered || authored_blank;
                overstrike_edit |= *buffered
                    || terminal.is_none()
                        && (context.slot_entry.prefix.is_some()
                            || context.slot_entry.trailing_tab_reference
                                && context.slot_entry.automatic_blanks > 0);
                armed_overstrike |= armed && *buffered && terminal.is_none();
                armed_empty_overstrike |= armed && !buffered;
            }
            RoffInlineEvent::ZeroAdvance => {
                armed = true;
                has_zero_advance = true;
            }
            RoffInlineEvent::Link(_) => has_link = true,
            RoffInlineEvent::Glyph(value) if !field => {
                isolated_needs_accepted_retreat |=
                    !value.is_empty() && mant_ir::geometry::text_width(value) == 0;
            }
            RoffInlineEvent::Breakpoint if !field => {
                isolated_needs_accepted_retreat = true;
            }
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                ..
            } => {
                edits = true;
                draws = true;
            }
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value) => {
                authored_blank |= value.contains([' ', '\t']);
            }
            _ => {}
        }
    }
    let replay_empty_overstrike =
        armed_empty_overstrike && context.slot_entry.trailing_tab_reference;
    let has_armed_overlap = armed_overstrike || replay_empty_overstrike;
    if has_unproved_armed_entry(context)
        && !(has_armed_overlap && context.slot_entry.automatic_blanks > 0)
    {
        return false;
    }
    edits |= absolute_edit || relative_retreat;
    !field
        && (relative_retreat
            || absolute_edit
            || has_overstrike
            || marker && relative_advance
            || has_zero_advance && isolated_needs_accepted_retreat)
        || marker && (edits || context.table_leading_blanks != 0)
        || field
            && (has_live_raw_cursor(context)
                || has_zero_advance && contains_local_overwrite(events, context)
                || has_link && decorated_word(events, context.font.current)
                || marker && marker_replay(events, context).is_some()
                || relative_advance && can_publish_inner_advances(events, context)
                || draws
                || overstrike_edit && has_proved_overstrike_entry(context)
                || absolute_edit
                || relative_retreat && has_proved_overstrike_entry(context)
                || armed_overstrike
                || replay_empty_overstrike
                || tight_word_can_carry_glyph(events, context)
                || replayable_device_tail(context, edits, !events.is_empty()))
}

fn motion_columns(value: &str) -> Option<(bool, i32)> {
    let absolute = value.strip_prefix('|');
    libmandoc_rs::TableColumnMetrics::motion_argument(absolute.unwrap_or(value))
        .map(|(units, _)| (absolute.is_some(), units))
}

fn has_prior_retreat(context: &TextExecutionContext<'_>) -> bool {
    context.zero_advance.has_buffered_glyph() || context.slot_entry.retreat_blanks > 0
}

fn replayable_device_tail(
    context: &TextExecutionContext<'_>,
    edits: bool,
    has_words: bool,
) -> bool {
    context
        .slot_entry
        .device_tail
        .as_ref()
        .is_some_and(|tail| edits || tail.has_retreated() && has_words)
}

fn decorated_word(events: &[RoffInlineEvent], entry_font: RoffFont) -> bool {
    use super::native_writes::has_native_decoration;
    has_native_decoration(entry_font)
        || events.iter().any(
            |event| matches!(event, RoffInlineEvent::Font(font) if has_native_decoration(*font)),
        )
}

fn can_place_word(context: &TextExecutionContext<'_>) -> bool {
    if !context.policy.record_native_cells {
        return false;
    }
    // The inherited BACK pair may own an earlier operand or its entry
    // blank. Leave that receipt with the live word executor.
    if context.slot_entry.prior_input
        && context.zero_advance.is_armed()
        && context.zero_advance.has_buffered_glyph()
    {
        return false;
    }
    !context.zero_advance.has_pending_glyph() || can_place_pending(context)
}

/// A complete in-word backspace stays editable until the native field
/// retires. An unfinished escape still uses the delayed glyph owner,
/// whose recovery and annotation state can span multiple fragments.
fn contains_local_overwrite(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    let inherited_glyph = has_prior_retreat(context);
    let mut armed = false;
    let mut backbefore = false;
    let mut wrote_pair = false;
    for event in events {
        match event {
            RoffInlineEvent::ZeroAdvance
                if !inherited_glyph || !armed && !backbefore && !wrote_pair =>
            {
                armed = true;
            }
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value)
                if value.chars().all(|ch| {
                    ch.is_ascii_graphic() || !inherited_glyph && matches!(ch, ' ' | '\u{a0}')
                }) =>
            {
                // term.c::encode does not consume BACKAFTER on a blank.
                // encode1 consumes BACKBEFORE before writing the next
                // glyph. A second arm may leave another pending glyph.
                for _ in value.bytes().filter(u8::is_ascii_graphic) {
                    wrote_pair |= backbefore;
                    backbefore = std::mem::take(&mut armed);
                }
            }
            RoffInlineEvent::Link(_)
            | RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::ZeroWidthGlyph => {}
            RoffInlineEvent::Breakpoint
            | RoffInlineEvent::LineBreak
            | RoffInlineEvent::NonBreakingWordBlank
                if !inherited_glyph => {}
            _ => return false,
        }
    }
    wrote_pair && !armed && (!inherited_glyph || !backbefore)
}

fn can_publish_inner_advances(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    if !context.policy.field_authoritative
        || context.zero_advance.is_armed()
        || context.zero_advance.has_pending_glyph()
        || context.zero_advance.has_annotation_output_owner()
    {
        // Pending glyphs and semantic label prefixes have their own
        // native output receipt. Do not claim that layout as inline text.
        return false;
    }
    let mut ink = false;
    let mut advance = false;
    let mut followed = false;
    for event in events {
        match event {
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text) => {
                let printable = text
                    .chars()
                    .any(|ch| !ch.is_whitespace() && !ch.is_control());
                followed |= advance && printable;
                ink |= printable;
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if motion_columns(value).is_some_and(|(absolute, units)| absolute || units > 0) => {
                advance |= ink;
            }
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                argument: Some(value),
            } if value.ends_with(' ') => {
                advance |= ink;
            }
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::ZeroWidthGlyph => {}
            _ => return false,
        }
    }
    followed
}

enum MarkerReplay {
    PreserveSlots,
    AwaitFieldFlush,
}

/// A marker sets breakline during the eventual `term_fill()`, while
/// `term_word()` can still overwrite raw slots on either side of it.
fn marker_replay(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> Option<MarkerReplay> {
    if context.zero_advance.has_buffered_glyph() {
        return None;
    }
    // Wider word shapes are replayable as a whole at a fresh filled
    // field. A pending run-in or source-blank receipt still owns its
    // earlier spans and must finish through the shared word path.
    // Breakable spaces stay with the incremental pass scanner, whose
    // row ends can close the previous owner as a section HEAD begins.
    let filled = context.slot_entry.domain == ReplayDomain::Filled
        && !context.slot_entry.prior_input
        && !context.defer_leading_blanks
        && !context.zero_advance.has_annotation_output_owner();
    let mut has_ink = false;
    let mut has_marker = false;
    let mut ink_after_marker = false;
    let mut needs_complete_word = false;
    let mut has_backstep = false;
    for event in events {
        match event {
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text)
                if text.bytes().all(|byte| byte.is_ascii_graphic()) =>
            {
                has_ink |= !text.is_empty();
                ink_after_marker |= has_marker && !text.is_empty();
            }
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text)
                if filled
                    && text
                        .chars()
                        .all(|ch| ch == '\u{a0}' || ch.is_ascii_graphic()) =>
            {
                let ink = text.bytes().any(|byte| byte.is_ascii_graphic());
                has_ink |= ink;
                ink_after_marker |= has_marker && ink;
                needs_complete_word = true;
            }
            RoffInlineEvent::LineBreak => {
                has_marker = true;
                // A trailing request does not prove an editable suffix.
                // The next word still owns execution of that boundary.
                ink_after_marker = false;
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if filled && motion_columns(value).is_some() => {
                needs_complete_word = true;
            }
            RoffInlineEvent::Breakpoint
            | RoffInlineEvent::BreakableHyphen
            | RoffInlineEvent::NonBreakingWordBlank
                if filled =>
            {
                needs_complete_word = true;
            }
            RoffInlineEvent::ZeroAdvance => has_backstep = true,
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::ZeroWidthGlyph => {}
            _ => return None,
        }
    }
    // Without a completed local pair, the existing glyph owner must
    // carry a pending BACK register through the next term_word().
    if has_backstep && (needs_complete_word || !contains_local_overwrite(events, context)) {
        return None;
    }
    if !has_marker || !has_ink || !ink_after_marker && !has_backstep {
        return None;
    }
    Some(if ink_after_marker {
        MarkerReplay::AwaitFieldFlush
    } else {
        MarkerReplay::PreserveSlots
    })
}

fn has_live_raw_cursor(context: &TextExecutionContext<'_>) -> bool {
    context.slot_entry.prefix.as_ref().is_some_and(|prefix| {
        prefix.cursor_retreat > 0
            || prefix
                .cells
                .iter()
                .rev()
                .find(|(cell, _)| {
                    !matches!(cell, FieldCell::ZeroWidthGraph | FieldCell::TabReference)
                })
                .is_some_and(|(cell, _)| *cell == FieldCell::Backline)
    })
}

fn has_proved_overstrike_entry(context: &TextExecutionContext<'_>) -> bool {
    !context.slot_entry.prior_input
        || context.slot_entry.prefix.is_some()
        || context.slot_entry.device_tail.is_some()
        || context.slot_entry.automatic_blanks > 0
}

fn has_unproved_armed_entry(context: &TextExecutionContext<'_>) -> bool {
    // A blank overstrike's input and the native entry blank can bound an
    // edit even when the earlier cursor is opaque. An empty overstrike
    // also needs a proved source-row reference to trim the required pair.
    context.slot_entry.prior_input
        && context.zero_advance.is_armed()
        && context.slot_entry.prefix.is_none()
}

fn can_place_pending(context: &TextExecutionContext<'_>) -> bool {
    context.zero_advance.can_project_prior_slot()
        && context
            .slot_entry
            .prefix
            .as_ref()
            .is_some_and(|prefix| prefix.pending_cell.is_some())
}

fn tight_word_can_carry_glyph(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    // This handoff extends the ordinary tight word path. Font decoration,
    // source delimiters and invisible cells keep their existing execution
    // receipts unless the arriving operand actually needs a raw-slot edit.
    context.slot_entry.automatic_blanks == 0
        && context.zero_advance.has_pending_glyph()
        && !context.zero_advance.is_armed()
        && !events.is_empty()
        && context.font.current == RoffFont::Regular
        && context.slot_entry.prefix.as_ref().is_some_and(|prefix| {
            prefix.pending_cell == prefix.cells.len().checked_sub(1)
                && matches!(prefix.cells.last(), Some((FieldCell::Graph { width: 1, .. }, 1)))
        })
        && events.iter().all(|event| {
            matches!(event, RoffInlineEvent::Text(value) if value.chars().all(|ch| ch.is_ascii_alphanumeric()))
        })
}

#[derive(Clone, Copy)]
enum ReadingGlyph<'a> {
    Scalar(char),
    Spelling(&'a str),
    Earlier(usize),
    EntryBlank,
    DeviceAdvance,
}

impl ReadingGlyph<'_> {
    fn append(self, text: &mut String) {
        match self {
            Self::Scalar(value) => text.push(value),
            Self::Spelling(value) => text.push_str(value),
            Self::Earlier(_) | Self::EntryBlank | Self::DeviceAdvance => {}
        }
    }

    fn scalars(self) -> usize {
        match self {
            Self::Scalar(_) | Self::EntryBlank => 1,
            Self::Spelling(text) => text.chars().count(),
            Self::Earlier(count) => count,
            Self::DeviceAdvance => 0,
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
        Self::push_reading(&mut self.reading, glyph, font, self.active_link)
    }

    fn push_reading(
        reading: &mut Vec<ReadingCell<'a>>,
        glyph: ReadingGlyph<'a>,
        font: Font,
        link: Option<usize>,
    ) -> usize {
        let owner = reading.len();
        reading.push(ReadingCell {
            glyph,
            font,
            link,
            sphinx_marker: false,
            authored_separator: false,
            preserve_separator: false,
        });
        owner
    }

    fn write_entry_blanks(&mut self, count: usize, kept: bool) {
        let blank = if kept {
            FieldCell::NonBreakingBlank
        } else {
            FieldCell::BreakableBlank
        };
        for _ in 0..count {
            let owner = self.mark(ReadingGlyph::EntryBlank, Font::Regular);
            self.slots.native_write(
                FieldWrite::Cell(blank.clone()),
                RoffFont::Regular,
                Some(owner),
            );
        }
    }

    fn input(
        &mut self,
        event: &'a RoffInlineEvent,
        state: &mut FontState,
        line_width_units: usize,
        include_advances: bool,
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
            RoffInlineEvent::Overstrike { source, .. } => {
                let reading = &mut self.reading;
                let link = self.active_link;
                self.slots.overstrike(source, font, |character| {
                    crate::text_safety::is_safe_source_character(character).then(|| {
                        Self::push_reading(reading, ReadingGlyph::Scalar(character), display, link)
                    })
                });
            }
            RoffInlineEvent::HorizontalMotion { argument, .. } => {
                let owner =
                    include_advances.then(|| self.mark(ReadingGlyph::DeviceAdvance, display));
                self.slots.motion(argument.as_deref(), owner);
            }
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                argument: Some(argument),
            } => {
                let owner =
                    include_advances.then(|| self.mark(ReadingGlyph::DeviceAdvance, display));
                self.slots.line(argument, line_width_units, font, owner);
            }
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
                if matches!(self.marker_boundary, MarkerBoundary::Pending)
                    && self.slots.cursor_before_end()
                {
                    self.marker_boundary = MarkerBoundary::AuthoredBlank;
                }
                if self.slots.claim_entry_blank(
                    owner,
                    |prior| matches!(self.reading[prior].glyph, ReadingGlyph::EntryBlank),
                    |prior| {
                        matches!(self.reading[prior].glyph,
                        ReadingGlyph::Scalar(ch) if !ch.is_whitespace())
                            || matches!(self.reading[prior].glyph, ReadingGlyph::Spelling(_))
                    },
                ) {
                    continue;
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
    prefix_start: usize,
    earlier: usize,
    unpublished: Option<usize>,
}

impl GeometrySeed<'_> {
    fn begin(context: &mut TextExecutionContext<'_>, events: &[RoffInlineEvent]) -> Self {
        let mut seed = if let Some(prefix) = context.slot_entry.prefix.take() {
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
                prefix_start: prefix.start,
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
            let automatic = context.slot_entry.automatic_blanks;
            let replace_tail = tail.fill_cells + automatic;
            let mut placement = Placement::empty(tail.into_slots());
            placement.write_entry_blanks(automatic, context.slot_entry.kept_blanks);
            Self {
                placement,
                replace_tail,
                replayed_entry: automatic > 0,
                prefix_start: 0,
                earlier: 0,
                unpublished: None,
            }
        } else {
            let mut placement =
                Placement::empty(NativeSlots::after_entry(context.slot_entry.prior_input));
            let mut replace_tail = context.slot_entry.retreat_blanks;
            let replayed_entry = replace_tail == 0
                && context.slot_entry.prior_input
                && context.slot_entry.automatic_blanks > 0
                && (context.zero_advance.is_armed()
                    || events
                        .iter()
                        .any(|event| matches!(event, RoffInlineEvent::Overstrike { .. }))
                        && !events
                            .iter()
                            .any(|event| matches!(event, RoffInlineEvent::LineBreak)));
            if replace_tail > 0 {
                placement.slots.enter_with_retreat(replace_tail);
            } else if replayed_entry {
                replace_tail = context.slot_entry.automatic_blanks;
                if context.slot_entry.trailing_tab_reference {
                    placement.slots.native_write(
                        FieldWrite::Cell(FieldCell::TabReference),
                        RoffFont::Regular,
                        None,
                    );
                    replace_tail += 1;
                }
                placement.write_entry_blanks(
                    context.slot_entry.automatic_blanks,
                    context.slot_entry.kept_blanks,
                );
            }
            Self {
                placement,
                replace_tail,
                replayed_entry,
                prefix_start: 0,
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
    let seed = GeometrySeed::begin(context, events);
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
    let include_advances = can_publish_inner_advances(events, context);
    if context.zero_advance.is_armed() {
        seed.placement.slots.arm_backafter();
    }
    for (index, event) in events.iter().enumerate() {
        if matches!(event, RoffInlineEvent::NoSpace) && index + 1 == events.len() {
            continuation = Some(!seed.placement.slots.backafter());
        }
        seed.placement
            .input(event, &mut font, context.line_width_units, include_advances);
        if let Some(edit) = seed.placement.slots.prior_edit() {
            return Err(edit);
        }
    }
    let has_marker = events
        .iter()
        .any(|event| matches!(event, RoffInlineEvent::LineBreak));
    let marker = has_marker.then(|| marker_replay(events, context)).flatten();
    let retain_suffix = context.policy.field_authoritative && (!has_marker || marker.is_some());
    if retain_suffix
        && events.iter().any(absolute_motion)
        && seed.placement.slots.cursor_before_owner()
    {
        // A cursor inside an earlier owner has no detachable slot suffix.
        // Preserve the text rather than implying that the cursor can be
        // carried forward at the end of that earlier word.
        return Err(PriorSlotEdit::Unresolved);
    }
    let placed = PlacedWord::prepare(
        seed,
        retain_suffix,
        include_advances,
        matches!(marker, Some(MarkerReplay::AwaitFieldFlush)),
    )?;
    *context.font = font;
    context.zero_advance.take_armed();
    word.source_continuation = continuation;
    placed.project(context, word);
    Ok(())
}
