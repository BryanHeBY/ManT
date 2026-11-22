//! Keep native edit operations before pass acceptance and reading projection.

use super::{Font, FontState, RoffInlineEvent, TextExecutionContext, WordExecution};
use crate::mandoc::inline::flow::{
    field_buffer::{FieldCell, FieldWrite},
    native_slots::{EntryCursor, NativeSlots, PriorSlotEdit, ReplayDomain, ReplayPass},
};
use crate::mandoc::roff_escape::{GlyphInput, PresentationKind, RoffFont};

mod overprint;
mod projection;

use projection::PlacedWord;

pub(super) fn absolute_motion(event: &RoffInlineEvent) -> bool {
    matches!(event, RoffInlineEvent::HorizontalMotion { argument: Some(value), .. }
        if value.strip_prefix('|').and_then(libmandoc_rs::TableColumnMetrics::motion_argument).is_some())
}

pub(super) fn relative_zero_motion(argument: Option<&str>) -> bool {
    argument.and_then(motion_columns) == Some((false, 0))
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
            RoffInlineEvent::Glyph(value, _) if !field => {
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
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _) => {
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
            || has_zero_advance && (isolated_needs_accepted_retreat || marker && authored_blank))
        || marker && (edits || context.table_leading_blanks != 0)
        || field
            && (has_live_raw_cursor(context)
                || has_zero_advance && local_overwrite(events, context).is_some()
                || has_link && decorated_word(events, context.font.current)
                || marker && marker_replay(events, context).is_some()
                || relative_advance && can_publish_inner_advances(events, context)
                || draws
                || overstrike_edit && has_proved_overstrike_entry(context)
                || absolute_edit
                || relative_retreat
                    && can_replay_relative_retreat(events, context, has_zero_advance)
                || has_armed_overlap
                || tight_word_can_carry_glyph(events, context)
                || replayable_device_tail(context, edits, !events.is_empty()))
}

fn motion_columns(value: &str) -> Option<(bool, i32)> {
    let absolute = value.strip_prefix('|');
    libmandoc_rs::TableColumnMetrics::motion_argument(absolute.unwrap_or(value))
        .map(|(units, _)| (absolute.is_some(), units))
}

fn trailing_relative_retreat(events: &[RoffInlineEvent]) -> bool {
    let Some((last, preceding)) = events.split_last() else {
        return false;
    };
    matches!(last, RoffInlineEvent::HorizontalMotion { argument: Some(value), .. }
        if motion_columns(value).is_some_and(|(absolute, units)| !absolute && units < 0))
        && preceding.iter().all(|event| {
            matches!(
                event,
                RoffInlineEvent::Font(_)
                    | RoffInlineEvent::PreviousFont
                    | RoffInlineEvent::Text(_)
                    | RoffInlineEvent::Glyph(..)
                    | RoffInlineEvent::ZeroWidthGlyph
            )
        })
}

fn has_prior_retreat(context: &TextExecutionContext<'_>) -> bool {
    context.zero_advance.has_buffered_glyph() || context.slot_entry.retreat_blanks > 0
}

fn can_replay_relative_retreat(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
    local_advance: bool,
) -> bool {
    // A local BACKBEFORE write can establish the cursor before the retreat
    // even when the previous word defers its final blank (term.c::encode1).
    has_proved_overstrike_entry(context)
        || local_advance && !has_prior_retreat(context)
        || context.slot_entry.cursor == EntryCursor::AtEnd && trailing_relative_retreat(events)
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
    if context.slot_entry.cursor.has_input()
        && context.zero_advance.is_armed()
        && context.zero_advance.has_buffered_glyph()
    {
        return false;
    }
    !context.zero_advance.has_pending_glyph() || can_place_pending(context)
}

#[derive(PartialEq, Eq)]
enum LocalOverwrite {
    Covered,
    HoldsGlyph,
    EditableDecoration,
}

