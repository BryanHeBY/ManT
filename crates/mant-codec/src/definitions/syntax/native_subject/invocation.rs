//! Native delimiters close macro scopes without necessarily ending an invocation.
use super::{offset, operand_at, operand_cursor};
use crate::definitions::syntax::{commands, named};
use crate::definitions::{NativeOperand, NativeOperandRole, NativeSubject, RecognizedName};
use std::ops::Range;

/// Read the leading Nm and any consecutive Cm/Ic words in one visible
/// prefix, stopping before arguments or a new physical line.
pub(super) fn program_name(
    text: &str,
    span: &Range<usize>,
    operands: &[NativeOperand],
    allow_bare: bool,
) -> Option<RecognizedName> {
    let operands = &operands[operand_cursor(operands, span.start)..];
    let (program, tail) = operands.split_first()?;
    if program.role != NativeOperandRole::ProgramName
        || !program.bytes.contains(&span.start)
        || program.bytes.end > span.end
        || !text[program.bytes.start..span.start].trim().is_empty()
    {
        return None;
    }
    let name = text[span.start..program.bytes.end].trim_end();
    if name.contains(char::is_whitespace) || !commands::is_command_name(name) {
        return None;
    }
    let mut end = span.start + name.len();
    for operand in tail {
        if operand.role != NativeOperandRole::Command || operand.bytes.end > span.end {
            break;
        }
        let word = text[operand.bytes.clone()].trim();
        let start = offset(text, word);
        let Some(gap) = text.get(end..start).filter(|gap| !gap.is_empty()) else {
            break;
        };
        if gap.contains('\n') || !gap.trim().is_empty() || word.contains('\n') {
            break;
        }
        end = start + word.len();
    }
    // Cm/Ic can supply several separately executed words even within one
    // native element. Recognize their argument syntax across all segments.
    let prefix = commands::styled_command_prefix(&text[span.start..end]);
    if prefix.len() == name.len()
        && !is_program_tail(text, program.bytes.end..span.end, tail, allow_bare)
    {
        return None;
    }
    let end = span.start + prefix.len();
    // A boundary between source elements can still be a joined visible
    // word. Only a complete name in the accepted text can bind.
    (commands::is_command_name(prefix)
        && text[end..span.end].chars().next().is_none_or(|character| {
            character.is_whitespace() || matches!(character, '[' | '<' | '{' | '(')
        }))
    .then(|| RecognizedName::contiguous(prefix, span.start))
}

/// Nm can also name the subject of a manual. A standalone invocation is
/// either bare or starts its tail with an actual argument, flag, or a
/// native enclosure containing such a choice. Ordinary prose is not proof.
fn is_program_tail(
    text: &str,
    span: Range<usize>,
    operands: &[NativeOperand],
    allow_bare: bool,
) -> bool {
    let rest = text[span.clone()].trim_start();
    if rest.is_empty() {
        // A bare Nm can declare a whole head or an explicit list of names.
        // Alongside a different
        // declaration (e.g. Ev HOME , Nm FILES) it can also be a reference.
        return allow_bare;
    }
    let start = offset(text, rest);
    let mut enclosure = None;
    let mut argument = false;
    for operand in operands
        .iter()
        .take_while(|operand| operand.bytes.start < span.end)
    {
        if !argument && enclosure.is_none() && !operand.bytes.contains(&start) {
            return false;
        }
        match operand.role {
            NativeOperandRole::EnclosureStart { body, parent: None } => {
                enclosure = Some(body);
                // Typewriter quotes in a native usage label can explicitly
                // delimit an argument (e.g. Qo reason Qc in atf-sh-api).
                argument |= text[operand.bytes.clone()].trim() == "\"";
            }
            NativeOperandRole::EnclosureEnd(body) if enclosure == Some(body) => {
                if !argument {
                    return false;
                }
                enclosure = None;
            }
            NativeOperandRole::ExplicitOption | NativeOperandRole::Placeholder => argument = true,
            NativeOperandRole::Command if enclosure.is_some() => argument = true,
            // An intervening option keeps a later subcommand out of the
            // contiguous name. Its presence cannot prove a bare Nm instead.
            NativeOperandRole::Command
            | NativeOperandRole::ProgramName
            | NativeOperandRole::ClippedInvocation
                if enclosure.is_none() =>
            {
                return false;
            }
            _ if !argument && enclosure.is_none() => return false,
            _ => {}
        }
    }
    argument
}

