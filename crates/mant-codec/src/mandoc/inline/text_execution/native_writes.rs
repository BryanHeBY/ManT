use super::RoffInlineEvent;

/// A native `encode1()` writes actual decoration bytes into the input
/// buffer (term.c:894–926). Their extent matters only while a leading
/// negative HORIZ can still overlap a prior buffered entry.
pub(super) fn qualify_rewound_graphs(
    writes: &mut [crate::mandoc::inline::flow::field_buffer::FieldWrite],
    start: usize,
    font: super::Font,
) {
    use crate::mandoc::inline::flow::field_buffer::{FieldCell, FieldWrite};
    if !matches!(writes.first(), Some(FieldWrite::RewindWordEntry(_))) {
        return;
    }
    let prefix = match font {
        super::Font::Strong
        | super::Font::Emphasis
        | super::Font::CodeStrong
        | super::Font::CodeEmphasis => 2,
        super::Font::StrongEmphasis => 4,
        super::Font::Regular | super::Font::Code => return,
    };
    for write in &mut writes[start..] {
        if let FieldWrite::Cell(cell @ (FieldCell::Graph { .. } | FieldCell::Hyphen)) = write {
            *write = FieldWrite::DecoratedGraph {
                cell: cell.clone(),
                prefix,
            };
        }
    }
}

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
        RoffInlineEvent::NoSpace => {
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