/// A complete in-word backspace stays editable until the native field
/// retires. An unfinished escape still uses the delayed glyph owner,
/// whose recovery and annotation state can span multiple fragments.
fn local_overwrite(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> Option<LocalOverwrite> {
    let inherited_glyph = has_prior_retreat(context);
    let mut armed = false;
    let mut backbefore = false;
    let mut wrote_pair = false;
    let mut retain_no_space_decoration = false;
    let mut projected_padding = false;
    for event in events {
        match event {
            RoffInlineEvent::ZeroAdvance
                if !inherited_glyph || !armed && !backbefore && !wrote_pair =>
            {
                armed = true;
            }
            RoffInlineEvent::NoSpace if armed && !inherited_glyph && !backbefore && !wrote_pair => {
                // term_word() consumes BACKAFTER here unless a glyph
                // such as UTF-8 NBSP already did so through encode1().
                // Defer the actual register transitions to placement.
                armed = false;
                retain_no_space_decoration = true;
            }
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _) => {
                // term.c::encode does not consume BACKAFTER on a blank.
                // encode1 consumes BACKBEFORE before writing the next
                // glyph. A second arm may leave another pending glyph.
                // A named escape can write a different native sequence
                // from its one-column reading glyph (term.c:611-636).
                let direct_scalar = matches!(
                    event,
                    RoffInlineEvent::Text(_) | RoffInlineEvent::Glyph(_, GlyphInput::Codepoint)
                );
                for ch in value.chars() {
                    if !inherited_glyph && matches!(ch, ' ' | '\u{a0}') {
                        continue;
                    }
                    if !(ch.is_ascii_graphic()
                        || direct_scalar && super::native_writes::is_single_column_ink(ch))
                    {
                        return None;
                    }
                    wrote_pair |= backbefore;
                    backbefore = std::mem::take(&mut armed);
                }
            }
            RoffInlineEvent::Link(_)
            | RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::ZeroWidthGlyph => {}
            RoffInlineEvent::LineBreak if !inherited_glyph => {}
            RoffInlineEvent::Breakpoint | RoffInlineEvent::NonBreakingWordBlank
                if !inherited_glyph =>
            {
                projected_padding = true;
            }
            _ => return None,
        }
    }
    // encode() writes an ordinary trailing SP without consuming
    // BACKBEFORE (term.c:946-967). Preserve the complete decorated word
    // so the next HORIZ can revisit its font slots. Ordinary words
    // already retain the editable blank through the shared word path.
    let retain_decoration = (backbefore || retain_no_space_decoration)
        && !inherited_glyph
        && context.slot_entry.domain == ReplayDomain::Filled
        && !context.slot_entry.cursor.has_input()
        && !context.defer_leading_blanks
        && !context.zero_advance.has_annotation_output_owner()
        // These controls can already have an independently projected
        // separator around BACKBEFORE. Leave that owner on the shared path.
        && !projected_padding
        && decorated_word(events, context.font.current)
        && (fits_editable_font_slots(events)
            || backbefore
                && events.last().is_some_and(
                    |event| matches!(event, RoffInlineEvent::Text(value) if value.ends_with(' ')),
                ));
    if armed || inherited_glyph && backbefore {
        None
    } else if retain_no_space_decoration {
        retain_decoration.then_some(LocalOverwrite::EditableDecoration)
    } else if backbefore {
        (wrote_pair || retain_decoration).then_some(LocalOverwrite::HoldsGlyph)
    } else {
        wrote_pair.then_some(LocalOverwrite::Covered)
    }
}

