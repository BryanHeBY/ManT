//! Bounded native proofs for marked operands that have not reached `term_fill()`.

use super::{ReplayDomain, ReplayPass, RoffInlineEvent, TextExecutionContext, motion_columns};
use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;

/// A marker followed by authored ink and buffered SP can leave live raw
/// slots even when the final event is another marker. `term.c::encode1()`
/// may put four decoration bytes before each graph; `term_word()` buffers
/// SP and the marker separately. Place these writes in their original
/// order so a later HORIZ addresses the native slots before `term_fill()`.
/// `ESCAPE_NOSPACE` changes registers without buffering a byte. A decoded
/// nonbreaking space still reaches `encode1()` as a single glyph.
pub(super) fn bounded_append(events: &[RoffInlineEvent]) -> bool {
    bounded_append_with_leading_columns(events, 0)
}

/// A relative HORIZ in a fresh definition line either writes native
/// NBRSP before any ink or retreats the row origin without advancing col.
/// Preserve those cells and later markers until the line closes so another
/// operand can still edit them.
pub(super) fn bounded_field_append(events: &[RoffInlineEvent]) -> bool {
    let Some((
        RoffInlineEvent::HorizontalMotion {
            argument: Some(argument),
            ..
        },
        remaining,
    )) = events.split_first()
    else {
        return false;
    };
    let Some((false, units)) = motion_columns(argument) else {
        return false;
    };
    let initial = if units > 0 {
        libmandoc_rs::TableColumnMetrics::relative_motion_columns(argument)
    } else {
        0
    };
    units != 0 && bounded_append_with_leading_columns(remaining, initial)
}

fn bounded_append_with_leading_columns(events: &[RoffInlineEvent], initial: usize) -> bool {
    let Some(mut remaining) = MAX_EDITABLE_APPEND_CELLS.checked_sub(initial) else {
        return false;
    };
    let has_link = events
        .iter()
        .any(|event| matches!(event, RoffInlineEvent::Link(_)));
    let mut marker = false;
    let mut ink_after_marker = false;
    let mut has_space = initial != 0;
    for event in events {
        let count = match event {
            RoffInlineEvent::Text(text)
                if text
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() || byte == b' ') =>
            {
                let spaces = text.bytes().filter(|&byte| byte == b' ').count();
                let ink = text.len() - spaces;
                has_space |= spaces != 0;
                ink_after_marker |= marker && ink != 0;
                spaces.saturating_add(ink.saturating_mul(5))
            }
            RoffInlineEvent::LineBreak => {
                marker = true;
                1
            }
            // A local arm after a marker remains in the same fresh
            // filled buffer. Reserve the extra backline that encode1()
            // can write when a following glyph covers the pending slot.
            RoffInlineEvent::ZeroAdvance if initial == 0 && marker => 1,
            // `\X'tty: link'` emits ESCAPE_IGNORE (ASCII_NBRZW) via
            // ZeroWidthGlyph without extra encode1() bytes (term.c:620-638).
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::NoSpace
            | RoffInlineEvent::Link(_) => 0,
            RoffInlineEvent::ZeroWidthGlyph if has_link => 1,
            RoffInlineEvent::Glyph(text, _) if text == "\u{a0}" => 5,
            _ => return false,
        };
        let Some(left) = remaining.checked_sub(count) else {
            return false;
        };
        remaining = left;
    }
    has_space && ink_after_marker
}

/// A continued no-fill definition line has not run `term_newln()` yet.
/// Retain a bounded graphless HORIZ+marker append on its complete live
/// buffer. The later `term_field()` call determines whether it prints.
pub(super) fn bounded_literal_control_append(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    use super::EntryCursor;
    let entry = &context.slot_entry;
    if entry.domain != ReplayDomain::LiteralDefinitionLine
        || entry.cursor != EntryCursor::AtEnd
        || entry.automatic_blanks != 0
        || context.zero_advance.is_armed()
        || context.zero_advance.has_annotation_output_owner()
        || context.defer_leading_blanks
    {
        return false;
    }
    let Some(prefix) = entry.prefix.as_ref().filter(|prefix| {
        prefix.start == 0
            && prefix.pass == ReplayPass::Incremental
            && !prefix.backbefore
            && !prefix.backafter
            && prefix.pending_cell.is_none()
            && prefix.tail_fonts.is_empty()
    }) else {
        return false;
    };
    let Some((
        RoffInlineEvent::HorizontalMotion {
            argument: Some(value),
            ..
        },
        tail,
    )) = events.split_first()
    else {
        return false;
    };
    let columns = libmandoc_rs::TableColumnMetrics::relative_motion_columns(value);
    columns != 0
        && prefix
            .raw_slot_count()
            .saturating_add(columns)
            .saturating_add(tail.len())
            <= MAX_EDITABLE_APPEND_CELLS
        && tail.iter().all(|event| {
            matches!(
                event,
                RoffInlineEvent::LineBreak
                    | RoffInlineEvent::ZeroWidthGlyph
                    | RoffInlineEvent::Breakpoint
            )
        })
}

