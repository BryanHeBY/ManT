//! Source spelling candidates for one native declaration operand.
//!
//! The caller supplies accepted word receipts separately, after the native
//! formatter settles ownership.

use super::super::{FontState, native_slots};
use crate::definitions::{NativeOperandRole, is_native_field_operator};
use crate::mandoc::roff_escape::{DecodedText, PresentationKind, RoffInlineEvent};

fn literal_declaration_chars(decoded: &DecodedText) -> impl Iterator<Item = char> + '_ {
    // CVS term_word() writes a zero-width cell for the ESCAPE_IGNORE
    // produced by roff_escape.c for color and point-size escapes. Those
    // cells and font controls do not change one No operand's spelling.
    // ESCAPE_HORIZ changes the cursor or inserts padding, not authored ink.
    // term_word records `\c` as spacing state and `\:`/`\p` as break
    // controls. term_fill can break after a word for `\p`, but term_field
    // does not emit ink or a blank for either break marker. Keep the source
    // operand's authored punctuation adjacent to any actual authored blank.
    // This only identifies candidates: execution retains the controls, and
    // accepted word receipts still determine spelling and invalidate covered ink.
    // Complete ignored arguments (including tty link operations) write an
    // ASCII_NBRZW cell in CVS. A linear word contributes no spelling there,
    // but a rewind can let even this cell replace a live slot. Link events
    // keep their separate rendering/annotation meaning outside this scanner.
    let mut can_rewind = None;
    decoded.events.iter().flat_map(move |event| match event {
        RoffInlineEvent::Font(_)
        | RoffInlineEvent::PreviousFont
        | RoffInlineEvent::ZeroWidthGlyph
        | RoffInlineEvent::NoSpace
        | RoffInlineEvent::Breakpoint
        | RoffInlineEvent::LineBreak
        | RoffInlineEvent::HorizontalMotion { .. }
        | RoffInlineEvent::Presentation {
            kind: PresentationKind::Color | PresentationKind::PointSize,
            ..
        } => "".chars(),
        RoffInlineEvent::Presentation {
            kind: PresentationKind::Motion | PresentationKind::IgnoredNativeCell,
            argument: Some(_),
        }
        | RoffInlineEvent::Link(_)
            if decoded.proves_native_glyphs()
                && !*can_rewind.get_or_insert_with(|| may_rewind_typed_ink(&decoded.events)) =>
        {
            "".chars()
        }
        RoffInlineEvent::Text(word) | RoffInlineEvent::Glyph(word, _) => word.chars(),
        RoffInlineEvent::NonBreakingWordBlank => " ".chars(),
        _ => "\0".chars(),
    })
}

pub(super) fn standalone_field_operator(decoded: &DecodedText) -> bool {
    let mut chars = literal_declaration_chars(decoded).skip_while(|glyph| glyph.is_whitespace());
    chars.next().is_some_and(is_native_field_operator) && chars.all(char::is_whitespace)
}

fn declaration_separator(decoded: &DecodedText) -> bool {
    let mut chars = literal_declaration_chars(decoded).skip_while(|glyph| glyph.is_whitespace());
    let separator = match chars.next() {
        Some(',' | '|' | '/') => true,
        Some('o') => chars.next() == Some('r'),
        _ => false,
    };
    separator && chars.all(char::is_whitespace)
}

pub(super) fn literal_declaration_role(decoded: &DecodedText) -> Option<NativeOperandRole> {
    if declaration_separator(decoded) || standalone_field_operator(decoded) {
        return Some(NativeOperandRole::LiteralContinuation(None));
    }
    // Keep only separator-like slashes opaque. A tight path such as
    // /var/log is still an ordinary literal in a complete assignment.
    // An opaque execution event can prevent a positive spelling proof,
    // but cannot turn the following text in the same source operand into
    // an independent declaration. In this negative check only, keep an
    // authored slash adjacent to its following authored blank across it.
    let mut slash = false;
    literal_declaration_chars(decoded)
        .filter(|&character| character != '\0')
        .any(|character| {
            let looks_like_separator =
                matches!(character, ',' | '|') || slash && character.is_whitespace();
            slash = character == '/';
            looks_like_separator
        })
        .then_some(NativeOperandRole::LiteralPunctuation)
}

