//! Keep native edit operations before pass acceptance and reading projection.

use super::{Font, FontState, RoffInlineEvent, TextExecutionContext, WordExecution};
use crate::mandoc::inline::flow::{
    field_buffer::{FieldCell, FieldProjectionOwner, FieldWrite},
    native_slots::{
        CursorReposition, EntryCursor, NativeSlots, PrefixImport, PriorSlotEdit, ReplayDomain,
        ReplayPass, SlotEntry,
    },
};
use crate::mandoc::roff_escape::{GlyphInput, PresentationKind, RoffFont};

mod advance_policy;
mod marked_operands;
mod marker_replay;
mod overprint;
mod projection;

use advance_policy::{
    AdvanceReading, can_defer_live_advances, can_defer_trailing_advance, can_place_relative_advance,
};
use marked_operands::ControlReplay;
use marker_replay::{MarkerReplay, marker_replay};
use projection::PlacedWord;

pub(super) fn absolute_motion(event: &RoffInlineEvent) -> bool {
    matches!(event, RoffInlineEvent::HorizontalMotion { argument: Some(value), .. }
        if value.strip_prefix('|').and_then(libmandoc_rs::TableColumnMetrics::motion_argument).is_some())
}

pub(super) fn relative_zero_motion(argument: Option<&str>) -> bool {
    argument.and_then(motion_columns) == Some((false, 0))
}

/// Both plain filled text (`Filled`) and filled list/definition lines
/// (`DefinitionLine`) execute `term_word()`/`term_fill()`/`term_flushln()` on
/// one live `tcol->buf` (`term.c:559-822`, `mdoc_term.c:757-838, 939-952`).
pub(super) const fn is_filled_line_domain(domain: ReplayDomain) -> bool {
    matches!(domain, ReplayDomain::Filled | ReplayDomain::DefinitionLine)
}