fn fits_editable_font_slots(events: &[RoffInlineEvent]) -> bool {
    use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;

    // encode1() writes up to five slots for one decorated glyph. Leave
    // room for the pending backstep and replay only a bounded local word.
    let mut slots = 1;
    for event in events {
        slots += match event {
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _) => {
                value.len().saturating_mul(5).min(MAX_EDITABLE_APPEND_CELLS)
            }
            _ => 1,
        };
        if slots > MAX_EDITABLE_APPEND_CELLS {
            return false;
        }
    }
    true
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
    let mut prior_zero_advance = false;
    let mut covered_pair = false;
    for (index, event) in events.iter().enumerate() {
        match event {
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _) => {
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
                if (prior_zero_advance || covered_pair) && absolute_motion(event) {
                    return false;
                }
                if std::mem::take(&mut prior_zero_advance) {
                    // encode1() has already consumed both BACK registers
                    // in this source operand. The next relative advance
                    // therefore writes its full width (term.c:678–699).
                    if local_overwrite(&events[..index], context) != Some(LocalOverwrite::Covered) {
                        return false;
                    }
                    covered_pair = true;
                }
                advance |= ink;
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if covered_pair
                && followed
                && motion_columns(value)
                    .is_some_and(|(absolute, units)| !absolute && units < 0) =>
            {
                // Native placement checks the earlier edit fence. Permit
                // an overwrite of later ink within the proved field.
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
            RoffInlineEvent::ZeroAdvance if !advance => prior_zero_advance = true,
            RoffInlineEvent::LineBreak if context.slot_entry.domain == ReplayDomain::Filled => {}
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
    let filled = context.slot_entry.domain == ReplayDomain::Filled
        && !context.slot_entry.cursor.has_input()
        && !context.defer_leading_blanks
        && !context.zero_advance.has_annotation_output_owner();
    // Plain source blanks already have an incremental pass receipt.
    // When they separate an annotation from surrounding text, keep the
    // blank and label in the editable slots until acceptance can decide
    // both their row and their visible ownership.
    let owns_link = events
        .iter()
        .any(|event| matches!(event, RoffInlineEvent::Link(_)));
    let mut has_ink = false;
    let mut has_marker = false;
    let mut ink_after_marker = false;
    let mut has_other_padding_controls = false;
    let mut has_backstep = false;
    let mut has_motion = false;
    for event in events {
        match event {
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _)
                if text.bytes().all(|byte| byte.is_ascii_graphic()) =>
            {
                has_ink |= !text.is_empty();
                ink_after_marker |= has_marker && !text.is_empty();
            }
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _)
                if filled
                    && text.chars().all(|ch| {
                        ch == '\u{a0}'
                            || ch == ' ' && (owns_link || has_marker && has_backstep)
                            || ch.is_ascii_graphic()
                    }) =>
            {
                let ink = text.bytes().any(|byte| byte.is_ascii_graphic());
                has_ink |= ink;
                ink_after_marker |= has_marker && ink;
                has_other_padding_controls |= owns_link || text.contains('\u{a0}');
            }
            RoffInlineEvent::LineBreak => {
                has_marker = true;
                // A final marker in a moving operand leaves its earlier
                // marker and graph editable. term_word() keeps the raw
                // suffix available to the next word until term_fill().
                ink_after_marker &= has_motion;
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if filled && motion_columns(value).is_some() => {
                has_other_padding_controls = true;
                has_motion = true;
            }
            RoffInlineEvent::Breakpoint
            | RoffInlineEvent::BreakableHyphen
            | RoffInlineEvent::NonBreakingWordBlank
                if filled =>
            {
                has_other_padding_controls = true;
            }
            RoffInlineEvent::ZeroAdvance => has_backstep = true,
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::ZeroWidthGlyph => {}
            _ => return None,
        }
    }
    // A placed local backstep proves the raw word, including any source
    // SP the terminal preserves beside a marker (term.c:659-661,946-967).
    // Otherwise the existing glyph owner must carry the pending BACK
    // register through the next term_word().
    if has_backstep && (has_other_padding_controls || local_overwrite(events, context).is_none()) {
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
    !context.slot_entry.cursor.has_input()
        || context.slot_entry.prefix.is_some()
        || context.slot_entry.device_tail.is_some()
        || context.slot_entry.automatic_blanks > 0
}

fn has_unproved_armed_entry(context: &TextExecutionContext<'_>) -> bool {
    // A blank overstrike's input and the native entry blank can bound an
    // edit even when the earlier cursor is opaque. An empty overstrike
    // also needs a proved source-row reference to trim the required pair.
    context.slot_entry.cursor.has_input()
        && context.zero_advance.is_armed()
        && context.slot_entry.prefix.is_none()
}

fn can_place_pending(context: &TextExecutionContext<'_>) -> bool {
    let Some(prefix) = &context.slot_entry.prefix else {
        return false;
    };
    let Some(cell) = prefix.pending_cell else {
        return false;
    };
    context.zero_advance.can_project_prior_slot()
        // termp_it_pre(BODY) can write its generated glyph into an
        // existing native SP slot (term.c:903-929). The field retains that
        // projected NBSP and the pending BACKBEFORE, but no cached glyph.
        || prefix.backbefore
            && context.zero_advance.can_replay_projected_prior_slot()
            && matches!(
                prefix.cells.get(cell),
                Some((FieldCell::Graph { text: '\u{a0}', width: 1 }, _))
            )
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
    DeferredBlank(crate::mandoc::inline::flow::field_buffer::DeferredSourceBlank),
}

impl ReadingGlyph<'_> {
    fn append(self, text: &mut String) {
        match self {
            Self::Scalar(value) => text.push(value),
            Self::Spelling(value) => text.push_str(value),
            Self::Earlier(_) | Self::EntryBlank | Self::DeviceAdvance | Self::DeferredBlank(_) => {}
        }
    }

    fn scalars(self) -> usize {
        match self {
            Self::Scalar(_) | Self::EntryBlank => 1,
            Self::Spelling(text) => text.chars().count(),
            Self::Earlier(count) => count,
            Self::DeviceAdvance | Self::DeferredBlank(_) => 0,
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
    marker_after_blank: bool,
    effective_absolute_cursor: bool,
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
            marker_after_blank: false,
            effective_absolute_cursor: false,
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
        self.retain_absolute_cursor_for_control(event);
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
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _) => {
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
                self.effective_absolute_cursor = self.slots.motion(argument.as_deref(), owner);
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
                        self.marker_after_blank = self.slots.follows_blank();
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

    fn retain_absolute_cursor_for_control(&mut self, event: &RoffInlineEvent) {
        self.effective_absolute_cursor &= matches!(
            event,
            RoffInlineEvent::NoSpace | RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont
        );
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
                if self.slots.claim_unprojected_blank(
                    owner,
                    |cell, prior| {
                        matches!(self.reading[prior].glyph, ReadingGlyph::EntryBlank)
                            || matches!(self.reading[prior].glyph, ReadingGlyph::Earlier(0))
                                && *cell == FieldCell::BreakableBlank
                    },
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
    prior_pending: Option<PriorPendingSlot>,
    advance_marker: Option<usize>,
    editable_pass: bool,
}

#[derive(Clone, Copy)]
enum PriorPendingSlot {
    Buffered(usize),
    Projected(usize),
}

impl PriorPendingSlot {
    fn at_entry(slot: Option<usize>, state: &super::ZeroAdvanceState) -> Option<Self> {
        slot.filter(|_| state.has_pending_glyph()).map(|slot| {
            if state.can_project_prior_slot() {
                Self::Buffered(slot)
            } else {
                Self::Projected(slot)
            }
        })
    }

    fn owner(self) -> usize {
        match self {
            Self::Buffered(owner) | Self::Projected(owner) => owner,
        }
    }

    fn unpublished(self) -> Option<usize> {
        match self {
            Self::Buffered(owner) => Some(owner),
            Self::Projected(_) => None,
        }
    }
}

// A continuation can make the previous trailing HORIZ printable. Its
// entry motion must stay in the new operand's own slots. A retreat
// through the retained prefix is handled by native overwrite replay.
fn moves_within_word_entry(events: &[RoffInlineEvent]) -> bool {
    let mut column = 0_i32;
    let mut moved = false;
    for event in events {
        let RoffInlineEvent::HorizontalMotion { argument, .. } = event else {
            break;
        };
        let Some((false, units)) = argument.as_deref().and_then(motion_columns) else {
            return false;
        };
        column = column.saturating_add(units);
        if column < 0 {
            return false;
        }
        moved |= units != 0;
    }
    moved && column == 0
}

impl GeometrySeed<'_> {
    fn begin(context: &mut TextExecutionContext<'_>, events: &[RoffInlineEvent]) -> Self {
        let mut seed = if let Some(prefix) = context.slot_entry.prefix.take() {
            let mut placement = Placement::empty(NativeSlots::with_field_prefix(&prefix));
            let earlier = prefix.cells.len();
            let advance_marker = placement
                .slots
                .trailing_advance_marker(prefix.automatic)
                .or_else(|| {
                    moves_within_word_entry(events)
                        .then(|| placement.slots.trailing_open_advance(prefix.automatic))
                        .flatten()
                });
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
                prior_pending: PriorPendingSlot::at_entry(
                    prefix.pending_cell,
                    context.zero_advance,
                ),
                replace_tail: 0,
                replayed_entry: false,
                advance_marker,
                editable_pass: prefix.pass == ReplayPass::Editable,
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
                prior_pending: None,
                advance_marker: None,
                editable_pass: false,
            }
        } else {
            let mut placement = Placement::empty(NativeSlots::after_entry(
                context.slot_entry.cursor.has_input(),
            ));
            let mut replace_tail = context.slot_entry.retreat_blanks;
            // An ordinary entry SP written at lastcol belongs to the
            // current operand's editable suffix before a relative HORIZ.
            // Earlier opaque slots need no replay if the retreat stays
            // within this suffix (term.c:573–582 and 704–714).
            let replay_entry_retreat = context.slot_entry.cursor == EntryCursor::AtEnd
                && trailing_relative_retreat(events);
            let replayed_entry = replace_tail == 0
                && context.slot_entry.cursor.has_input()
                && context.slot_entry.automatic_blanks > 0
                && (replay_entry_retreat
                    || context.zero_advance.is_armed()
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
                prior_pending: None,
                advance_marker: None,
                editable_pass: false,
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
    // The bounded overlay knows both col and lastcol. A marker in this
    // operand does not reset that cursor (term.c:659-660,678-714).
    // Carry the exact cursor if the next word must still write there.
    let slots = &seed.placement.slots;
    // A marker after new ink leaves an open cursor at the end of that
    // native segment, even when the old tail still holds glyphs.
    let live_cursor = retains_word_cursor(&seed, context.slot_entry.cursor, events);
    // A placed glyph can still be waiting under BACKBEFORE after a marker.
    // Keep its raw decoration and the invisible label slots around it, so
    // the next word can revisit the same native suffix (term.c::encode1).
    let pending_glyph = seed.placement.slots.pending_owner().is_some();
    // term_word() writes a marker at col. An unpainted native suffix can
    // carry it through continuation, motion or a second break control.
    // Keep its slots editable until term_fill() accepts a pass.
    let terminal_marker = seed.earlier != 0
        && slots.has_unpainted_marker_tail()
        && events
            .iter()
            .rposition(|event| matches!(event, RoffInlineEvent::LineBreak))
            .is_some_and(|marker| {
                // HORIZ may traverse an occupied slot without writing it
                // (term.c::bufferc). Extend the cursor proof only for a
                // marker actually reached after writing fixed advances.
                events[..marker].iter().all(|event| match event {
                    RoffInlineEvent::HorizontalMotion { argument, .. } => {
                        match argument.as_deref().and_then(motion_columns) {
                            Some((false, 0)) => true,
                            Some((false, units)) if units > 0 => {
                                slots.trailing_advance_marker(0).is_some()
                            }
                            _ => false,
                        }
                    }
                    _ => true,
                })
            });
    let retain_suffix = context.policy.field_authoritative
        && (!has_marker
            || marker.is_some()
            || live_cursor
            || pending_glyph
            || terminal_marker
            || seed.editable_pass);
    let editable_marker = matches!(marker, Some(MarkerReplay::AwaitFieldFlush))
        || has_marker && seed.editable_pass && slots.cursor_before_end()
        || has_marker && live_cursor
        // BACKBEFORE can hold a glyph inside material still following
        // col. Leave the marker editable until a real device flush.
        || has_marker && pending_glyph && seed.earlier != 0 && slots.cursor_before_end()
        // term_fill() advances past buffered blanks before the next graph.
        // Leave that separator to the real flush while keeping the raw
        // cursor and its owners available to intervening native writes.
        || terminal_marker && seed.placement.marker_after_blank;
    if retain_suffix
        && events.iter().any(absolute_motion)
        && seed.placement.slots.cursor_before_owner()
        && !covers_field_start(&seed, context.slot_entry.cursor)
    {
        // A partial replay cannot carry a cursor inside an earlier owner.
        // When the whole field is available, keep col and lastcol even
        // when a leading \p makes term_flushln() accept none of its ink.
        return Err(PriorSlotEdit::Unresolved);
    }
    let placed = PlacedWord::prepare(
        seed,
        retain_suffix,
        include_advances,
        editable_marker,
        context.zero_advance.native_word_owner(),
    )?;
    *context.font = font;
    context.zero_advance.take_armed();
    word.source_continuation = continuation;
    placed.project(context, word);
    Ok(())
}

fn retains_word_cursor(
    seed: &GeometrySeed<'_>,
    entry: EntryCursor,
    events: &[RoffInlineEvent],
) -> bool {
    let slots = &seed.placement.slots;
    slots.cursor_before_end()
        && (seed.prefix_start > 0
            // An initial control-only word can leave the cursor before
            // a marker. The next word must be able to overwrite it.
            || !entry.has_input()
                && slots.cursor_before_invisible_tail()
                && !slots.contains_ink_from(0)
            || retains_absolute_word_cursor(seed, entry)
            || seed.earlier != 0
                && (slots.cursor_before_invisible_tail()
                    || slots.has_unpainted_marker_tail()
                        && !events.iter().any(absolute_motion)
                        && slots.contains_ink_from(seed.earlier)
                    || slots_rewritten_by_controls(events)))
}

fn covers_field_start(seed: &GeometrySeed<'_>, entry: EntryCursor) -> bool {
    use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;

    // An exact prefix starting at col zero and a fresh field both own
    // their origin. Missing prefix evidence with prior input stays opaque.
    seed.prefix_start == 0
        && (seed.earlier != 0
            || !entry.has_input()
                && seed.placement.slots.extent_and_retreat().0 <= MAX_EDITABLE_APPEND_CELLS)
}

fn retains_absolute_word_cursor(seed: &GeometrySeed<'_>, entry: EntryCursor) -> bool {
    use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;

    // An absolute motion in a complete field moves col without
    // discarding the later slots. A marker before the motion is still
    // unread by term_fill() until the actual flush (term.c:659–660,
    // 678–714). Keep that state only after the absolute motion executed.
    let slots = &seed.placement.slots;
    covers_field_start(seed, entry)
        && slots.extent_and_retreat().0 <= MAX_EDITABLE_APPEND_CELLS
        && slots.cursor_before_owner()
        && slots.contains_ink_from(seed.earlier)
        && seed.placement.effective_absolute_cursor
}

/// A complete replay can place `\p` or another control inside a field
/// without contributing a new scalar. `bufferc()` can advance `col` within
/// the existing `lastcol` extent (term.c:860–873). Both an absolute target
/// and a negative relative motion can start that edit (term.c:678–719).
fn slots_rewritten_by_controls(events: &[RoffInlineEvent]) -> bool {
    let mut rewound = false;
    let mut marker = false;
    for event in events {
        match event {
            RoffInlineEvent::HorizontalMotion { argument, .. } => {
                rewound |= argument
                    .as_deref()
                    .and_then(motion_columns)
                    .is_some_and(|(absolute, units)| absolute || units < 0);
            }
            RoffInlineEvent::LineBreak => marker = true,
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::NoSpace
            | RoffInlineEvent::ZeroAdvance
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint => {}
            _ => return false,
        }
    }
    rewound && marker
}
