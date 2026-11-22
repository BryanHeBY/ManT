//! Bounded native proofs for marked operands that have not reached `term_fill()`.

use super::{ReplayDomain, ReplayPass, RoffInlineEvent, TextExecutionContext, motion_columns};
use crate::mandoc::inline::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS;

/// A marker followed by authored ink and buffered SP can leave live raw
/// slots even when the final event is another marker. `term.c::encode1()`
/// may put four decoration bytes before each graph; `term_word()` buffers
/// SP and the marker separately. Place these writes in their original
/// order so a later HORIZ addresses the native slots before `term_fill()`.
pub(super) fn bounded_append(events: &[RoffInlineEvent]) -> bool {
    let mut remaining = MAX_EDITABLE_APPEND_CELLS;
    let mut marker = false;
    let mut ink_after_marker = false;
    let mut has_space = false;
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
            RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont => 0,
            _ => return false,
        };
        let Some(left) = remaining.checked_sub(count) else {
            return false;
        };
        remaining = left;
    }
    has_space && ink_after_marker
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
