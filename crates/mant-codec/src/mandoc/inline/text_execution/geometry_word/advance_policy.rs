//! Decide which native horizontal advances should survive as reading content.

use super::{
    EntryCursor, FieldCell, GeometrySeed, GlyphInput, LocalOverwrite, MarkerReplay,
    PresentationKind, PriorPendingSlot, ReplayDomain, ReplayPass, RoffFont, RoffInlineEvent,
    SeedPlan, TextExecutionContext, absolute_motion, can_place_pending, fits_bounded_filled_prefix,
    is_filled_line_domain, is_fresh_word_entry, local_overwrite, motion_columns,
};
use crate::mandoc::inline::flow::native_slots::DeferredTail;

impl AdvanceReading {
    pub(super) fn for_seed(
        events: &[RoffInlineEvent],
        context: &TextExecutionContext<'_>,
        seed: &GeometrySeed<'_>,
        plan: &SeedPlan,
    ) -> Self {
        let bounded_marked_advances = matches!(
            plan.marker,
            Some(MarkerReplay::AwaitFieldFlush | MarkerReplay::PreserveSlots)
        ) && context.slot_entry.domain == ReplayDomain::Filled
            && !context.zero_advance.is_armed()
            && !context.zero_advance.has_pending_glyph()
            && !context.zero_advance.has_annotation_output_owner()
            && events.iter().any(|event| {
                matches!(
                    event,
                    RoffInlineEvent::HorizontalMotion {
                        argument: Some(value),
                        ..
                    } if motion_columns(value).is_some_and(|(absolute, units)| !absolute && units > 0)
                )
            });
        if can_publish_inner_advances(events, context)
            || can_publish_retained_advances(events, context, seed)
            || can_defer_retreated_advances(events, context)
            || bounded_marked_advances
        {
            Self::Inner
        } else if plan.existing_tail {
            Self::DeferredBlankTail
        } else if plan.deferred_trailing {
            Self::DeferredTail
        } else {
            Self::Unprojected
        }
    }
}

pub(super) fn can_place_relative_advance(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    can_publish_inner_advances(events, context)
        || appends_held_nbsp_before_authored_blank(events, context)
        || can_defer_retreated_advances(events, context)
        || can_defer_trailing_advance(events, context)
        || can_defer_live_advances(events, context)
        || can_place_leading_advance(events, context)
}

fn appends_held_nbsp_before_authored_blank(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    // An armed fixed blank writes a raw scalar before the HORIZ clears
    // BACKBEFORE. The following authored SP occupies its own editable
    // cell, even though neither is graph ink (term.c:691–699, 903–929).
    context.slot_entry.domain == ReplayDomain::Filled
        && context.slot_entry.cursor == EntryCursor::AtEnd
        && !context.zero_advance.is_armed()
        && !context.zero_advance.has_pending_glyph()
        && !context.zero_advance.has_annotation_output_owner()
        && matches!(events, [
            RoffInlineEvent::ZeroAdvance,
            RoffInlineEvent::Glyph(text, GlyphInput::Mapped),
            RoffInlineEvent::HorizontalMotion { argument: Some(value), .. },
            RoffInlineEvent::Text(tail),
        ] if text == "\u{a0}"
            && motion_columns(value).is_some_and(|(absolute, units)| !absolute && units > 0 && units <= 3 * 24)
            && (1..=4).contains(&tail.len()) && tail.bytes().all(|b| b == b' '))
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
    scan_inner_advances(events, context)
}

