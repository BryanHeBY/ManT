use super::{RoffInlineEvent, TextEventState, is_formatter_word_blank};

/// `term.c::term_word()` buffers controls and invisible cells before
/// `term_field()` projects printable output. Record these facts independently
/// of the semantic IR and the zero-advance presentation machine.
pub(super) fn record_native_event(
    event: &RoffInlineEvent,
    writes: &mut Vec<crate::mandoc::inline::flow::field_buffer::FieldWrite>,
    fallback_projected: bool,
    authoritative_state: Option<&TextEventState>,
) {
    use crate::mandoc::inline::flow::field_buffer::{FieldCell, FieldWrite};
    match event {
        RoffInlineEvent::NonBreakingWordBlank => {
            writes.push(FieldWrite::Cell(FieldCell::NonBreakingBlank));
        }
        // term_word() -> encode() copies an ordinary text run without
        // changing the marker state. Only an already pending break/suppressed
        // blank needs the per-character ownership path below (term.c:294-305,
        // 593-607). Otherwise append the run once, with Vec's size hint.
        RoffInlineEvent::Text(value)
            if authoritative_state.is_some_and(|state| {
                state.pending_word_end_break || state.suppress_break_whitespace
            }) =>
        {
            let state = authoritative_state.expect("authoritative text state");
            let mut pending = state.pending_word_end_break;
            let mut suppress = state.suppress_break_whitespace;
            for character in value.chars() {
                if is_formatter_word_blank(character) && (pending || suppress) {
                    writes.push(FieldWrite::UnprojectedBlank);
                    pending = false;
                    suppress = true;
                } else {
                    suppress = false;
                    FieldWrite::append_character(writes, character);
                }
            }
        }

        RoffInlineEvent::Text(value)
        | RoffInlineEvent::Glyph(value)
        | RoffInlineEvent::Overstrike {
            terminal: Some(value),
            ..
        } => FieldWrite::append_literal(writes, value),
        RoffInlineEvent::FallbackGlyph(value) => {
            writes.push(FieldWrite::RecoveryGlyph {
                projected_scalars: if fallback_projected {
                    value.chars().count()
                } else {
                    0
                },
            });
        }
        RoffInlineEvent::DeviceName => {
            FieldWrite::append_literal(writes, "utf8");
        }
        RoffInlineEvent::BreakableHyphen => {
            writes.push(FieldWrite::Cell(FieldCell::Hyphen));
        }
        RoffInlineEvent::ZeroAdvance => {
            writes.push(FieldWrite::ArmBackafter);
        }
        RoffInlineEvent::NoSpace | RoffInlineEvent::HorizontalMotion { .. } => {
            writes.push(FieldWrite::CancelBackafter);
        }
        RoffInlineEvent::LineBreak => {
            writes.push(FieldWrite::Cell(FieldCell::BreakMarker));
        }
        RoffInlineEvent::ZeroWidthGlyph => {
            writes.push(FieldWrite::Cell(FieldCell::ZeroWidthGraph));
        }
        RoffInlineEvent::Breakpoint => {
            writes.push(FieldWrite::Cell(FieldCell::Breakpoint));
        }
        RoffInlineEvent::EmptyDestination => {
            FieldWrite::append_literal(writes, "<>");
        }
        _ => {}
    }
}
