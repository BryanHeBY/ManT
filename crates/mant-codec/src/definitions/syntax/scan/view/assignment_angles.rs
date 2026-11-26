//! Pair literal assignment angles within accepted operand and enclosure boundaries.

use super::super::{
    MAX_NESTING, closing, internal_apostrophe, option_character,
    parameter_token::parameter_character, quote_closer,
};
use super::{NativeOperand, NativeOperandRole, operand_at};
use std::ops::Range;

#[derive(Default)]
pub(super) struct Pairs {
    pub(super) offsets: Vec<usize>,
    pub(super) assignments: Vec<Range<usize>>,
}

#[derive(Clone, Copy)]
struct AngleView<'a> {
    text: &'a str,
    operands: &'a [NativeOperand],
}

impl<'a> AngleView<'a> {
    fn operand(self, offset: usize) -> Option<&'a NativeOperand> {
        operand_at(self.operands, offset)
    }

    fn native_option_starts(self, offset: usize) -> bool {
        self.operand(offset).is_some_and(|operand| {
            operand.role == NativeOperandRole::ExplicitOption
                && self.text[operand.bytes.start..offset].trim().is_empty()
        })
    }

    fn literal_punctuation(self, offset: usize) -> bool {
        self.operand(offset)
            .is_some_and(|operand| operand.role == NativeOperandRole::LiteralPunctuation)
    }

    fn option_assignment_at(self, equals: usize) -> bool {
        let lhs = self.text[..equals].trim_end();
        let start = lhs.rfind(|ch: char| !option_character(ch)).map_or(0, |at| {
            at + lhs[at..].chars().next().expect("scalar").len_utf8()
        });
        let key = &lhs[start..];
        super::super::super::options::option_prefix(key) == Some(key)
            && self.native_option_starts(start)
    }
}

pub(super) fn paired_angles(text: &str, operands: &[NativeOperand]) -> Pairs {
    let view = AngleView { text, operands };
    let mut pairs = Pairs::default();
    let mut scopes = vec![AngleScope::default()];
    let mut operands = view.operands.iter().peekable();
    for (offset, ch) in view.text.char_indices() {
        while operands.peek().is_some_and(|item| item.bytes.end <= offset) {
            operands.next();
        }
        let operand = operands
            .peek()
            .copied()
            .filter(|item| item.bytes.contains(&offset));
        let scope = scopes.last_mut().expect("root angle scope");
        match operand.map(|item| item.role) {
            Some(NativeOperandRole::EnclosureStart { body, parent, .. }) => {
                if operand.is_some_and(|item| item.bytes.start == offset) {
                    // termp_quote_pre/post own the generated delimiters and
                    // enclose a native BODY or ELEM. Its lexical state cannot
                    // modify the parent assignment, even for identical glyphs.
                    if parent != scope.body || scopes.len() > MAX_NESTING {
                        break;
                    }
                    scopes.last_mut().expect("root angle scope").previous = None;
                    scopes.push(AngleScope {
                        body: Some(body),
                        ..AngleScope::default()
                    });
                }
            }
            Some(NativeOperandRole::EnclosureEnd(body)) => {
                if operand.is_some_and(|item| item.bytes.start == offset) {
                    if scope.body != Some(body) {
                        break;
                    }
                    scopes.pop();
                }
            }
            _ => {
                if angle_enclosure(operand).is_none_or(|body| scope.body == Some(body)) {
                    scope.observe(&view, offset, ch, operand, &mut pairs);
                }
            }
        }
    }
    pairs.offsets.sort_unstable();
    pairs.assignments.sort_unstable_by_key(|range| range.start);
    // Nested pairs are covered by their outer assignment. Keep disjoint
    // intervals so each separator lookup can use one binary search.
    let mut covered = 0;
    pairs.assignments.retain(|range| {
        if range.start < covered {
            debug_assert!(range.end <= covered, "paired angles cannot cross");
            return false;
        }
        covered = range.end;
        true
    });
    pairs
}

#[derive(Default)]
struct AngleScope {
    body: Option<u32>,
    open: Vec<OpenAngle>,
    escaped: bool,
    quote: Option<char>,
    groups: Vec<char>,
    previous: Option<(char, usize)>,
}