fn scan_inner_advances(events: &[RoffInlineEvent], context: &TextExecutionContext<'_>) -> bool {
    let mut ink = false;
    let mut advance = false;
    let mut followed = false;
    let mut armed_local = false;
    let mut prior_zero_advance = false;
    let mut post_advance_backbefore = false;
    let mut covered_pair = false;
    for (index, event) in events.iter().enumerate() {
        match event {
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _) => {
                let printable = text
                    .chars()
                    .any(|ch| (!ch.is_whitespace() || ch == '\u{a0}') && !ch.is_control());
                if printable {
                    if post_advance_backbefore {
                        return false;
                    }
                    if std::mem::take(&mut armed_local) {
                        if advance {
                            if text.chars().count() != 1 {
                                return false;
                            }
                            post_advance_backbefore = true;
                        } else {
                            prior_zero_advance = true;
                        }
                    }
                }
                followed |= advance && printable;
                ink |= printable;
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if motion_columns(value).is_some_and(|(absolute, units)| absolute || units > 0) => {
                if std::mem::take(&mut armed_local) {
                    // term.c:678-682: ESCAPE_HORIZ clears BACKAFTER without moving.
                    continue;
                }
                if absolute_motion(event) {
                    // A final absolute can reposition col without erasing the
                    // earlier accepted inter-glyph advance. Only the complete
                    // fresh field proves the origin for that combined replay.
                    let complete = !context.slot_entry.cursor.has_input()
                        && context.slot_entry.automatic_blanks == 0;
                    if prior_zero_advance
                        || post_advance_backbefore
                        || covered_pair && !(complete && followed && index + 1 == events.len())
                    {
                        return false;
                    }
                } else if std::mem::take(&mut post_advance_backbefore) {
                    // term.c:692-698: the first unit of a positive relative
                    // HORIZ clears BACKBEFORE before any remaining cells write.
                    advance |= ink;
                    continue;
                }
                if std::mem::take(&mut prior_zero_advance) {
                    let before = &events[..index];
                    covered_pair =
                        local_overwrite(before, context) == Some(LocalOverwrite::Covered);
                    if !covered_pair && !holds_backbefore_on_local_blank(before, context) {
                        return false;
                    }
                    covered_pair = true;
                }
                advance |= ink;
            }
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } if !armed_local
                && !post_advance_backbefore
                && motion_columns(value).is_some_and(|(absolute, units)| {
                    !absolute
                        && units < 0
                        && (followed
                            || advance
                                && !prior_zero_advance
                                && retreats_inside_fresh_advance(context, &events[..index], units))
                }) => {}
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                argument: Some(value),
            } if value.ends_with(' ') && !post_advance_backbefore => {
                advance |= ink;
            }
            RoffInlineEvent::NoSpace => armed_local = false,
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint => {}
            RoffInlineEvent::ZeroAdvance if !post_advance_backbefore => armed_local = true,
            RoffInlineEvent::LineBreak
                if matches!(
                    context.slot_entry.domain,
                    ReplayDomain::Filled | ReplayDomain::DefinitionLine
                ) => {}
            _ => return false,
        }
    }
    followed && !armed_local && !post_advance_backbefore
}

fn retreats_inside_fresh_advance(
    context: &TextExecutionContext<'_>,
    before: &[RoffInlineEvent],
    retreat: i32,
) -> bool {
    // term.c:691–718: a retreat shorter than the immediately preceding
    // relative advance stays inside the newly written NBRSP run. Native
    // placement can keep both the inner advance and the trailing cursor
    // even when the first post-advance glyph has not arrived yet.
    is_fresh_word_entry(context)
        && matches!(before.last(), Some(RoffInlineEvent::HorizontalMotion { argument: Some(amount), .. })
            if motion_columns(amount).is_some_and(|(absolute, units)| !absolute && units > retreat.saturating_abs()))
}

fn holds_backbefore_on_local_blank(
    before: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    // encode() does not consume BACKBEFORE when it writes an ordinary
    // space after the one armed graph. The first unit of a positive HORIZ
    // clears BACKBEFORE, and every later unit occupies its own NBRSP slot
    // (term.c:691–699, 903–909 and 946–955).
    context.slot_entry.domain == ReplayDomain::Filled
        && is_fresh_word_entry(context)
        && matches!(before, [RoffInlineEvent::ZeroAdvance, RoffInlineEvent::Text(text)]
            if matches!(text.as_bytes(), [first, rest @ ..]
                if first.is_ascii_graphic() && !rest.is_empty() && rest.iter().all(|&byte| byte == b' ')))
}