pub(super) fn applies(events: &[RoffInlineEvent], context: &TextExecutionContext<'_>) -> bool {
    let Some(control_replay) = can_place_word(events, context) else {
        return false;
    };
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
            RoffInlineEvent::HorizontalMotion { argument, .. } => {
                if let Some((absolute, units)) = argument.as_deref().and_then(motion_columns) {
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
    let has_armed_overlap =
        armed_overstrike || armed_empty_overstrike && context.slot_entry.trailing_tab_reference;
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
                || control_replay.is_proved()
                || relative_advance && can_place_relative_advance(events, context)
                || draws
                || overstrike_edit && has_proved_overstrike_entry(context)
                || absolute_edit
                || relative_retreat
                    && can_replay_relative_retreat(events, context, has_zero_advance)
                || has_armed_overlap
                || tight_word_can_carry_glyph(events, context)
                || tight_run_in_word_can_replay(events, context)
                || marker_then_graph_can_replay(events, context)
                || word_can_consume_projected_blank(events, context)
                || replayable_device_tail(context, edits, !events.is_empty()))
}

fn motion_columns(value: &str) -> Option<(bool, i32)> {
    let absolute = value.strip_prefix('|');
    libmandoc_rs::TableColumnMetrics::motion_argument(absolute.unwrap_or(value))
        .map(|(units, _)| (absolute.is_some(), units))
}

fn trailing_relative_retreat(mut events: &[RoffInlineEvent]) -> bool {
    while let Some((RoffInlineEvent::NoSpace, rest)) = events.split_last() {
        events = rest;
    }
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
                    | RoffInlineEvent::NoSpace
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

/// An incoming marker is a write without a retreat; the following graph
/// consumes BACKBEFORE and turns the live BACKAFTER into the next retreat.
/// Replay only with the exact pending slot and a bounded native field.
fn marker_then_graph_can_replay(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    let [RoffInlineEvent::LineBreak, RoffInlineEvent::Text(text)] = events else {
        return false;
    };
    is_filled_line_domain(context.slot_entry.domain)
        && context.zero_advance.can_replay_projected_prior_slot()
        && !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_graphic())
        && context.slot_entry.prefix.as_ref().is_some_and(|prefix| {
            prefix.start == 0
                && prefix.backbefore
                && prefix.backafter
                && prefix.pending_cell.is_some()
                && prefix
                    .raw_slot_count()
                    .saturating_add(text.len().saturating_mul(5))
                    .saturating_add(1)
                    <= prefix.slot_limit
        })
}

fn can_place_word(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> Option<ControlReplay> {
    if !context.policy.record_native_cells {
        return None;
    }
    let controls = marked_operands::control_replay(events, context);
    // The inherited BACK pair may own an earlier operand or its entry
    // blank. Leave that receipt with the live word executor unless we hold
    // the exact prefix and the incoming word first cancels BACKAFTER
    // (term.c:662-682).
    if context.slot_entry.cursor.has_input()
        && context.zero_advance.is_armed()
        && context.zero_advance.has_buffered_glyph()
        && !controls.cancels_arm()
        && !marker_then_graph_can_replay(events, context)
        && !(context
            .slot_entry
            .prefix
            .as_ref()
            .is_some_and(|prefix| prefix.start == 0 && prefix.backbefore && prefix.backafter)
            && first_control_cancels_backafter(events))
    {
        return None;
    }
    (!context.zero_advance.has_buffered_glyph() || can_place_pending(context)).then_some(controls)
}

fn first_control_cancels_backafter(events: &[RoffInlineEvent]) -> bool {
    // term.c:621-622, 659-661, 799-801: `\:` (`ASCII_BREAK`), `\&`
    // (`ASCII_NBRZW`), and `\p` (`'\n'`) call `bufferc()` without clearing
    // `TERMP_BACKAFTER` or `TERMP_BACKBEFORE`, so a following `\c` or `\h`
    // still cancels `BACKAFTER`.
    let Some(first) = events.iter().position(|event| {
        !matches!(
            event,
            RoffInlineEvent::Font(_)
                | RoffInlineEvent::PreviousFont
                | RoffInlineEvent::Breakpoint
                | RoffInlineEvent::ZeroWidthGlyph
                | RoffInlineEvent::LineBreak
                | RoffInlineEvent::ZeroAdvance
        ) && !matches!(event, RoffInlineEvent::Text(text) if text.bytes().all(|byte| byte == b' '))
    }) else {
        return false;
    };
    if !matches!(
        events[first],
        RoffInlineEvent::NoSpace | RoffInlineEvent::HorizontalMotion { .. }
    ) {
        return false;
    }
    let mut seen_motion = false;
    let mut consumed_backbefore = false;
    for event in &events[first + 1..] {
        match event {
            RoffInlineEvent::HorizontalMotion { argument, .. } => {
                seen_motion = true;
                // term.c:678-718: a non-zero HORIZ consumes BACKBEFORE before
                // later ink reaches encode1(), while non-motion `\cX` stays on
                // the zero-advance path.
                if argument
                    .as_deref()
                    .and_then(motion_columns)
                    .is_some_and(|(absolute, units)| absolute || units != 0)
                {
                    consumed_backbefore = true;
                }
            }
            RoffInlineEvent::LineBreak
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint
            | RoffInlineEvent::NoSpace
            | RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont => {}
            RoffInlineEvent::Text(text) if text.bytes().all(|byte| byte == b' ') => {}
            RoffInlineEvent::Text(_)
            | RoffInlineEvent::Glyph(..)
            | RoffInlineEvent::BreakableHyphen
            | RoffInlineEvent::NonBreakingWordBlank
            | RoffInlineEvent::ZeroAdvance
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::DeviceName
                if consumed_backbefore => {}
            _ => return false,
        }
    }
    seen_motion
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
fn is_fresh_word_entry(context: &TextExecutionContext<'_>) -> bool {
    !context.slot_entry.cursor.has_input()
        && !context.defer_leading_blanks
        && !context.zero_advance.has_annotation_output_owner()
}

fn allows_fresh_decoration(
    context: &TextExecutionContext<'_>,
    fresh_word: bool,
    has_marker_event: bool,
) -> bool {
    fresh_word
        && (context.slot_entry.domain == ReplayDomain::Filled
            || has_marker_event
                && matches!(
                    context.slot_entry.domain,
                    ReplayDomain::DefinitionLine | ReplayDomain::Scoped
                ))
}

/// Check whether a local `\z` word can stay in bounded raw slots (`term.c:662-667,
/// 678-699, 888-969`: `ESCAPE_NOSPACE` clears `BACKAFTER`, positive `HORIZ`
/// clears `BACKAFTER`/`BACKBEFORE`, and `encode1` consumes `BACKBEFORE`).
fn has_word_end_marker(events: &[RoffInlineEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, RoffInlineEvent::LineBreak))
}

fn local_overwrite(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> Option<LocalOverwrite> {
    let inherited_glyph = has_prior_retreat(context);
    let fresh_word = !inherited_glyph && is_fresh_word_entry(context);
    let bounded_filled_prefix = !inherited_glyph && is_bounded_filled_prefix_entry(events, context);
    let has_marker_event = has_word_end_marker(events);
    let fresh_decoration = allows_fresh_decoration(
        context,
        fresh_word || bounded_filled_prefix,
        has_marker_event,
    );
    let append_motion = !inherited_glyph && local_motion_appends_at_word_end(events, context);
    let fresh_filled_motion = (fresh_word || bounded_filled_prefix)
        && is_filled_line_domain(context.slot_entry.domain)
        && (!has_marker_event || bounded_filled_prefix);
    let fresh_scoped_motion = fresh_word && context.slot_entry.domain == ReplayDomain::Scoped;
    let mut wrote_ink = false;
    let mut armed = false;
    let mut backbefore = false;
    let mut wrote_pair = false;
    let mut trailing_blank = false;
    let mut retain_no_space_decoration = false;
    let mut projected_padding = false;
    let mut resolved_motion_retreat = false;
    for event in events {
        match event {
            RoffInlineEvent::ZeroAdvance
                if !inherited_glyph || !armed && !backbefore && !wrote_pair =>
            {
                armed = true;
            }
            RoffInlineEvent::NoSpace => {
                // term_word() consumes BACKAFTER here without buffering a byte;
                // when unarmed, mid-word \c is a no-op and word-end \c only
                // sets continuation (term.c:662-667), handled by execute_seed().
                if std::mem::take(&mut armed) && !inherited_glyph && !backbefore && !wrote_pair {
                    retain_no_space_decoration = true;
                }
            }
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _) => {
                let direct_scalar = directly_buffered_glyphs(event);
                for ch in value.chars() {
                    if !inherited_glyph && matches!(ch, ' ' | '\u{a0}') {
                        trailing_blank = true;
                        continue;
                    }
                    if !(ch.is_ascii_graphic()
                        || direct_scalar && super::native_writes::is_single_column_ink(ch))
                    {
                        return None;
                    }
                    trailing_blank = false;
                    wrote_ink = true;
                    wrote_pair |= backbefore;
                    backbefore = std::mem::take(&mut armed);
                }
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if (append_motion || fresh_filled_motion && (!wrote_ink || backbefore))
                && motion_columns(value)
                    .is_some_and(|(absolute, units)| !absolute && units > 0)
                || fresh_scoped_motion
                    && (armed || backbefore)
                    && motion_columns(value) == Some((false, 24)) =>
            {
                // term_word() clears BACKAFTER without moving, or uses
                // the first positive HORIZ unit to clear BACKBEFORE
                // (term.c:678-699).
                let settles_motion = fresh_filled_motion || fresh_scoped_motion;
                resolved_motion_retreat |= !std::mem::take(&mut armed)
                    && std::mem::take(&mut backbefore)
                    && settles_motion;
                projected_padding |= !settles_motion;
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
        && fresh_decoration
        && !projected_padding
        && decorated_word(events, context.font.current)
        && retains_decorated_word(events, backbefore);
    let rearmed_tail = backbefore && retain_decoration && !trailing_blank && !wrote_pair;
    let rearmed_pair = fresh_word
        && wrote_pair
        && !(trailing_blank || projected_padding || has_marker_event)
        && retains_fresh_covered_rearm(events, context, !backbefore || retain_decoration);
    if armed && !(rearmed_tail || rearmed_pair) || inherited_glyph && backbefore {
        None
    } else if retain_no_space_decoration {
        retain_decoration.then_some(LocalOverwrite::EditableDecoration)
    } else if backbefore {
        (wrote_pair || retain_decoration).then_some(LocalOverwrite::HoldsGlyph)
    } else {
        (wrote_pair || resolved_motion_retreat).then_some(LocalOverwrite::Covered)
    }
}

fn retains_fresh_covered_rearm(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
    has_replayable_backbefore: bool,
) -> bool {
    // Once encode1() consumed the earlier BACK pair, a final \z arms
    // the next byte without changing the already proved raw placement
    // (term.c:773–775, 903–929).
    has_replayable_backbefore
        && context.slot_entry.domain == ReplayDomain::Filled
        && !context.zero_advance.has_annotation_output_owner()
        && matches!(events.last(), Some(RoffInlineEvent::ZeroAdvance))
        && fits_editable_font_slots(events)
}

// A named escape can write a different native sequence from the
// one-column reading glyph (term.c:611-636).
fn directly_buffered_glyphs(event: &RoffInlineEvent) -> bool {
    matches!(
        event,
        RoffInlineEvent::Text(_)
            | RoffInlineEvent::Glyph(_, GlyphInput::Codepoint | GlyphInput::NativeSpecial)
    )
}

/// `encode1()` expands decorated glyphs to 3-5 raw bytes (`term.c:911-925`),
/// and `encode()` preserves `BACKBEFORE` across a trailing `SP` (`term.c:946-967`).
fn retains_decorated_word(events: &[RoffInlineEvent], backbefore: bool) -> bool {
    fits_editable_font_slots(events)
        || backbefore
            && events.last().is_some_and(
                |event| matches!(event, RoffInlineEvent::Text(value) if value.ends_with(' ')),
            )
}

fn local_motion_appends_at_word_end(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    // A leading advance writes at lastcol before local ink. A tight
    // continuation or a marked operand can still own an old editable
    // suffix whose motion and delayed reading receipt must stay intact.
    context.slot_entry.domain == ReplayDomain::Filled
        && context.slot_entry.cursor == EntryCursor::AtEnd
        && context.slot_entry.automatic_blanks > 0
        && !context.defer_leading_blanks
        && !context.zero_advance.has_annotation_output_owner()
        && matches!(events.first(), Some(RoffInlineEvent::HorizontalMotion { argument: Some(value), .. })
            if motion_columns(value).is_some_and(|(absolute, units)| !absolute && units > 0))
        && !events
            .iter()
            .any(|event| matches!(event, RoffInlineEvent::LineBreak))
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

fn is_bounded_filled_prefix_entry(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    !context.zero_advance.is_armed()
        && !has_prior_retreat(context)
        && fits_bounded_filled_prefix(events, context)
        && context
            .slot_entry
            .prefix
            .as_ref()
            .is_some_and(|prefix| !prefix.backbefore && !prefix.backafter)
}

/// Allow `ESCAPE_HORIZ` alongside `\p` when `p->col` moves inside the live
/// `tcol->buf` before `term_fill()` executes (`term.c:659-660, 678-718`).
fn fits_bounded_filled_prefix(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    context.slot_entry.domain == ReplayDomain::Filled
        && context.slot_entry.prefix.as_ref().is_some_and(|prefix| {
            let per_glyph = if decorated_word(events, context.font.current) {
                5
            } else {
                1
            };
            let Some(incoming) = events.iter().try_fold(0usize, |acc, event| {
                let added = match event {
                    RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _) => {
                        text.len().saturating_mul(per_glyph)
                    }
                    RoffInlineEvent::HorizontalMotion {
                        argument: Some(value),
                        ..
                    } => {
                        let (false, units) = motion_columns(value)? else {
                            return None;
                        };
                        if units > 0 {
                            libmandoc_rs::TableColumnMetrics::relative_motion_columns(value)
                        } else {
                            0
                        }
                    }
                    RoffInlineEvent::Font(_)
                    | RoffInlineEvent::PreviousFont
                    | RoffInlineEvent::Link(_)
                    | RoffInlineEvent::NoSpace => 0,
                    _ => 1,
                };
                acc.checked_add(added)
            }) else {
                return false;
            };
            prefix.start == 0
                && prefix.raw_slot_count().saturating_add(incoming)
                    <= crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS
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

fn word_can_consume_projected_blank(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    // encode1() consumes the generated NBSP's BACKBEFORE even for a
    // plain text operand (term.c:903-929). Keep the resulting backline
    // and decoration addressable to later words in the same field.
    let [RoffInlineEvent::Text(text), tail @ ..] = events else {
        return false;
    };
    // ESCAPE_IGNORE appends one ASCII_NBRZW using bufferc(), without
    // consuming either back flag (term.c:799–801, 860–873). Admit it
    // only after the graph word has performed the proved retreat.
    if !tail
        .iter()
        .all(|event| matches!(event, RoffInlineEvent::ZeroWidthGlyph))
    {
        return false;
    }
    context.zero_advance.can_replay_projected_prior_slot()
        && context.slot_entry.prefix.as_ref().is_some_and(|prefix| {
            prefix.start == 0
                && prefix.pass == ReplayPass::Editable
                && prefix
                    .raw_slot_count()
                    .saturating_add(text.len().saturating_mul(5))
                    .saturating_add(tail.len())
                    .saturating_add(1)
                    <= prefix.slot_limit
        })
        && text
            .bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        && text.bytes().any(|byte| byte.is_ascii_graphic())
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

/// A tight word uses the existing cursor and BACK flags. In particular,
/// `termp_ap_pre()` calls `term_word()` in the current font, and `encode1()`
/// decorates the glyph before moving BACKAFTER to BACKBEFORE. A subsequent
/// filled source line can append TABREF without settling either flag
/// (`mdoc_term.c:314–323`, 1836–1842 and `term.c:873–878`, 903–929).
fn tight_run_in_word_can_replay(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    let [RoffInlineEvent::Text(text)] = events else {
        return false;
    };
    let entry = &context.slot_entry;
    entry.projection_owner == FieldProjectionOwner::RunInHead
        && entry.domain == ReplayDomain::DefinitionLine
        && entry.automatic_blanks == 0
        && !context.zero_advance.has_annotation_output_owner()
        && !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_graphic())
        && entry.prefix.as_ref().is_some_and(|prefix| {
            (prefix.start == 0 || prefix.import == PrefixImport::RunInTail)
                && prefix.cursor_retreat == 0
                && prefix.backafter == context.zero_advance.is_armed()
                && (prefix.backafter && !prefix.backbefore
                    || prefix.backbefore && context.zero_advance.can_project_prior_slot())
                && prefix
                    .raw_slot_count()
                    .saturating_add(text.len().saturating_mul(5))
                    .saturating_add(usize::from(prefix.backbefore))
                    <= prefix.slot_limit
        })
}

#[derive(Clone, Copy)]
enum ReadingGlyph<'a> {
    Scalar(char),
    Spelling(&'a str),
    Earlier(usize),
    EntryBlank,
    DeviceAdvance,
    DeferredBlank {
        source: crate::mandoc::inline::flow::field_buffer::DeferredSourceBlank,
        imported_cell: Option<usize>,
    },
}

impl ReadingGlyph<'_> {
    fn append(self, text: &mut String) {
        match self {
            Self::Scalar(value) => text.push(value),
            Self::Spelling(value) => text.push_str(value),
            Self::Earlier(_)
            | Self::EntryBlank
            | Self::DeviceAdvance
            | Self::DeferredBlank { .. } => {}
        }
    }

    fn scalars(self) -> usize {
        match self {
            Self::Scalar(_) | Self::EntryBlank => 1,
            Self::Spelling(text) => text.chars().count(),
            Self::Earlier(count) => count,
            Self::DeviceAdvance | Self::DeferredBlank { .. } => 0,
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
    drew_space: bool,
    reposition: Option<CursorReposition>,
    retreated: bool,
    pre_column_retreat_units: usize,
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
            drew_space: false,
            reposition: None,
            retreated: false,
            pre_column_retreat_units: 0,
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
        if !matches!(
            event,
            RoffInlineEvent::NoSpace | RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont
        ) {
            self.reposition = None;
        }
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
                let effect = self.slots.motion(argument.as_deref(), owner);
                self.reposition = effect.reposition;
                self.retreated |= effect.reposition.is_some();
                if effect.before_origin_units > 0 {
                    self.pre_column_retreat_units = effect.before_origin_units;
                }
            }
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                argument: Some(argument),
            } => {
                let owner =
                    include_advances.then(|| self.mark(ReadingGlyph::DeviceAdvance, display));
                self.drew_space |= self.slots.line(argument, line_width_units, font, owner);
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
                // term.c:724-764,946-955: a drawn SP can admit a pass
                // without ink, then HORIZ can rewind SP onto a decorated
                // byte. Keep the explicit reading word boundary in that
                // drawing projection even when a device consumes the SP.
                if matches!(self.marker_boundary, MarkerBoundary::Pending)
                    && (self.slots.cursor_at_blank()
                        || self.drew_space && self.retreated && self.slots.cursor_before_end())
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
    prefix_origin: (usize, PrefixImport),
    earlier: usize,
    prior_pending: Option<PriorPendingSlot>,
    advance_marker: Option<usize>,
    editable_pass: bool,
    slot_limit: usize,
}

#[derive(Clone, Copy)]
enum PriorPendingSlot {
    Buffered(usize),
    Projected(usize),
}

impl PriorPendingSlot {
    fn at_entry(slot: Option<usize>, state: &super::ZeroAdvanceState) -> Option<Self> {
        // A completed BACKBEFORE glyph still occupies `slot` even when a
        // subsequent `\z` has armed BACKAFTER (term.c:662-682, 701-705, 924-927).
        slot.filter(|_| state.has_buffered_glyph()).map(|slot| {
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
                prefix_origin: (prefix.start, prefix.import),
                earlier,
                prior_pending: PriorPendingSlot::at_entry(
                    prefix.pending_cell,
                    context.zero_advance,
                ),
                replace_tail: 0,
                replayed_entry: false,
                advance_marker,
                editable_pass: prefix.pass == ReplayPass::Editable,
                slot_limit: prefix.slot_limit,
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
                prefix_origin: (0, PrefixImport::Regular),
                earlier: 0,
                prior_pending: None,
                advance_marker: None,
                editable_pass: false,
                slot_limit: crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS,
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
                prefix_origin: (0, PrefixImport::Regular),
                earlier: 0,
                prior_pending: None,
                advance_marker: None,
                editable_pass: false,
                slot_limit: crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS,
            }
        };
        seed.placement
            .slots
            .prepend_fixed_blanks(context.table_leading_blanks);
        seed
    }
}

struct SeedPlan {
    existing_tail: bool,
    deferred_trailing: bool,
    tracks_origin_retreat: bool,
    marker: Option<MarkerReplay>,
}

/// Execute `-inset`/`-diag` BODY pre's generated `\ ` / `\ \ ` word
/// (`mdoc_term.c:757-772, 939-945`, `chars.c:50`, `term.c:620-636, 888-930`)
/// through `NativeSlots::with_field_prefix` and `PlacedWord::prepare`.
///
/// In UTF-8, `\ ` calls `encode1(p, 0x00A0)`, which unconditionally overwrites
/// `buf[p->col]` (including bold copies, `\b`, owner glyphs, blanks, and
/// controls), transitions `TERMP_BACKAFTER -> TERMP_BACKBEFORE`, and consumes
/// `TERMP_BACKBEFORE` via `\b` or blank retreat.
pub(in crate::mandoc::inline) fn replay_run_in_word(
    prefix: &crate::mandoc::inline::flow::native_slots::FieldPrefix,
    (count, native_count): (usize, usize),
    owner_serial: u64,
    (display_font, native_font): (Font, RoffFont),
    replay_scope: (ReplayDomain, FieldProjectionOwner),
    zero_advance: &mut super::ZeroAdvanceState,
    (projected, pending_head_ink): (&mut Vec<mant_ir::Inline>, &mut Vec<mant_ir::Inline>),
) -> Option<Vec<FieldWrite>> {
    use crate::mandoc::inline::flow::field_buffer::DeferredSourceBlank;

    let mut placement = Placement::empty(NativeSlots::with_field_prefix(prefix));
    let earlier = prefix.cells.len();
    for (_, scalars) in &prefix.cells {
        placement.mark(ReadingGlyph::Earlier(*scalars), Font::Regular);
    }
    let projected_start = native_count.saturating_sub(count);
    // A fixed blank following a proved decorated origin can remain editable
    // with both native copies of its HEAD. Do not fence the first BODY word
    // at the generated blank when no register or previous pass intervened.
    let replayable_tail = prefix.start == 0
        && !prefix.backbefore
        && !prefix.backafter
        && prefix.cursor_retreat == 0
        && (!prefix.tail_fonts.is_empty() || !prefix.duplicates.is_empty());
    for index in 0..native_count {
        let owner = (index >= projected_start).then(|| {
            placement.mark(
                ReadingGlyph::DeferredBlank {
                    source: DeferredSourceBlank {
                        owner: owner_serial,
                        write: index,
                        font: display_font,
                        replayable_tail,
                        generated_run_in: true,
                    },
                    imported_cell: None,
                },
                display_font,
            )
        });
        placement.slots.native_write(
            FieldWrite::Cell(FieldCell::Graph {
                text: '\u{a0}',
                width: 1,
            }),
            native_font,
            owner,
        );
        if placement.slots.prior_edit().is_some() {
            return None;
        }
    }
    let (extent, _) = placement.slots.extent_and_retreat();
    if extent > prefix.slot_limit {
        return None;
    }
    let editable_marker = placement.slots.has_break_marker() || placement.slots.cursor_before_end();
    let seed = GeometrySeed {
        placement,
        replace_tail: 0,
        replayed_entry: false,
        prefix_origin: (prefix.start, prefix.import),
        earlier,
        prior_pending: PriorPendingSlot::at_entry(prefix.pending_cell, zero_advance),
        advance_marker: None,
        editable_pass: prefix.pass == ReplayPass::Editable,
        slot_limit: prefix.slot_limit,
    };
    let placed = PlacedWord::prepare(
        seed,
        true,
        AdvanceReading::Unprojected,
        editable_marker,
        replay_scope,
        Some(owner_serial),
    )
    .ok()?;
    Some(placed.project_run_in(zero_advance, projected, pending_head_ink))
}

pub(super) fn try_execute(
    events: &[RoffInlineEvent],
    context: &mut TextExecutionContext<'_>,
    word: &mut WordExecution,
) -> Result<(), PriorSlotEdit> {
    let existing_tail = can_defer_live_advances(events, context);
    let plan = SeedPlan {
        existing_tail,
        deferred_trailing: !existing_tail && can_defer_trailing_advance(events, context),
        tracks_origin_retreat: marked_operands::tracks_literal_control_retreat(events, context),
        marker: events
            .iter()
            .any(|event| matches!(event, RoffInlineEvent::LineBreak))
            .then(|| marker_replay(events, context))
            .flatten(),
    };
    let seed = GeometrySeed::begin(context, events);
    execute_seed(seed, events, context, word, &plan)
}

fn execute_seed<'a>(
    mut seed: GeometrySeed<'a>,
    events: &'a [RoffInlineEvent],
    context: &mut TextExecutionContext<'_>,
    word: &mut WordExecution,
    plan: &SeedPlan,
) -> Result<(), PriorSlotEdit> {
    // Font and reading owners only commit after all addressed bytes can be
    // placed. Falling back to the live word executor must not run them twice.
    let mut font = context.font.clone();
    let mut continuation = None;
    let advances = AdvanceReading::for_seed(events, context, &seed, plan);
    if context.zero_advance.is_armed() {
        seed.placement.slots.arm_backafter();
    }
    for (index, event) in events.iter().enumerate() {
        if matches!(event, RoffInlineEvent::NoSpace) && index + 1 == events.len() {
            continuation = Some(!seed.placement.slots.backafter());
        }
        seed.placement.input(
            event,
            &mut font,
            context.line_width_units,
            !matches!(advances, AdvanceReading::Unprojected),
        );
        if let Some(edit) = seed.placement.slots.prior_edit() {
            return Err(edit);
        }
    }
    let has_marker = has_word_end_marker(events);
    let (retain_suffix, editable_marker) =
        seed_retention_flags(&seed, events, context, plan, advances, has_marker);
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
    let pre_column_retreat = seed.placement.pre_column_retreat_units;
    let placed = PlacedWord::prepare(
        seed,
        retain_suffix,
        advances,
        editable_marker,
        (
            context.slot_entry.domain,
            context.slot_entry.projection_owner,
        ),
        context.zero_advance.native_word_owner(),
    )?;
    *context.font = font;
    context.zero_advance.take_armed();
    word.source_continuation = continuation;
    if plan.tracks_origin_retreat {
        word.native_origin_retreat_units = pre_column_retreat;
    }
    placed.project(context, word);
    Ok(())
}

fn seed_retention_flags(
    seed: &GeometrySeed<'_>,
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
    plan: &SeedPlan,
    advances: AdvanceReading,
    has_marker: bool,
) -> (bool, bool) {
    // The bounded overlay knows both col and lastcol. A marker in this
    // operand does not reset that cursor (term.c:659-660,678-714).
    // Carry the exact cursor if the next word must still write there.
    let slots = &seed.placement.slots;
    let has_surviving_marker = slots.has_break_marker();
    let extent = slots.extent_and_retreat().0;
    let bounded_complete_field =
        covers_field_start(seed, context.slot_entry.cursor) && extent <= seed.slot_limit;
    // Fresh bounded Filled, DefinitionLine, and Scoped fields share the same
    // live `tcol->buf` before `term_flushln()` (term.c:659-660, 678-718;
    // mdoc_term.c:760-838). LiteralDefinitionLine stays excluded.
    let bounded_editable_domain = matches!(
        context.slot_entry.domain,
        ReplayDomain::Filled | ReplayDomain::DefinitionLine | ReplayDomain::Scoped
    );
    let bounded_edited_field = bounded_complete_field
        && bounded_editable_domain
        && (seed.placement.retreated
            || !context.slot_entry.cursor.has_input()
                && !events
                    .iter()
                    .any(|event| matches!(event, RoffInlineEvent::Breakpoint))
                && local_overwrite(events, context) == Some(LocalOverwrite::Covered));
    // A marker after new ink leaves an open cursor at the end of that
    // native segment, even when the old tail still holds glyphs.
    let live_cursor = retains_word_cursor(seed, &context.slot_entry, events);
    // A placed glyph can still be waiting under BACKBEFORE after a marker.
    // Keep its raw decoration and the invisible label slots around it, so
    // the next word can revisit the same native suffix (term.c::encode1).
    let pending_glyph = slots.pending_owner().is_some();
    // term_word() writes a marker at col. An unpainted native suffix can
    // carry it through continuation, motion or a second break control.
    // Keep its slots editable until term_fill() accepts a pass.
    let terminal_marker =
        has_terminal_marker(seed.earlier, slots, events, context.zero_advance.is_armed());
    let bounded_inner_definition_marker = has_marker
        && matches!(advances, AdvanceReading::Inner)
        && context.slot_entry.domain == ReplayDomain::DefinitionLine
        && is_fresh_word_entry(context)
        && extent <= seed.slot_limit;
    let retain_suffix = context.policy.field_authoritative
        && (!has_marker
            || !has_surviving_marker && bounded_complete_field
            || bounded_edited_field
            || plan.marker.is_some()
            || live_cursor
            || pending_glyph
            || terminal_marker
            || seed.editable_pass
            || plan.existing_tail
            || bounded_inner_definition_marker
            || matches!(advances, AdvanceReading::Inner)
                && context.slot_entry.domain == ReplayDomain::Filled
                && slots.has_unpainted_marker_tail()
                && extent <= seed.slot_limit);
    let editable_marker = matches!(plan.marker, Some(MarkerReplay::AwaitFieldFlush))
        || matches!(plan.marker, Some(MarkerReplay::PreserveSlots))
            && has_surviving_marker
            && bounded_complete_field
            && context.slot_entry.domain == ReplayDomain::Filled
        || bounded_inner_definition_marker
        || bounded_edited_field && has_surviving_marker
        || has_marker && seed.editable_pass && slots.cursor_before_end()
        || (has_marker || seed.earlier != 0 && has_surviving_marker) && live_cursor
        || has_marker && plan.existing_tail
        // BACKBEFORE can hold a glyph inside material still following
        // col. Leave the marker editable until a real device flush.
        || has_marker && pending_glyph && seed.earlier != 0 && slots.cursor_before_end()
        // term_fill() advances past buffered blanks before the next graph.
        // Leave that separator to the real flush while keeping the raw
        // cursor and its owners available to intervening native writes.
        || terminal_marker && seed.placement.marker_after_blank;
    (retain_suffix, editable_marker)
}

fn has_terminal_marker(
    earlier: usize,
    slots: &NativeSlots,
    events: &[RoffInlineEvent],
    inherited_arm: bool,
) -> bool {
    earlier != 0
        && slots.has_unpainted_marker_tail()
        && events
            .iter()
            .rposition(|event| matches!(event, RoffInlineEvent::LineBreak))
            .is_some_and(|marker| {
                // HORIZ may traverse an occupied slot without writing it
                // (term.c::bufferc). Extend the cursor proof only for a
                // marker actually reached after writing fixed advances, while
                // tracking BACKAFTER cancellation of HORIZ/NoSpace (term.c:662-682).
                let mut armed = inherited_arm;
                events[..marker].iter().all(|event| match event {
                    RoffInlineEvent::ZeroAdvance => {
                        armed = true;
                        true
                    }
                    RoffInlineEvent::NoSpace => {
                        armed = false;
                        true
                    }
                    RoffInlineEvent::HorizontalMotion { argument, .. } => {
                        if std::mem::take(&mut armed) {
                            true
                        } else {
                            match argument.as_deref().and_then(motion_columns) {
                                Some((false, 0)) => true,
                                Some((false, units)) if units > 0 => {
                                    slots.trailing_advance_marker(0).is_some()
                                }
                                _ => false,
                            }
                        }
                    }
                    _ => true,
                })
            })
}

/// Carry `p->col < p->tcol->lastcol` across `term_word()` boundaries
/// (`term.c:659-660, 678-718, 860-872`) while the field stays un-flushed.
fn retains_word_cursor(
    seed: &GeometrySeed<'_>,
    entry: &SlotEntry,
    events: &[RoffInlineEvent],
) -> bool {
    let slots = &seed.placement.slots;
    slots.cursor_before_end()
        && (seed.prefix_origin.0 > 0
            // An initial control-only word can leave the cursor before
            // a marker. The next word must be able to overwrite it.
            || !entry.cursor.has_input()
                && slots.cursor_before_invisible_tail()
                && !slots.contains_ink_from(0)
            // term_word() keeps the marked word's trailing raw advancement
            // when HORIZ returns into it. The next word still writes
            // inside those fixed cells before term_fill() sees the marker.
            || covers_field_start(seed, entry.cursor)
                && slots.extent_and_retreat().0
                    <= seed.slot_limit
                && slots.cursor_in_unread_advance_after_marker()
            || retains_repositioned_word_cursor(seed, entry, events)
            || matches!(
                entry.domain,
                ReplayDomain::Filled | ReplayDomain::DefinitionLine | ReplayDomain::Scoped
            ) && covers_field_start(seed, entry.cursor)
                && slots.extent_and_retreat().0
                    <= seed.slot_limit
                && seed.placement.retreated
            || seed.earlier != 0
                && (slots.cursor_before_invisible_tail()
                    || slots.has_unpainted_marker_tail()
                        && !events.iter().any(absolute_motion)
                        && slots.contains_ink_from(seed.earlier)
                    || slots_rewritten_by_controls(events)))
}

fn covers_field_start(seed: &GeometrySeed<'_>, entry: EntryCursor) -> bool {
    // An exact prefix starting at col zero and a fresh field both own
    // their origin. Missing prefix evidence with prior input stays opaque.
    seed.prefix_origin.0 == 0
        && (seed.earlier != 0
            || !entry.has_input() && seed.placement.slots.extent_and_retreat().0 <= seed.slot_limit)
}

fn retains_repositioned_word_cursor(
    seed: &GeometrySeed<'_>,
    entry: &SlotEntry,
    events: &[RoffInlineEvent],
) -> bool {
    // An executed HORIZ in a complete field moves col without discarding
    // later slots. In filled text, a retreat into the raw operand also
    // leaves its subsequent markers available to the next term_word()
    // until term_fill() runs (term.c:659–660, 678–714).
    let slots = &seed.placement.slots;
    covers_field_start(seed, entry.cursor)
        && slots.extent_and_retreat().0 <= seed.slot_limit
        && slots.cursor_before_owner()
        && slots.contains_ink_from(seed.earlier)
        && match seed.placement.reposition {
            Some(CursorReposition::Absolute) => true,
            Some(CursorReposition::RelativeRetreat) => {
                matches!(
                    entry.domain,
                    ReplayDomain::Filled | ReplayDomain::DefinitionLine | ReplayDomain::Scoped
                )
                    // A previously relocated cursor can have rewritten the
                    // span before these markers. Forward writes leave the
                    // earlier slots in place (term.c::bufferc). When a live
                    // prefix already spans the field start, carry the final
                    // relative cursor just as a partial suffix replay does.
                    && (seed.earlier != 0
                        || events
                            .iter()
                            .filter(|event| {
                                matches!(event, RoffInlineEvent::HorizontalMotion { argument: Some(value), .. }
                                    if motion_columns(value).is_some_and(|(absolute, units)| absolute || units < 0))
                            })
                            .nth(1)
                            .is_none())
            }
            None => false,
        }
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
            // When col < lastcol, bufferc(' ') advances col without
            // overwriting existing cells (term.c:868-871).
            RoffInlineEvent::Text(value) if marker && value.bytes().all(|byte| byte == b' ') => {}
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