impl AngleScope {
    fn observe(
        &mut self,
        view: &AngleView<'_>,
        offset: usize,
        ch: char,
        operand: Option<&NativeOperand>,
        pairs: &mut Pairs,
    ) {
        if std::mem::take(&mut self.escaped) {
            return;
        }
        if ch == '\\' {
            self.escaped = true;
            return;
        }
        if let Some(closer) = self.quote {
            if ch == closer {
                self.quote = None;
                self.previous = Some((ch, offset + ch.len_utf8()));
            }
            return;
        }
        if !internal_apostrophe(view.text, offset) {
            self.quote = quote_closer(ch);
        }
        let previous = self.previous;
        if ch == '\n' {
            self.open.clear();
            self.groups.clear();
        } else if ch == '-' && assignment_alias_boundary(view, offset, self.previous) {
            // Only an independent option with an executed value can resolve
            // a deferred numeric close, and only in the same lexical scope.
            self.retire_assignment_angles(
                pairs,
                option_starts_assignment(view, offset)
                    || option_starts_native_argument(view, offset),
            );
        }
        if !ch.is_whitespace() {
            self.previous = Some((ch, offset + ch.len_utf8()));
        }
        match ch {
            '(' | '[' | '{' if self.groups.len() < MAX_NESTING => {
                self.groups.push(closing(ch).expect("opening bracket"));
            }
            ')' | ']' | '}' if self.groups.last() == Some(&ch) => {
                self.open.retain(|angle| angle.depth < self.groups.len());
                self.groups.pop();
            }
            '<' | '⟨' if self.open.len() < MAX_NESTING => {
                let leading = operand.is_none_or(|operand| {
                    let prefix = view.text[operand.bytes.start..offset].trim();
                    prefix.is_empty() || prefix.ends_with('=')
                });
                self.open.push(OpenAngle {
                    offset,
                    closer: closing(ch).expect("angle opener"),
                    leading,
                    numeric_close: None,
                    assignment: leading
                        && previous.is_some_and(|(ch, end)| {
                            ch == '=' && view.option_assignment_at(end - 1)
                        }),
                    depth: self.groups.len(),
                });
            }
            '>' | '⟩' => close_assignment_angle(
                view.text,
                &mut self.open,
                operand,
                offset,
                self.groups.len(),
                pairs,
            ),
            _ => {}
        }
    }

    fn retire_assignment_angles(&mut self, pairs: &mut Pairs, independent: bool) {
        let depth = self.groups.len();
        while self
            .open
            .last()
            .is_some_and(|angle| angle.depth == depth && !angle.leading)
        {
            self.open.pop();
        }
        if independent
            && let Some(angle) = self.open.last().filter(|angle| angle.depth == depth)
            && let Some(offset) = angle.numeric_close
        {
            pairs.record(angle, offset);
            self.open.pop();
        }
    }
}

fn assignment_alias_boundary(
    view: &AngleView<'_>,
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
    pairs: &mut Pairs,
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
        pairs.record(angle, offset);
        open.pop();
    }
}

fn option_starts_assignment(view: &AngleView<'_>, offset: usize) -> bool {
    let tail = view.text[offset..].trim_start_matches(option_character);
    tail.trim_start().starts_with('=')
}

fn option_starts_native_argument(view: &AngleView<'_>, offset: usize) -> bool {
    let name = view.text[offset..].trim_start_matches(option_character);
    let value = name.trim_start();
    value.len() < name.len()
        && view
            .operand(view.text.len() - value.len())
            .is_some_and(|operand| {
                operand.role.is_parameter()
                    || matches!(operand.role, NativeOperandRole::PathOperand(_))
            })
}

struct OpenAngle {
    offset: usize,
    closer: char,
    leading: bool,
    numeric_close: Option<usize>,
    assignment: bool,
    depth: usize,
}

impl Pairs {
    fn record(&mut self, angle: &OpenAngle, close: usize) {
        self.offsets.extend([angle.offset, close]);
        if angle.assignment {
            self.assignments
                .push(angle.offset..close + angle.closer.len_utf8());
        }
    }
}

impl OpenAngle {
    fn closes_at(&self, text: &str, offset: usize, operand: Option<&NativeOperand>) -> bool {
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