fn can_defer_retreated_advances(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    // A word entering geometry replay with a retreated cursor (col < lastcol)
    // or a bounded editable Filled prefix may still append positive HORIZ cells
    // after ink (`term.c:692-698`). Keep those trailing advances owned as
    // deferred blanks so `term_field()` releases them before later ink.
    let editable_prefix = context.slot_entry.prefix.as_ref().is_some_and(|prefix| {
        prefix.start == 0
            && prefix.pass == ReplayPass::Editable
            && !prefix.backafter
            && fits_bounded_filled_prefix(events, context)
    });
    let cursor_eligible = context.slot_entry.cursor == EntryCursor::NeedsReplay || editable_prefix;
    let pending_ok = !context.zero_advance.has_pending_glyph()
        || context
            .slot_entry
            .prefix
            .as_ref()
            .is_some_and(|prefix| !prefix.backafter)
            && can_place_pending(context);
    if !context.policy.field_authoritative
        || !is_filled_line_domain(context.slot_entry.domain)
        || !cursor_eligible
        || context.zero_advance.is_armed()
        || !pending_ok
        || context.zero_advance.has_annotation_output_owner()
        || context.zero_advance.native_word_owner().is_none()
    {
        return false;
    }
    let mut ink = false;
    let mut advance = false;
    let mut armed_local = false;
    let mut backbefore_local = false;
    for event in events {
        match event {
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _) => {
                for ch in text.chars() {
                    if ch.is_whitespace() && ch != '\u{a0}' {
                        if editable_prefix && ch == ' ' {
                            continue;
                        }
                        return false;
                    }
                    if !ch.is_control() {
                        // On a retreated cursor (`cursor_retreat > 0`), only
                        // defer trailing positive HORIZ after the final ink; an
                        // inner advance can traverse earlier slots without
                        // DeviceAdvance owners (`term.c:691-699, 868`).
                        if advance
                            && context
                                .slot_entry
                                .prefix
                                .as_ref()
                                .is_some_and(|prefix| prefix.cursor_retreat > 0)
                        {
                            return false;
                        }
                        backbefore_local = false;
                        if std::mem::take(&mut armed_local) {
                            backbefore_local = true;
                        }
                        ink = true;
                    }
                }
            }
            RoffInlineEvent::ZeroAdvance => armed_local = true,
            RoffInlineEvent::NoSpace => armed_local = false,
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } => {
                let Some((false, units)) = motion_columns(value) else {
                    return false;
                };
                if units < 0 {
                    return false;
                }
                if std::mem::take(&mut armed_local) || units == 0 {
                    continue;
                }
                if !ink {
                    return false;
                }
                // term.c:694-698: the first EN (24 basic units) clears
                // BACKBEFORE; only remaining positive units write NBRSP.
                if std::mem::take(&mut backbefore_local) {
                    if units > 24 {
                        advance = true;
                    }
                } else {
                    advance = true;
                }
            }
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint
            | RoffInlineEvent::LineBreak => {}
            _ => return false,
        }
    }
    advance && !armed_local && (!backbefore_local || editable_prefix)
}

fn deferred_trailing_entry_slots(context: &TextExecutionContext<'_>) -> Option<usize> {
    if !is_filled_line_domain(context.slot_entry.domain)
        || context.zero_advance.is_armed()
        || context.zero_advance.has_pending_glyph()
        || context.defer_leading_blanks
        || context.zero_advance.has_annotation_output_owner()
        || context.zero_advance.native_word_owner().is_none()
    {
        return None;
    }
    if !context.slot_entry.cursor.has_input() {
        return Some(0);
    }
    context
        .slot_entry
        .prefix
        .as_ref()
        .filter(|prefix| {
            let holds_pending =
                prefix.backbefore && prefix.automatic == 0 && prefix.pending_cell.is_some();
            prefix.start == 0
                && !prefix.backafter
                && !holds_pending
                && !prefix
                    .cells
                    .iter()
                    .any(|(cell, _)| *cell == FieldCell::BreakMarker)
        })
        .map(crate::mandoc::inline::flow::native_slots::FieldPrefix::raw_slot_count)
}