pub(super) fn native_spelling_role(
    role: NativeOperandRole,
    decoded: Option<&DecodedText>,
    follows_option: bool,
    font: impl FnOnce() -> FontState,
) -> NativeOperandRole {
    if !decoded.is_some_and(move |decoded| {
        !decoded.proves_native_glyphs()
            || follows_option
                && role.is_attached_composite_value()
                && !sole_authored_joiner(role, decoded)
                && loses_typed_ink(&decoded.events, font)
    }) {
        return role;
    }
    match role {
        NativeOperandRole::Placeholder { .. } => NativeOperandRole::IncompletePlaceholder,
        NativeOperandRole::SearchArgument(_) | NativeOperandRole::FlWord(_) => {
            NativeOperandRole::Argument
        }
        _ => role.without_complete_spelling(),
    }
}

fn sole_authored_joiner(role: NativeOperandRole, decoded: &DecodedText) -> bool {
    if !matches!(
        role,
        NativeOperandRole::LiteralValue | NativeOperandRole::SymbolicText
    ) {
        return false;
    }
    // A leading rewind cannot clip this word's sole authored glyph.
    // Receiver and writer still need accepted receipts: covering another
    // word vetoes an exact form, and losing this glyph clips its role.
    let mut glyphs = literal_declaration_chars(decoded).filter(|glyph| !glyph.is_whitespace());
    glyphs
        .next()
        .is_some_and(|glyph| glyph == '/' || is_native_field_operator(glyph))
        && glyphs.next().is_none()
}

/// Recover same-word clipping in the bounded, relative terminal input
/// domain. Accepted field covers handle writes across native word owners.
/// Within one word, CVS `term_word` changes col but not lastcol on rewind;
/// only a later write to a live glyph slot invalidates its source proof.
fn loses_typed_ink(events: &[RoffInlineEvent], font: impl FnOnce() -> FontState) -> bool {
    use libmandoc_rs::TableColumnMetrics;

    if !may_rewind_typed_ink(events) {
        return false;
    }
    let mut font = font();
    let mut grid = TypedInkGrid::default();
    let mut backafter = false;
    let mut backbefore = false;
    for event in events {
        match event {
            RoffInlineEvent::ZeroAdvance => backafter = true,
            RoffInlineEvent::NoSpace => backafter = false,
            RoffInlineEvent::HorizontalMotion { argument, .. } => {
                if std::mem::take(&mut backafter) {
                    continue;
                }
                let Some(argument) = argument.as_deref() else {
                    continue;
                };
                if let Some(target) = argument.strip_prefix('|') {
                    // Absolute addresses need the live field origin.
                    if TableColumnMetrics::motion_argument(target).is_some() {
                        return true;
                    }
                } else if let Some(columns) =
                    crate::mandoc::inline::word_input::negative_entry_columns(Some(argument))
                {
                    if !grid.retreat(columns + usize::from(std::mem::take(&mut backbefore))) {
                        return true;
                    }
                } else if TableColumnMetrics::motion_argument(argument)
                    .is_some_and(|(units, _)| units > 0)
                {
                    let columns = TableColumnMetrics::relative_motion_columns(argument);
                    if !grid.advance_blanks(
                        columns.saturating_sub(usize::from(std::mem::take(&mut backbefore))),
                        TypedSlot::Other,
                    ) {
                        return true;
                    }
                }
            }
            RoffInlineEvent::Font(next) => font.select(*next),
            RoffInlineEvent::PreviousFont => font.restore(),
            RoffInlineEvent::Text(text) | RoffInlineEvent::Glyph(text, _) if text.is_ascii() => {
                for glyph in text.bytes() {
                    let intact = if glyph == b' ' {
                        grid.advance_blanks(1, TypedSlot::Blank)
                    } else if glyph.is_ascii_graphic() {
                        let intact = grid.write_ink(
                            native_slots::decoration_slots(font.current) + 1,
                            &mut backbefore,
                        );
                        backbefore = std::mem::take(&mut backafter);
                        intact
                    } else {
                        false
                    };
                    if !intact {
                        return true;
                    }
                }
            }
            RoffInlineEvent::BreakableHyphen => {
                if !grid.write_ink(
                    native_slots::decoration_slots(font.current) + 1,
                    &mut backbefore,
                ) {
                    return true;
                }
                backbefore = std::mem::take(&mut backafter);
            }
            RoffInlineEvent::NonBreakingWordBlank => {
                if !grid.advance_blanks(1, TypedSlot::Other) {
                    return true;
                }
            }
            RoffInlineEvent::Link(_) => {}
            RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint
            | RoffInlineEvent::Presentation {
                kind: PresentationKind::Color | PresentationKind::PointSize,
                ..
            } => {
                if !grid.overwrite(1, TypedSlot::Other) {
                    return true;
                }
            }
            // Ambiguous codepoint/fallback encodings and drawing controls
            // cannot certify survival across a real native motion.
            _ => return true,
        }
    }
    false
}

