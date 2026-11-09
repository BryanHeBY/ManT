use super::RoffInlineEvent;

/// `term.c::term_word()` buffers controls and invisible cells before
/// `term_field()` projects printable output. Record these facts independently
/// of the semantic IR and the zero-advance presentation machine.
pub(super) fn record_native_event(
    event: &RoffInlineEvent,
    writes: &mut Vec<crate::mandoc::inline::flow::field_buffer::FieldWrite>,
    fallback_projected: bool,
) {
    use crate::mandoc::inline::flow::field_buffer::{FieldCell, FieldWrite};
    match event {
        RoffInlineEvent::NonBreakingWordBlank => {
            writes.push(FieldWrite::Cell(FieldCell::NonBreakingBlank));
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
            writes.push(FieldWrite::Cell(FieldCell::ZeroWidthGraph));
            FieldWrite::append_literal(writes, "<>");
        }
        _ => {}
    }
}
