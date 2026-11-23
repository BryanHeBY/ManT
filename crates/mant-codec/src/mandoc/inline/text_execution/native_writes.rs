use super::RoffInlineEvent;
use crate::mandoc::inline::flow::field_buffer::{AppendedSlotProof, FieldCell, FieldWrite};

#[derive(Default)]
pub(super) struct AppendedFonts {
    cells: usize,
    fonts: Vec<(std::ops::Range<usize>, super::Font)>,
    armed: bool,
    pending: bool,
}

impl AppendedFonts {
    /// `encode1()` adds decoration at each graph, not at the end of a
    /// source word. A final pending glyph hasn't yet caused a retreat.
    pub(super) fn record(&mut self, writes: &[FieldWrite], font: super::Font) -> bool {
        for write in writes {
            match write {
                FieldWrite::Cell(cell) if decorates_single_column(cell) => {
                    if self.pending {
                        return false;
                    }
                    self.pending = std::mem::take(&mut self.armed);
                    if has_native_decoration(font) {
                        if let Some((run, prior)) = self.fonts.last_mut()
                            && run.end == self.cells
                            && *prior == font
                        {
                            run.end += 1;
                        } else {
                            self.fonts.push((self.cells..self.cells + 1, font));
                        }
                    }
                }
                FieldWrite::ArmBackafter if !self.pending => {
                    self.armed = true;
                    continue;
                }
                FieldWrite::CancelBackafter if !self.pending => {
                    // term.c:662-667: ESCAPE_NOSPACE clears BACKAFTER and
                    // buffers no bytes into tcol->buf.
                    self.armed = false;
                    continue;
                }
                FieldWrite::Cell(
                    FieldCell::BreakableBlank
                    | FieldCell::NonBreakingBlank
                    | FieldCell::ZeroWidthGraph
                    | FieldCell::Breakpoint
                    | FieldCell::BreakMarker,
                ) if !self.pending => {}
                _ => return false,
            }
            self.cells += 1;
        }
        true
    }

    pub(super) fn proof(self, styled_tail: Option<(usize, super::Font)>) -> Option<FieldWrite> {
        if (self.pending || self.armed)
            && self.cells > super::super::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS
        {
            return None;
        }
        let decorated: usize = self.fonts.iter().map(|(range, _)| range.len()).sum();
        let replay_tail = match styled_tail {
            Some((count, _)) if count == decorated => None,
            Some((count, _)) => Some(count),
            None if decorated == 0 => None,
            None => Some(0),
        };
        Some(FieldWrite::ProveAppendedSlots(AppendedSlotProof {
            cells: self.cells,
            fonts: self.fonts,
            replay_tail,
            pending: self.pending,
        }))
    }
}

pub(super) fn proved_decorated_tail(
    events: &[RoffInlineEvent],
    writes: &[crate::mandoc::inline::flow::field_buffer::FieldWrite],
    font: super::Font,
) -> Option<(usize, super::Font)> {
    // A styled append proves the raw copies in its final one-column run.
    // `term_word()` can buffer marker and BACKAFTER controls before
    // that run. They do not change the decoration written by encode1.
    // Actual cursor edits require the full placement path.
    if !has_native_decoration(font) {
        return None;
    }
    let mut armed = false;
    for event in events {
        match event {
            RoffInlineEvent::Text(value) => {
                if value.chars().any(uses_native_font) {
                    armed = false;
                }
            }
            RoffInlineEvent::ZeroAdvance => armed = true,
            RoffInlineEvent::NoSpace => armed = false,
            RoffInlineEvent::HorizontalMotion { argument, .. }
                if std::mem::take(&mut armed)
                    || super::geometry_word::relative_zero_motion(argument.as_deref()) => {}
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::LineBreak
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint => {}
            _ => return None,
        }
    }
    let mut count = 0;
    let mut canceled = false;
    for write in writes.iter().rev() {
        match write {
            FieldWrite::CancelBackafter => canceled = true,
            FieldWrite::ArmBackafter if canceled => {}
            FieldWrite::Cell(cell) if decorates_single_column(cell) => {
                canceled = false;
                count += 1;
            }
            _ => break,
        }
    }
    (count != 0).then_some((count, font))
}

pub(super) fn writes_decorated_graph(writes: &[FieldWrite], font: super::Font) -> bool {
    has_native_decoration(font)
        && writes.iter().any(|write| {
            matches!(
                write,
                FieldWrite::Cell(FieldCell::Graph { text, .. }) if uses_native_font(*text)
            ) || matches!(
                write,
                FieldWrite::Cell(FieldCell::Hyphen)
                    | FieldWrite::DecoratedGraph { .. }
                    | FieldWrite::UnprojectedGraph { .. }
            )
        })
}

fn decorates_single_column(cell: &FieldCell) -> bool {
    // term.c::encode1() uses the current font for ASCII graphs and
    // codepoints above 127. The measured UTF-8 width bounds the row edit.
    matches!(
        cell,
        FieldCell::Graph { text, width: 1 } if uses_native_font(*text)
    )
}

fn uses_native_font(character: char) -> bool {
    character.is_ascii_graphic() || !character.is_ascii()
}

pub(super) fn is_single_column_ink(character: char) -> bool {
    if character.is_ascii() {
        return character.is_ascii_graphic();
    }
    !character.is_whitespace()
        && !character.is_control()
        && mant_ir::geometry::text_width(character.encode_utf8(&mut [0; 4])) == 1
}

pub(super) fn has_native_decoration(font: super::Font) -> bool {
    !matches!(font, super::Font::Regular | super::Font::Code)
}

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
pub(in crate::mandoc::inline) fn record_native_event(
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
        | RoffInlineEvent::Glyph(value, _)
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