fn may_rewind_typed_ink(events: &[RoffInlineEvent]) -> bool {
    use libmandoc_rs::TableColumnMetrics;

    let may_inherit_backbefore = events
        .iter()
        .any(|event| matches!(event, RoffInlineEvent::ZeroAdvance));
    events.iter().any(|event| {
        let RoffInlineEvent::HorizontalMotion {
            argument: Some(value),
            ..
        } = event
        else {
            return false;
        };
        if let Some(target) = value.strip_prefix('|') {
            return TableColumnMetrics::motion_argument(target).is_some();
        }
        crate::mandoc::inline::word_input::negative_entry_columns(Some(value))
            .is_some_and(|columns| columns > 0 || may_inherit_backbefore)
    })
}

#[derive(Clone, Copy)]
enum TypedSlot {
    Blank,
    Other,
    Ink,
}

#[derive(Default)]
struct TypedInkGrid {
    slots: Vec<TypedSlot>,
    cursor: usize,
}

impl TypedInkGrid {
    const MAX_SLOTS: usize = 16_384;

    fn retreat(&mut self, columns: usize) -> bool {
        if let Some(landing) = self.cursor.checked_sub(columns) {
            self.cursor = landing;
            true
        } else {
            // This write could target ink from an earlier native word.
            false
        }
    }

    fn advance_blanks(&mut self, count: usize, slot: TypedSlot) -> bool {
        let end = self.cursor.saturating_add(count);
        if end > Self::MAX_SLOTS {
            return false;
        }
        // bufferc leaves previously buffered cells intact for space/NBRSP.
        if end > self.slots.len() {
            self.slots.resize(end, slot);
        }
        self.cursor = end;
        true
    }

    fn write_ink(&mut self, slots: usize, backbefore: &mut bool) -> bool {
        if std::mem::take(backbefore) {
            if self
                .cursor
                .checked_sub(1)
                .is_some_and(|previous| matches!(self.slots[previous], TypedSlot::Blank))
            {
                self.cursor -= 1;
            } else if !self.overwrite(1, TypedSlot::Other) {
                return false;
            }
        }
        self.overwrite(slots, TypedSlot::Ink)
    }

    fn overwrite(&mut self, count: usize, slot: TypedSlot) -> bool {
        let end = self.cursor.saturating_add(count);
        if end > Self::MAX_SLOTS
            || self.slots[self.cursor..end.min(self.slots.len())]
                .iter()
                .any(|slot| matches!(slot, TypedSlot::Ink))
        {
            return false;
        }
        self.slots
            .resize(self.slots.len().max(end), TypedSlot::Other);
        self.slots[self.cursor..end].fill(slot);
        self.cursor = end;
        true
    }
}

#[derive(Clone)]
pub(super) enum FlSpelling {
    KeyBytes(usize),
    Assignment(String),
}

impl FlSpelling {
    pub(super) fn from_events(events: &[RoffInlineEvent]) -> Option<Self> {
        if let Some(prefix) = assignment_prefix(events) {
            return Some(Self::Assignment(prefix));
        }
        // Ordinary Fl keys have one projected byte sequence even across
        // font changes. Accepting fewer bytes from this unique owner
        // cannot prove the original spelling. Cursor-editing operands use
        // the execution's existing native clipping receipts instead.
        let bytes = events.iter().try_fold(0, |bytes, event| match event {
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _)
                if !value.contains(char::is_whitespace) =>
            {
                Some(bytes + value.len())
            }
            RoffInlineEvent::BreakableHyphen => Some(bytes + 1),
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::ZeroWidthGlyph => Some(bytes),
            _ => None,
        })?;
        (bytes > 0).then_some(Self::KeyBytes(bytes))
    }

    pub(super) fn survived(&self, value: &str) -> bool {
        match self {
            Self::KeyBytes(bytes) => value.len() == *bytes,
            // A native overstrike in an attached value need not invalidate
            // the untouched key and assignment separator.
            Self::Assignment(prefix) => value.starts_with(prefix),
        }
    }
}

fn assignment_prefix(events: &[RoffInlineEvent]) -> Option<String> {
    if !events.iter().any(|event| matches!(event, RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _) if value.contains('='))) {
        return None;
    }
    let mut prefix = String::new();
    for event in events {
        match event {
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value, _) => {
                if let Some(index) = value.find('=') {
                    prefix.push_str(&value[..=index]);
                    return (prefix.len() > 1).then_some(prefix);
                }
                prefix.push_str(value);
            }
            RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont => {}
            _ => return None,
        }
    }
    None
}