struct DeferredTrailingScan {
    size: usize,
    has_space: bool,
    has_inner_motion: bool,
    has_zero_advance: bool,
}

fn scan_deferred_trailing_prefix(
    prefix: &[RoffInlineEvent],
    leading_retreat: bool,
) -> Option<(DeferredTrailingScan, bool)> {
    let single_paint = |font| {
        !matches!(
            font,
            RoffFont::Strong | RoffFont::CodeStrong | RoffFont::StrongEmphasis
        )
    };
    let mut armed_local = false;
    let mut has_space = false;
    let mut has_inner_motion = false;
    let mut has_zero_advance = false;
    let size = prefix.iter().try_fold(0usize, |count, event| match event {
        RoffInlineEvent::Text(text)
            if text
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || !leading_retreat && byte == b' ') =>
        {
            has_space |= text.bytes().any(|byte| byte == b' ');
            if text.bytes().any(|byte| byte.is_ascii_graphic()) {
                armed_local = false;
            }
            Some(count.saturating_add(text.len().saturating_mul(5)))
        }
        // `\~` (U+00A0) encodes as a decorated glyph and clears BACKAFTER
        // (term.c:915-929).
        RoffInlineEvent::Glyph(text, _) if text == "\u{a0}" => {
            armed_local = false;
            Some(count.saturating_add(5))
        }
        RoffInlineEvent::ZeroAdvance if !leading_retreat && !armed_local => {
            armed_local = true;
            has_zero_advance = true;
            Some(count.saturating_add(1))
        }
        RoffInlineEvent::NoSpace => {
            armed_local = false;
            Some(count)
        }
        // HORIZ under BACKAFTER clears BACKAFTER without buffering bytes
        // (term.c:678-682).
        RoffInlineEvent::HorizontalMotion {
            argument: Some(value),
            ..
        } if armed_local && motion_columns(value).is_some() => {
            armed_local = false;
            Some(count)
        }
        RoffInlineEvent::HorizontalMotion {
            argument: Some(value),
            ..
        } if count > 0
            && !armed_local
            && motion_columns(value).is_some_and(|(absolute, units)| !absolute && units > 0) =>
        {
            has_inner_motion = true;
            Some(
                count.saturating_add(libmandoc_rs::TableColumnMetrics::relative_motion_columns(
                    value,
                )),
            )
        }
        // Keep the ordinary receipt when the ink itself can have multiple
        // raw paint copies with independent coverage ownership.
        RoffInlineEvent::Font(font) if single_paint(*font) => Some(count),
        // Font selection and device links do not advance the native column;
        // the ignorable break and zero-width controls may occupy one raw byte
        // without painting or disarming BACKAFTER (term.c:632-660, 724-836).
        RoffInlineEvent::PreviousFont | RoffInlineEvent::Link(_) => Some(count),
        RoffInlineEvent::Breakpoint | RoffInlineEvent::ZeroWidthGlyph => {
            Some(count.saturating_add(1))
        }
        // term.c::encode() treats ASCII_HYPH as glyph input.
        RoffInlineEvent::BreakableHyphen => {
            armed_local = false;
            Some(count.saturating_add(5))
        }
        _ => None,
    })?;
    Some((
        DeferredTrailingScan {
            size,
            has_space,
            has_inner_motion,
            has_zero_advance,
        },
        armed_local,
    ))
}

