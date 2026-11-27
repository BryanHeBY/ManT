use super::RoffInlineEvent;
use crate::mandoc::inline::flow::field_buffer::{AppendedSlotProof, FieldCell, FieldWrite};

#[derive(Default)]
pub(super) struct AppendedFonts {
    cells: usize,
    fonts: Vec<(std::ops::Range<usize>, super::Font)>,
    armed: bool,
    pending: bool,
    retain_pending_controls: bool,
}

impl AppendedFonts {
    pub(super) fn new(retain_pending_controls: bool) -> Self {
        Self {
            retain_pending_controls,
            ..Self::default()
        }
    }
    /// `encode1()` (`term.c:900-929`) adds decoration at each graph and
    /// converts `TERMP_BACKAFTER` to `TERMP_BACKBEFORE` after writing it;
    /// non-graph blanks in `encode()`/`bufferc()` (`term.c:860-872, 947-967`)
    /// do not advance that state. A final pending glyph hasn't yet retreated.
    pub(super) fn record(&mut self, writes: &[FieldWrite], font: super::Font) -> bool {
        for write in writes {
            match write {
                FieldWrite::Cell(cell) if decorates_at_most_one_column(cell) => {
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
                FieldWrite::ArmBackafter => {
                    // term.c:773-775: ESCAPE_SKIPCHAR only sets TERMP_BACKAFTER
                    // without writing any buffer bytes or triggering encode1().
                    self.armed = true;
                    continue;
                }
                FieldWrite::CancelBackafter if !self.pending || self.retain_pending_controls => {
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
                )
                | FieldWrite::UnprojectedBlank
                    if !self.pending || self.retain_pending_controls => {}
                FieldWrite::MotionPadding { .. } if !self.pending && !self.armed => {}
                FieldWrite::StageBacklineProjection if self.retain_pending_controls => continue,
                FieldWrite::DeferredBlanks { count, .. }
                    if !self.pending || self.retain_pending_controls =>
                {
                    self.cells += *count;
                    continue;
                }
                _ => return false,
            }
            self.cells += 1;
        }
        true
    }

    /// Emit the appended-slot font proof. When `pending` is set, the next
    /// word's `encode1()` (`term.c:903-921`) retreats via `TERMP_BACKBEFORE`
    /// (`term.c:926-929`) before decorating, so `replay_tail` tracks any tail
    /// copies outside `self.fonts`.
    pub(super) fn proof(self, styled_tail: Option<(usize, super::Font)>) -> Option<FieldWrite> {
        if (self.pending || self.armed)
            && self.cells > super::super::flow::field_buffer::MAX_EDITABLE_APPEND_CELLS
        {
            return None;
        }
        let decorated: usize = self.fonts.iter().map(|(range, _)| range.len()).sum();
        // When `!self.pending`, every decorated cell in this word was recorded
        // in `self.fonts` without a pending encode1() retreat, so
        // `raw_tail_fonts` is exact for `NativeSlots::with_field_prefix`
        // (term.c:899-969).
        let replay_tail = if self.pending && self.retain_pending_controls {
            let raw = self.fonts.iter().fold(self.cells, |slots, (range, font)| {
                slots.saturating_add(range.len().saturating_mul(
                    crate::mandoc::inline::flow::native_slots::decoration_slots(*font),
                ))
            });
            if raw > crate::mandoc::inline::flow::field_buffer::MAX_RUN_IN_REPLAY_CELLS {
                return None;
            }
            None
        } else if self.pending {
            match styled_tail {
                Some((count, _)) if count == decorated => None,
                Some((count, _)) => Some(count),
                None if decorated == 0 => None,
                None => Some(0),
            }
        } else {
            None
        };
        Some(FieldWrite::ProveAppendedSlots(AppendedSlotProof {
            cells: self.cells,
            fonts: self.fonts,
            replay_tail,
            pending: self.pending,
        }))
    }
}

/// Prove a fresh styled HEAD ending with BACKBEFORE still live. Control
/// writes and ordinary SP do not settle it (term.c:620–660, 903–969).
/// The next `encode1()` is the generated BODY separator. Its raw cursor
/// needs the complete HEAD, including any earlier decorated glyphs.
pub(in crate::mandoc::inline) fn retain_pending_run_in_head(
    events: &[RoffInlineEvent],
    writes: &mut [FieldWrite],
    font: super::Font,
) {
    if !has_native_decoration(font)
        || events.iter().any(|event| {
            matches!(
                event,
                RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont
            )
        })
    {
        return;
    }
    let Some((proof @ FieldWrite::OpaqueRawSlots { .. }, emitted)) = writes.split_last_mut() else {
        return;
    };
    let mut append = AppendedFonts {
        retain_pending_controls: true,
        ..AppendedFonts::default()
    };
    if !append.record(emitted, font) || !append.pending {
        return;
    }
    let decoration: usize = append.fonts.iter().map(|(range, _)| range.len()).sum();
    let raw_slots = append.cells.saturating_add(decoration.saturating_mul(
        crate::mandoc::inline::flow::native_slots::decoration_slots(font),
    ));
    if raw_slots <= crate::mandoc::inline::flow::field_buffer::MAX_RUN_IN_REPLAY_CELLS {
        *proof = FieldWrite::ProveAppendedSlots(AppendedSlotProof {
            cells: append.cells,
            fonts: append.fonts,
            replay_tail: None,
            pending: true,
        });
    }
}

/// Prove a final bounded-width decorated run (`encode1()`, `term.c:900-925`).
/// Buffered `ESCAPE_SKIPCHAR` / `ESCAPE_NOSPACE` / zero-width `ESCAPE_HORIZ`
/// (`term.c:662-682, 773-775`) and font/break controls (`term.c:640-661, 800`)
/// do not alter that decoration until a real cursor retreat executes.
pub(super) fn proved_decorated_tail(
    events: &[RoffInlineEvent],
    writes: &[crate::mandoc::inline::flow::field_buffer::FieldWrite],
    font: super::Font,
) -> Option<(usize, super::Font)> {
    // A styled append proves the raw copies in its final bounded-width run.
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
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::LineBreak
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint => {}
            RoffInlineEvent::Glyph(text, _) if text == "\u{a0}" => {
                armed = false;
            }
            _ => return None,
        }
    }
    let mut count = 0;
    let mut canceled = false;
    for write in writes.iter().rev() {
        match write {
            FieldWrite::CancelBackafter => canceled = true,
            FieldWrite::ArmBackafter if canceled => {}
            FieldWrite::Cell(cell) if decorates_at_most_one_column(cell) => {
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

fn decorates_at_most_one_column(cell: &FieldCell) -> bool {
    // term.c::encode1() uses the current font for ASCII graphs and
    // codepoints above 127, including zero-width combining marks. The
    // UTF-8 width bounds the row edit, not the raw encoded slot count.
    matches!(
        cell,
        FieldCell::Graph { text, width: 0..=1 } if uses_native_font(*text)
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

#[cfg(test)]
mod decorated_append_tests {
    use super::*;

    #[test]
    fn decorated_marks_keep_slots_without_admitting_wide_graphs() {
        for (text, width, proved) in [('a', 1, true), ('\u{301}', 0, true), ('中', 2, false)] {
            let writes = [FieldWrite::Cell(FieldCell::Graph { text, width })];
            let mut append = AppendedFonts::default();
            assert_eq!(append.record(&writes, super::super::Font::Emphasis), proved);
        }
    }
}