/// A graphless operand can retreat beyond the complete live input and
/// change the native margin before that node restores its offset.
pub(super) fn tracks_literal_control_retreat(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> bool {
    use super::EntryCursor;
    let entry = &context.slot_entry;
    entry.domain == ReplayDomain::LiteralDefinitionLine
        && entry.cursor == EntryCursor::AtEnd
        && !entry.kept_blanks
        && !context.zero_advance.has_annotation_output_owner()
        && !context.defer_leading_blanks
        && entry.prefix.as_ref().is_some_and(|prefix| {
            prefix.start == 0
                && prefix.pass == ReplayPass::Incremental
                && !prefix.backbefore
                && !prefix.backafter
                && prefix.pending_cell.is_none()
                && prefix.tail_fonts.is_empty()
        })
        && events.split_first().is_some_and(|(first, tail)| {
            matches!(first, RoffInlineEvent::HorizontalMotion { .. })
                && tail
                    .iter()
                    .any(|event| matches!(event, RoffInlineEvent::LineBreak))
                && tail.iter().all(|event| {
                    matches!(
                        event,
                        RoffInlineEvent::LineBreak
                            | RoffInlineEvent::ZeroWidthGlyph
                            | RoffInlineEvent::Breakpoint
                    )
                })
        })
}

#[derive(Clone, Copy)]
pub(super) enum ControlReplay {
    None,
    Append,
    CancelArm,
}

impl ControlReplay {
    pub(super) const fn is_proved(self) -> bool {
        !matches!(self, Self::None)
    }

    pub(super) const fn cancels_arm(self) -> bool {
        matches!(self, Self::CancelArm)
    }
}

/// A later operand can change the same native field even if it produces
/// no printable ink. Require a complete editable prefix and direct writes
/// understood by `NativeSlots`. `term.c::encode()` buffers leading literal SP
/// without consuming BACKBEFORE or BACKAFTER; a following `ESCAPE_NOSPACE`
/// can cancel only BACKAFTER while keeping the known earlier glyph.
pub(super) fn control_replay(
    events: &[RoffInlineEvent],
    context: &TextExecutionContext<'_>,
) -> ControlReplay {
    if context.slot_entry.domain != ReplayDomain::Filled
        || context.zero_advance.has_annotation_output_owner()
        || !context
            .slot_entry
            .prefix
            .as_ref()
            .is_some_and(|prefix| prefix.start == 0 && prefix.pass == ReplayPass::Editable)
    {
        return ControlReplay::None;
    }
    let mut remaining = MAX_EDITABLE_APPEND_CELLS;
    let mut marker = false;
    let mut first_control = None;
    for event in events {
        let count = match event {
            RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont => continue,
            RoffInlineEvent::Text(text) if text.bytes().all(|byte| byte == b' ') => text.len(),
            _ => {
                first_control.get_or_insert(event);
                let Some(count) = control_slots(event) else {
                    return ControlReplay::None;
                };
                marker |= matches!(event, RoffInlineEvent::LineBreak);
                count
            }
        };
        let Some(left) = remaining.checked_sub(count) else {
            return ControlReplay::None;
        };
        remaining = left;
    }
    if !marker {
        ControlReplay::None
    } else if matches!(first_control, Some(RoffInlineEvent::NoSpace)) {
        ControlReplay::CancelArm
    } else {
        ControlReplay::Append
    }
}

fn control_slots(event: &RoffInlineEvent) -> Option<usize> {
    match event {
        RoffInlineEvent::NoSpace => Some(0),
        RoffInlineEvent::LineBreak
        | RoffInlineEvent::ZeroWidthGlyph
        | RoffInlineEvent::Breakpoint => Some(1),
        RoffInlineEvent::Glyph(text, _) if text == "\u{a0}" => Some(5),
        RoffInlineEvent::HorizontalMotion { argument, .. } => {
            let argument = argument.as_deref()?;
            let (absolute, units) = motion_columns(argument)?;
            match (absolute, units) {
                (false, 1..) => Some(libmandoc_rs::TableColumnMetrics::relative_motion_columns(
                    argument,
                )),
                (false, _) => Some(0),
                (true, _) => None,
            }
        }
        _ => None,
    }
}