/// Keep bounded words ending in positive `HORIZ` in raw slots (`term.c:678-718,
/// 915-929`) so trailing `ASCII_NBRSP` advances stay editable until `term_fill()`.
pub(super) fn can_defer_trailing_advance(
    mut events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;

    let Some(earlier) = deferred_trailing_entry_slots(context) else {
        return false;
    };
    while let Some((RoffInlineEvent::NoSpace, rest)) = events.split_last() {
        events = rest;
    }
    let Some((
        RoffInlineEvent::HorizontalMotion {
            argument: Some(argument),
            ..
        },
        prefix,
    )) = events.split_last()
    else {
        return false;
    };
    let width = libmandoc_rs::TableColumnMetrics::relative_motion_columns(argument);
    let Some(budget @ 1..) = MAX_EDITABLE_APPEND_CELLS.checked_sub(earlier.saturating_add(width))
    else {
        return false;
    };
    // At a fresh field origin, a negative HORIZ changes the position of
    // the field but cannot consume any raw slots (term.c::term_word).
    // The later internal and trailing advances still occupy exact slots.
    let (leading_retreat, prefix) = if earlier == 0
        && let Some((
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            },
            rest,
        )) = prefix.split_first()
        && motion_columns(value).is_some_and(|(absolute, units)| !absolute && units < 0)
    {
        (true, rest)
    } else {
        (false, prefix)
    };
    let Some((
        DeferredTrailingScan {
            size,
            has_space,
            has_inner_motion,
            has_zero_advance,
        },
        armed_local,
    )) = scan_deferred_trailing_prefix(prefix, leading_retreat)
    else {
        return false;
    };
    let single_paint = |font| {
        !matches!(
            font,
            RoffFont::Strong | RoffFont::CodeStrong | RoffFont::StrongEmphasis
        )
    };
    let size = Some(size);
    // Bounded filled words with a trailing advance keep exact raw font
    // slots (and bold copy aliases, `term.c:915-925`) until a later HORIZ or
    // `term_fill()`; `size <= budget` bounds multi-glyph bold words.
    let bounded_bold = !leading_retreat && !armed_local;
    (leading_retreat
        || earlier > 0
        || width > 1
        || !single_paint(context.font.current) && bounded_bold
        || has_space
        || has_inner_motion
        || has_zero_advance)
        && !armed_local
        && width > 0
        && size.is_some_and(|count| count > 0 && count <= budget)
        && (single_paint(context.font.current) || bounded_bold)
}

fn can_place_leading_advance(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;

    if context.slot_entry.domain != ReplayDomain::Filled
        || context.slot_entry.cursor.has_input()
        || context.zero_advance.is_armed()
        || context.defer_leading_blanks
        || context.zero_advance.has_annotation_output_owner()
    {
        return false;
    }
    let Some((
        RoffInlineEvent::HorizontalMotion {
            argument: Some(argument),
            ..
        },
        rest,
    )) = events.split_first()
    else {
        return false;
    };
    let width = libmandoc_rs::TableColumnMetrics::relative_motion_columns(argument);
    let Some(budget @ 1..) = MAX_EDITABLE_APPEND_CELLS.checked_sub(width) else {
        return false;
    };
    let mut ink = false;
    let size = rest.iter().try_fold(0usize, |count, event| match event {
        RoffInlineEvent::Text(text)
            if text
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ') =>
        {
            ink |= text.bytes().any(|byte| byte.is_ascii_graphic());
            Some(count.saturating_add(text.len().saturating_mul(5)))
        }
        RoffInlineEvent::NoSpace => Some(count),
        RoffInlineEvent::Font(_)
        | RoffInlineEvent::PreviousFont
        | RoffInlineEvent::ZeroWidthGlyph
        | RoffInlineEvent::Breakpoint => Some(count.saturating_add(1)),
        _ => None,
    });
    width > 0 && ink && size.is_some_and(|count| count <= budget)
}

pub(super) fn can_defer_live_advances(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    can_defer_blank_tail_advances(events, context) || can_append_to_covered_tail(events, context)
}

