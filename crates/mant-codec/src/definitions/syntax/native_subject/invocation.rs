//! Native delimiters close macro scopes without necessarily ending an invocation.
use super::{offset, operand_at, operand_cursor};
use crate::definitions::syntax::named;
use crate::definitions::{NativeOperand, NativeOperandRole};

pub(super) struct Syntax<'a> {
    text: &'a str,
    operands: &'a [NativeOperand],
    prefix: Option<Prefix>,
}

struct Prefix {
    start: usize,
    first: usize,
    assignment: Option<usize>,
    role: LeadingRole,
}

enum LeadingRole {
    Option(OptionTail),
    Command { next: usize, has_argument: bool },
    Other,
}

struct OptionTail {
    cursor: usize,
    end: usize,
    has_argument: bool,
}

impl OptionTail {
    fn reaches_argument(&mut self, text: &str, operands: &[NativeOperand], at: usize) -> bool {
        if at <= self.end || self.has_argument {
            return self.has_argument;
        }
        while self.end < at
            && !self.has_argument
            && let Some(operand) = operands
                .get(self.cursor)
                .filter(|operand| operand.bytes.start < at)
        {
            // Accepted table text only decorates the invocation. Untyped
            // text surrounding a receipt still carries its source word.
            let start = operand.bytes.start.max(self.end);
            let end = operand.bytes.end.min(at);
            self.has_argument = !text[self.end..start].trim().is_empty()
                || !operand.role.display_only() && !text[start..end].trim().is_empty();
            self.end = end;
            if end == operand.bytes.end {
                self.cursor += 1;
            }
        }
        self.has_argument |= !text[self.end..at].trim().is_empty();
        self.end = at;
        self.has_argument
    }
}

impl<'a> Syntax<'a> {
    pub(super) const fn new(text: &'a str, operands: &'a [NativeOperand]) -> Self {
        Self {
            text,
            operands,
            prefix: None,
        }
    }

    pub(super) fn has_operand_pipe(mut self, start: usize, end: usize) -> bool {
        self.text[start..end]
            .match_indices('|')
            .any(|(at, _)| self.protects(start, start + at))
    }

    pub(super) fn protects(&mut self, start: usize, at: usize) -> bool {
        let text = self.text;
        let operands = self.operands;
        let next = following_operand(text, operands, at + 1);
        let begins_owner = next.is_some_and(|next| is_new_owner(next.role));
        let bare_pipe = text.as_bytes()[at] == b'|'
            && operand_at(operands, at)
                .is_some_and(|operand| matches!(operand.role, NativeOperandRole::PipeDelimiter(_)));
        let prefix = self.prefix(start, at);
        if !begins_owner && prefix.assignment.is_some_and(|equals| equals < at) {
            return true;
        }
        match &mut prefix.role {
            LeadingRole::Option(tail) => {
                // A separately executed Fl or typed subject can open
                // its own declaration after an option argument list.
                !begins_owner && tail.reaches_argument(text, operands, at)
            }
            LeadingRole::Command {
                next: cursor,
                has_argument,
            } if bare_pipe => {
                // Visit each accepted operand only once, even for a long
                // command prefix followed by many argument alternatives.
                while !*has_argument
                    && let Some(word) = operands.get(*cursor).filter(|word| word.bytes.start < at)
                {
                    *cursor += 1;
                    *has_argument = word.role.is_operand_label()
                        || word.role == NativeOperandRole::ExplicitOption;
                }
                *has_argument
                    || !begins_owner && operand_pipe(text, operands, prefix.first, at, next)
            }
            _ => bare_pipe && !begins_owner && operand_pipe(text, operands, prefix.first, at, next),
        }
    }