pub(super) fn assignment_operator(text: &str) -> Option<usize> {
    let (key, _) = text.split_once('=')?;
    named::is_configuration_key(key.trim()).then_some(key.len())
}

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
        let separator = text.as_bytes()[at];
        let bare_pipe = separator == b'|'
            && operand_at(operands, at).is_some_and(|operand| {
                matches!(
                    operand.role.declaration_delimiter(),
                    NativeOperandRole::PipeDelimiter(_)
                )
            });
        let reopened_flag = separator == b','
            && operand_at(operands, at).is_some_and(|operand| {
                matches!(operand.role, NativeOperandRole::FlReopening { .. })
            });
        let argument_separator = bare_pipe
            || reopened_flag
            || separator == b','
                && !begins_owner
                && next.is_none_or(|next| !next.role.is_invocation_name());
        let prefix = self.prefix(start, at);
        if prefix.assignment.is_some_and(|equals| {
            equals < at && (!begins_owner || same_assignment_rhs(text, operands, equals, at, next))
        }) {
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
            } if argument_separator => {
                // Visit each accepted operand only once, even for a long
                // command prefix followed by many argument alternatives.
                while !*has_argument
                    && let Some(word) = operands.get(*cursor).filter(|word| word.bytes.start < at)
                {
                    *cursor += 1;
                    *has_argument = word.role.is_operand_label()
                        || matches!(
                            word.role,
                            NativeOperandRole::ExplicitOption
                                | NativeOperandRole::SearchArgument(_)
                                | NativeOperandRole::SymbolicText
                        );
                }
                *has_argument
                    || bare_pipe
                        && operand_at(operands, prefix.first)
                            .is_some_and(|word| word.role == NativeOperandRole::ProgramName)
                        && next.is_some_and(|word| word.role == NativeOperandRole::ProgramName)
                    || bare_pipe
                        && !begins_owner
                        && operand_pipe(text, operands, prefix.first, at, next)
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
                Some(role) if role.is_invocation_name() => LeadingRole::Command {
                    next: cursor + 1,
                    has_argument: false,
                },
                _ => LeadingRole::Other,
            };
            // Validate the entire written key before the assignment.
            // Whitespace and macro boundaries may occur beside '='.
            let assignment =
                assignment_operator(&self.text[first..at]).map(|equals| first + equals);
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

fn same_assignment_rhs(
    text: &str,
    operands: &[NativeOperand],
    equals: usize,
    separator: usize,
    next: Option<&NativeOperand>,
) -> bool {
    let Some(previous) = operand_cursor(operands, separator)
        .checked_sub(1)
        .and_then(|index| operands.get(index))
    else {
        return false;
    };
    // mdoc_macro.c::in_line closes and reopens the same source invocation
    // at a bare delimiter. A separately called macro gets a new receipt.
    text.get(previous.bytes.start.max(equals + 1)..previous.bytes.end)
        .is_some_and(|rhs| !rhs.trim().is_empty())
        && text[previous.bytes.end..separator].trim().is_empty()
        && next.is_some_and(|next| {
            let reopened_flag = previous.role == NativeOperandRole::ExplicitOption
                && next.role == NativeOperandRole::ExplicitOption
                && operand_at(operands, separator).is_some_and(|operand| {
                    matches!(operand.role, NativeOperandRole::FlReopening { .. })
                });
            reopened_flag
                || typed_invocation(previous.role)
                    .is_some_and(|left| typed_invocation(next.role) == Some(left))
        })
}

fn typed_invocation(role: NativeOperandRole) -> Option<(Option<NativeSubject>, u32, Option<u32>)> {
    match role {
        NativeOperandRole::Environment {
            invocation: Some(invocation),
            enclosure,
        } => Some((None, invocation, enclosure)),
        NativeOperandRole::Subject(subject) => {
            Some((Some(subject.kind), subject.invocation?, subject.enclosure))
        }
        _ => None,
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