fn can_defer_blank_tail_advances(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;

    let Some(prefix) = context.slot_entry.prefix.as_ref().filter(|prefix| {
        prefix.deferred_tail == DeferredTail::Replayable && prefix.tail_fonts.is_empty()
            || prefix.start == 0
                && !prefix.backbefore
                && !prefix.backafter
                && (prefix.pass == ReplayPass::Editable
                    || !prefix
                        .cells
                        .iter()
                        .any(|(cell, _)| *cell == FieldCell::BreakMarker))
    }) else {
        return false;
    };
    if context.slot_entry.domain != ReplayDomain::Filled
        || context.zero_advance.has_pending_glyph()
        || context.zero_advance.has_annotation_output_owner()
        || context.zero_advance.native_word_owner().is_none()
    {
        return false;
    }
    // No ink releases the pending advances in this word. Keep its
    // positive motion and ordinary SP in raw order until term_field()
    // encounters the next printable graph, after any live cursor edits.
    let earlier = prefix.raw_slot_count();
    let replayable_tail = prefix.deferred_tail == DeferredTail::Replayable;
    let mut armed = context.zero_advance.is_armed();
    let mut has_positive_motion = false;
    let cells = events.iter().try_fold(earlier, |cells, event| {
        let added = match event {
            RoffInlineEvent::ZeroAdvance => {
                armed = true;
                0
            }
            RoffInlineEvent::NoSpace => {
                armed = false;
                0
            }
            RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont => 0,
            RoffInlineEvent::HorizontalMotion {
                argument: Some(value),
                ..
            } => {
                let (false, units) = motion_columns(value)? else {
                    return None;
                };
                if std::mem::take(&mut armed) {
                    0
                } else if units > 0 {
                    has_positive_motion = true;
                    libmandoc_rs::TableColumnMetrics::relative_motion_columns(value)
                } else if units == 0 || replayable_tail {
                    0
                } else {
                    return None;
                }
            }
            RoffInlineEvent::Text(value) if value.bytes().all(|byte| byte == b' ') => value.len(),
            RoffInlineEvent::LineBreak => 1,
            _ => return None,
        };
        cells
            .checked_add(added)
            .filter(|&total| total <= MAX_EDITABLE_APPEND_CELLS)
    });
    has_positive_motion && cells.is_some()
}