    fn prefix(&mut self, start: usize, at: usize) -> &mut Prefix {
        if self.prefix.as_ref().is_none_or(|memo| memo.start != start) {
            let first = offset(self.text, self.text[start..at].trim_start());
            let cursor = operand_cursor(self.operands, first);
            let lead = self
                .operands
                .get(cursor)
                .filter(|operand| operand.bytes.contains(&first));
            let role = match lead.map(|operand| operand.role) {
                Some(NativeOperandRole::ExplicitOption) => {
                    let end = self.option_end(cursor);
                    LeadingRole::Option(OptionTail {
                        cursor: operand_cursor(self.operands, end),
                        end,
                        has_argument: false,
                    })
                }
                Some(NativeOperandRole::Command) => LeadingRole::Command {
                    next: cursor + 1,
                    has_argument: false,
                },
                _ => LeadingRole::Other,
            };
            // Compare a joined configuration key to the same complete
            // grammar used for named definitions. A separated word that
            // merely contains '=' supplies no proof for the preceding one.
            let key_and_value = self.text[first..at]
                .split_whitespace()
                .next()
                .unwrap_or_default();
            let assignment = key_and_value
                .split_once('=')
                .filter(|(key, _)| named::is_configuration_key(key))
                .map(|(key, _)| first + key.len());
            self.prefix = Some(Prefix {
                start,
                first,
                assignment,
                role,
            });
        }
        self.prefix.as_mut().expect("initialized invocation prefix")
    }

    fn option_end(&self, cursor: usize) -> usize {
        let (mut word, words) = self.operands[cursor..]
            .split_first()
            .expect("accepted option");
        for next in words {
            // Nested Fl can contribute an adjacent dash-only prefix.
            if next.bytes.start != word.bytes.end
                || next.role != NativeOperandRole::ExplicitOption
                || !self.text[word.bytes.clone()]
                    .trim()
                    .bytes()
                    .all(|byte| byte == b'-')
            {
                break;
            }
            word = next;
        }
        word.bytes.end
    }
}

fn following_operand<'a>(
    text: &str,
    operands: &'a [NativeOperand],
    mut after: usize,
) -> Option<&'a NativeOperand> {
    loop {
        after = offset(text, text[after..].trim_start());
        let word = operand_at(operands, after)?;
        if !word.role.display_only() {
            return Some(word);
        }
        after = word.bytes.end;
    }
}

fn operand_pipe(
    text: &str,
    operands: &[NativeOperand],
    start: usize,
    at: usize,
    next: Option<&NativeOperand>,
) -> bool {
    let cursor = operand_cursor(operands, at);
    let Some(left) = cursor.checked_sub(1).and_then(|index| operands.get(index)) else {
        return false;
    };
    // mdoc_macro.c::in_line rewinds Ar before the bare middle delimiter.
    // This remains a pipeline unless both sides are operand labels.
    // An explicit No delimiter is not a native pipe.
    left.bytes.end > start
        && left.role.is_operand_label()
        && text[left.bytes.end..at].trim().is_empty()
        && next.is_none_or(|right| !right.role.is_operand_label())
}

fn is_new_owner(role: NativeOperandRole) -> bool {
    role == NativeOperandRole::ExplicitOption
        || matches!(role, NativeOperandRole::Environment { .. })
        || role.subject().is_some()
}

/// Carry the separator's resolved role into the shared group grammar. This
/// projection is private: names still map to the original accepted bytes.
pub(super) fn protect_arguments(
    operands: Vec<NativeOperand>,
    separators: &[usize],
    start: usize,
) -> Vec<NativeOperand> {
    if separators.is_empty() {
        return operands;
    }
    let delimiter = |at| NativeOperand {
        bytes: at..at + 1,
        role: NativeOperandRole::Argument,
    };
    let mut separators = separators.iter().map(|at| at - start).peekable();
    let mut result = Vec::with_capacity(operands.len());
    for mut word in operands {
        while let Some(at) = separators.next_if(|at| *at < word.bytes.end) {
            if at > word.bytes.start {
                result.push(NativeOperand {
                    bytes: word.bytes.start..at,
                    role: word.role,
                });
            }
            result.push(delimiter(at));
            word.bytes.start = word.bytes.start.max(at + 1);
        }
        if !word.bytes.is_empty() {
            result.push(word);
        }
    }
    result.extend(separators.map(delimiter));
    result
}
