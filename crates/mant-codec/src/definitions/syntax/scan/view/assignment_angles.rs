//! Pair literal assignment angles within accepted operand and enclosure boundaries.

use super::super::{
    MAX_NESTING, closing, option_character, parameter_token::parameter_character, quote_closer,
};
use super::{HeadView, NativeOperand, NativeOperandRole};

pub(super) fn paired_offsets(view: &HeadView<'_>) -> Vec<usize> {
    // A leading angle can contain native Fl operands. An
    // interior comparison cannot claim a later declaration's
    // closing angle. Use accepted native operand starts, which
    // can follow a space owned by the same operand.
    let mut pairs = Vec::new();
    let mut open: Vec<OpenAngle> = Vec::new();
    let mut escaped = false;
    let mut quote = None;
    let mut groups = Vec::new();
    let mut previous = None;
    let mut operands = view.operands.iter().peekable();
    for (offset, ch) in view.text.char_indices() {
        while let Some(operand) = operands.peek().filter(|item| item.bytes.end <= offset) {
            if let NativeOperandRole::EnclosureEnd(body) = operand.role {
                open.retain(|angle| angle.enclosure != Some(body));
            }
            operands.next();
        }
        let operand = operands
            .peek()
            .copied()
            .filter(|item| item.bytes.contains(&offset));
        if std::mem::take(&mut escaped) {
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(closer) = quote {
            if ch == closer {
                quote = None;
                previous = Some((ch, offset + ch.len_utf8()));
            }
            continue;
        }
        quote = quote_closer(ch);
        if ch == '\n' {
            open.clear();
            groups.clear();
        } else if ch == '-' && assignment_alias_boundary(view, offset, previous) {
            retire_assignment_angles(
                &mut open,
                &mut pairs,
                option_starts_assignment(view, offset)
                    || matches!(previous, Some((',', _)))
                        && option_starts_native_argument(view, offset),
            );
        }
        if !ch.is_whitespace() {
            previous = Some((ch, offset + ch.len_utf8()));
        }
        if operand.is_some_and(|item| {
            matches!(
                item.role,
                NativeOperandRole::EnclosureStart { .. } | NativeOperandRole::EnclosureEnd(_)
            )
        }) {
            continue;
        }
        match ch {
            '(' | '[' | '{' if groups.len() < MAX_NESTING => {
                groups.push(closing(ch).expect("opening bracket"));
            }
            ')' | ']' | '}' if groups.last() == Some(&ch) => {
                open.retain(|angle| angle.depth < groups.len());
                groups.pop();
            }
            '<' | '⟨' if open.len() < MAX_NESTING => {
                let leading = operand.is_none_or(|operand| {
                    let prefix = view.text[operand.bytes.start..offset].trim();
                    prefix.is_empty() || prefix.ends_with('=')
                });
                open.push(OpenAngle {
                    offset,
                    closer: closing(ch).expect("angle opener"),
                    leading,
                    numeric_close: None,
                    enclosure: angle_enclosure(operand),
                    depth: groups.len(),
                });
            }
            '>' | '⟩' => close_assignment_angle(
                &view.text,
                &mut open,
                operand,
                offset,
                groups.len(),
                &mut pairs,
            ),
            _ => {}
        }
    }
    pairs.sort_unstable();
    pairs
}

fn assignment_alias_boundary(
    view: &HeadView<'_>,
    offset: usize,
    previous: Option<(char, usize)>,
) -> bool {
    view.native_option_starts(offset)
        && matches!(previous, Some((delimiter @ (',' | '|'), end)) if end < offset
            || delimiter == ',' && !view.literal_punctuation(end - 1)
            || option_starts_assignment(view, offset))
}

fn close_assignment_angle(
    text: &str,
    open: &mut Vec<OpenAngle>,
    operand: Option<&NativeOperand>,
    offset: usize,
    depth: usize,
    pairs: &mut Vec<usize>,
) {
    let Some(angle) = open.last_mut().filter(|angle| angle.depth == depth) else {
        return;
    };
    if !text[offset..].starts_with(angle.closer) {
        return;
    }
    let after = offset + angle.closer.len_utf8();
    if operand.is_some_and(|operand| {
        operand.bytes.contains(&angle.offset)
            && text[after..operand.bytes.end].starts_with(|ch: char| ch.is_ascii_digit())
    }) {
        angle.numeric_close = Some(offset);
    }
    if angle.closes_at(text, offset, operand) {
        pairs.extend([angle.offset, offset]);
        open.pop();
    }
}

fn option_starts_assignment(view: &HeadView<'_>, offset: usize) -> bool {
    let tail = view.text[offset..].trim_start_matches(option_character);
    tail.trim_start().starts_with('=')
}

fn option_starts_native_argument(view: &HeadView<'_>, offset: usize) -> bool {
    let name = view.text[offset..].trim_start_matches(option_character);
    let value = name.trim_start();
    value.len() < name.len() && view.explicit_argument(view.text.len() - value.len())
}

struct OpenAngle {
    offset: usize,
    closer: char,
    leading: bool,
    numeric_close: Option<usize>,
    enclosure: Option<u32>,
    depth: usize,
}

fn retire_assignment_angles(open: &mut Vec<OpenAngle>, pairs: &mut Vec<usize>, independent: bool) {
    while open.last().is_some_and(|angle| !angle.leading) {
        open.pop();
    }
    // An independent option with its own value resolves a deferred
    // same-word numeric close in the preceding assignment.
    if independent
        && let Some(offset) = open.last().and_then(|angle| angle.numeric_close)
        && let Some(angle) = open.pop()
    {
        pairs.extend([angle.offset, offset]);
    }
}

impl OpenAngle {
    fn closes_at(&self, text: &str, offset: usize, operand: Option<&NativeOperand>) -> bool {
        if self.enclosure != angle_enclosure(operand) {
            return false;
        }
        operand.is_none_or(|operand| {
            let tail = &text[offset + self.closer.len_utf8()..operand.bytes.end];
            // Hold an opener across a numeric comparison within the same
            // typed operand. A later typed >suffix can close the opener.
            if operand.bytes.contains(&self.offset)
                && tail.starts_with(|ch: char| ch.is_ascii_digit())
            {
                return false;
            }
            self.leading || tail.is_empty() || tail.starts_with(|ch| !parameter_character(ch))
        })
    }
}

fn angle_enclosure(operand: Option<&NativeOperand>) -> Option<u32> {
    match operand?.role {
        NativeOperandRole::Placeholder { enclosure, .. }
        | NativeOperandRole::PathOperand(enclosure) => enclosure,
        NativeOperandRole::EnclosureContent { body, .. } => Some(body),
        _ => None,
    }
}