fn can_append_to_covered_tail(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;
    let Some(prefix) = context.slot_entry.prefix.as_ref().filter(|prefix| {
        prefix.deferred_tail == DeferredTail::Covered
            && prefix.tail_fonts.is_empty()
            // An inherited native arm can write independently of the
            // incoming word's BACK controls (term.c::encode1).
            && !prefix.backbefore
            && !prefix.backafter
    }) else {
        return false;
    };
    if context.slot_entry.domain != ReplayDomain::Filled
        || context.zero_advance.has_annotation_output_owner()
        || context.zero_advance.has_pending_glyph()
        || context.zero_advance.is_armed()
        || context.zero_advance.native_word_owner().is_none()
        || !matches!(events.first(), Some(RoffInlineEvent::ZeroAdvance))
    {
        return false;
    }
    let mut armed = false;
    let mut advance = false;
    let cells = events
        .iter()
        .try_fold(prefix.raw_slot_count(), |cells, event| {
            let added = match event {
                RoffInlineEvent::ZeroAdvance => {
                    armed = true;
                    1
                }
                RoffInlineEvent::NoSpace => {
                    armed = false;
                    0
                }
                RoffInlineEvent::LineBreak => 1,
                RoffInlineEvent::Text(text) if text.bytes().all(|byte| byte.is_ascii_graphic()) => {
                    if !text.is_empty() {
                        armed = false;
                    }
                    text.len().saturating_mul(5)
                }
                RoffInlineEvent::HorizontalMotion {
                    argument: Some(value),
                    ..
                } => {
                    let (false, units) = motion_columns(value)? else {
                        return None;
                    };
                    if std::mem::take(&mut armed) {
                        0
                    } else if units > 0 {
                        advance = true;
                        libmandoc_rs::TableColumnMetrics::relative_motion_columns(value)
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            cells
                .checked_add(added)
                .filter(|&total| total <= MAX_EDITABLE_APPEND_CELLS)
        });
    advance && cells.is_some()
}

enum RetainedAdvancePhase {
    Inherited,
    Armed,
    PendingLocal,
    Settled,
    Advance,
    InkAfterAdvance,
}

#[derive(Clone, Copy)]
pub(super) enum AdvanceReading {
    Unprojected,
    Inner,
    DeferredTail,
    DeferredBlankTail,
}

fn has_retained_advance_entry(context: &TextExecutionContext<'_>, seed: &GeometrySeed<'_>) -> bool {
    let slots = &seed.placement.slots;
    match context.slot_entry.automatic_blanks {
        0 => {
            // Published ink can still leave native BACKBEFORE. Require
            // the complete slot proof when no pending reading owner remains.
            (!context.zero_advance.has_pending_glyph()
                || matches!(seed.prior_pending, Some(PriorPendingSlot::Buffered(_))))
                && slots.cursor_follows_retained_glyph()
        }
        1 => {
            !context.slot_entry.kept_blanks
                && !context.zero_advance.has_pending_glyph()
                && slots.cursor_follows_retained_glyph_and_blank()
        }
        _ => false,
    }
}

fn can_publish_retained_advances(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
    seed: &GeometrySeed<'_>,
) -> bool {
    use RetainedAdvancePhase::{Advance, Armed, Inherited, InkAfterAdvance, PendingLocal, Settled};

    if !context.policy.field_authoritative
        || context.slot_entry.domain != ReplayDomain::Filled
        || context.zero_advance.is_armed()
        || context.zero_advance.has_annotation_output_owner()
        || seed.prefix_start != 0
        || !seed.editable_pass
        || seed.placement.slots.backafter()
        || !has_retained_advance_entry(context, seed)
    {
        return false;
    }
    let mut phase = Inherited;
    for event in events {
        phase = match (phase, event) {
            (Inherited, RoffInlineEvent::ZeroAdvance) => Armed,
            // ESCAPE_NOSPACE cancels BACKAFTER before checking for the
            // end of the operand. The inherited BACKBEFORE stays live.
            (Armed, RoffInlineEvent::NoSpace) => Inherited,
            (
                state,
                RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, GlyphInput::Codepoint),
            ) if !text.is_empty()
                && text
                    .chars()
                    .all(super::super::native_writes::is_single_column_ink) =>
            {
                match state {
                    // encode1() first executes inherited BACKBEFORE, then
                    // the newly armed BACKAFTER leaves the local glyph held.
                    Armed if text.chars().nth(1).is_none() => PendingLocal,
                    Inherited | Armed | PendingLocal | Settled => Settled,
                    Advance | InkAfterAdvance => InkAfterAdvance,
                }
            }
            (
                state @ (PendingLocal | Settled | Advance | InkAfterAdvance),
                RoffInlineEvent::HorizontalMotion {
                    argument: Some(value),
                    ..
                },
            ) if motion_columns(value).is_some_and(|(absolute, units)| !absolute && units > 0) => {
                // A positive HORIZ consumes the local BACKBEFORE with its
                // first unit if necessary. NativeSlots records only the
                // remaining advances before the next accepted graph.
                if matches!(state, InkAfterAdvance) {
                    InkAfterAdvance
                } else {
                    Advance
                }
            }
            (
                Advance | InkAfterAdvance,
                RoffInlineEvent::Glyph(text, GlyphInput::NativeSpecial),
            ) if text
                .chars()
                .next()
                .is_some_and(super::super::native_writes::is_single_column_ink) =>
            {
                InkAfterAdvance
            }
            // In other states, only a terminal continuation is recorded
            // by actual placement; this escape leaves advance proof intact.
            (
                state,
                RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont | RoffInlineEvent::NoSpace,
            )
            | (
                state @ (Settled | Advance | InkAfterAdvance),
                RoffInlineEvent::LineBreak | RoffInlineEvent::ZeroWidthGlyph,
            ) => state,
            _ => return false,
        };
    }
    matches!(phase, InkAfterAdvance)
}
