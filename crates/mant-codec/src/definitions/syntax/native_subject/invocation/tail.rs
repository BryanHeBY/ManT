//! Argument groups distinguish a program pipe from a nested choice.

use super::ParameterEdges;
use crate::definitions::{NativeOperand, NativeOperandRole};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
enum InvocationGroup {
    Native(u32),
    Authored(char),
}

struct Group {
    kind: InvocationGroup,
    argument: bool,
    native_body: Option<u32>,
}

pub(super) struct InvocationTail {
    arguments: ArgumentProof,
    pub(super) pipeline: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ArgumentProof {
    #[default]
    Bare,
    Proved,
    Unproved,
}

#[derive(Default)]
struct ScopeScan {
    groups: Vec<Group>,
    leading_delimiters: Vec<(u32, char)>,
    malformed: bool,
    arguments: ArgumentProof,
    top_level_pipe: bool,
}

impl InvocationTail {
    pub(super) fn read(
        text: &str,
        span: Range<usize>,
        operands: &[NativeOperand],
        bare: bool,
    ) -> Self {
        let edges = ParameterEdges::new(text, span.clone(), operands);
        let mut state = ScopeScan::default();
        let mut cursor = span.start;
        let mut possible_pipeline = false;
        for (index, operand) in operands.iter().enumerate() {
            if operand.bytes.start >= span.end {
                break;
            }
            if operand.bytes.end <= span.start {
                continue;
            }
            let end = operand.bytes.end.min(span.end);
            if end <= cursor {
                continue;
            }
            let start = operand.bytes.start.max(cursor);
            state.authored_text(&text[cursor..start]);
            let role = operand.role;
            let word = text[start..end].trim();
            match role {
                NativeOperandRole::EnclosureStart { body, parent } => {
                    state.malformed |= parent != state.native_body();
                    state.open(InvocationGroup::Native(body), word == "\"");
                    state.enter_leading_delimiters(body);
                }
                NativeOperandRole::EnclosureEnd(body) => state.close(InvocationGroup::Native(body)),
                NativeOperandRole::LiteralDelimiter { glyph, enclosure } => {
                    if !edges.absorbs(start) {
                        state.authored_delimiter(glyph, enclosure);
                    }
                }
                NativeOperandRole::Literal
                | NativeOperandRole::LiteralContinuation(_)
                | NativeOperandRole::EnclosureContent(_)
                    if is_bracket_text(word) =>
                {
                    state.authored_text(word);
                }
                NativeOperandRole::ExplicitOption | NativeOperandRole::Placeholder { .. } => {
                    state.argument();
                    // A bracket attached to Ar's spelling did not go
                    // through mdoc_isdelim(). Accepted edge glyphs can
                    // still enclose arguments in the visible invocation.
                    if matches!(role, NativeOperandRole::Placeholder { .. }) {
                        for (offset, glyph) in edges.within(start..end) {
                            if edges.exposes(offset, state.native_body(), state.authored_closer()) {
                                state.bracket(glyph);
                            }
                        }
                    }
                }
                NativeOperandRole::Command if !state.groups.is_empty() => state.argument(),
                NativeOperandRole::Command
                | NativeOperandRole::ProgramName(_)
                | NativeOperandRole::ClippedInvocation
                    if state.groups.is_empty() && bare =>
                {
                    state.arguments = ArgumentProof::Unproved;
                }
                _ => {
                    if matches!(
                        role.declaration_delimiter(),
                        NativeOperandRole::PipeDelimiter(_)
                    ) && starts_program(text, end, span.end, &operands[index + 1..])
                    {
                        possible_pipeline = true;
                        state.top_level_pipe |= state.groups.is_empty();
                    }
                    if !word.is_empty() {
                        state.unproved_prose();
                    }
                }
            }
            cursor = end;
        }
        state.authored_text(&text[cursor..span.end]);
        state.malformed |= !state.groups.is_empty() || !state.leading_delimiters.is_empty();
        // A damaged group cannot establish a pipe's scope, but it is not
        // itself a pipe. A contiguous Nm/Cm name can still be complete.
        let arguments = if state.malformed
            || state.arguments == ArgumentProof::Bare && !text[span].trim().is_empty()
        {
            ArgumentProof::Unproved
        } else {
            state.arguments
        };
        Self {
            arguments,
            pipeline: state.top_level_pipe || possible_pipeline && state.malformed,
        }
    }

    pub(super) fn proves_arguments(&self, allow_bare: bool) -> bool {
        match self.arguments {
            ArgumentProof::Bare => allow_bare,
            ArgumentProof::Proved => true,
            ArgumentProof::Unproved => false,
        }
    }
}

impl ScopeScan {
    fn authored_delimiter(&mut self, glyph: char, enclosure: Option<u32>) {
        if let Some(body) = enclosure
            && Some(body) != self.native_body()
            && matches!(glyph, '(' | '[')
        {
            self.leading_delimiters.push((body, glyph));
        } else {
            self.bracket(glyph);
        }
    }

    fn enter_leading_delimiters(&mut self, body: u32) {
        for (scope, glyph) in std::mem::take(&mut self.leading_delimiters) {
            if scope == body {
                self.bracket(glyph);
            } else {
                self.malformed = true;
            }
        }
    }

    fn unproved_prose(&mut self) {
        if self.groups.is_empty() && self.arguments == ArgumentProof::Bare {
            self.arguments = ArgumentProof::Unproved;
        }
    }

    fn authored_closer(&self) -> Option<char> {
        match self.groups.last()?.kind {
            InvocationGroup::Authored(closer) => Some(closer),
            InvocationGroup::Native(_) => None,
        }
    }

    fn authored_text(&mut self, text: &str) {
        let text = text.trim();
        if is_bracket_text(text) {
            for ch in text.chars().filter(|ch| !ch.is_whitespace()) {
                self.bracket(ch);
            }
        } else if !text.is_empty() {
            self.unproved_prose();
        }
    }

    fn bracket(&mut self, glyph: char) {
        match glyph {
            '(' => self.open(InvocationGroup::Authored(')'), false),
            '[' => self.open(InvocationGroup::Authored(']'), false),
            ')' | ']' => self.close(InvocationGroup::Authored(glyph)),
            _ => {}
        }
    }

    fn open(&mut self, kind: InvocationGroup, argument: bool) {
        let native_body = match kind {
            InvocationGroup::Native(body) => Some(body),
            InvocationGroup::Authored(_) => self.native_body(),
        };
        self.groups.push(Group {
            kind,
            argument,
            native_body,
        });
    }

    fn native_body(&self) -> Option<u32> {
        self.groups.last().and_then(|group| group.native_body)
    }

    fn close(&mut self, kind: InvocationGroup) {
        if self.groups.last().is_none_or(|group| group.kind != kind) {
            self.malformed = true;
            return;
        }
        let group = self.groups.pop().expect("matching invocation group");
        if group.argument {
            self.argument();
        } else {
            self.unproved_prose();
        }
    }

    fn argument(&mut self) {
        if let Some(group) = self.groups.last_mut() {
            group.argument = true;
        } else if self.arguments == ArgumentProof::Bare {
            self.arguments = ArgumentProof::Proved;
        }
    }
}

fn is_bracket(ch: char) -> bool {
    matches!(ch, '(' | '[' | ')' | ']')
}

fn is_bracket_text(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| is_bracket(ch) || ch.is_whitespace())
}

fn opening_text(text: &str) -> bool {
    text.chars()
        .all(|ch| matches!(ch, '(' | '[') || ch.is_whitespace())
}

pub(super) fn starts_program(
    text: &str,
    mut at: usize,
    end: usize,
    operands: &[NativeOperand],
) -> bool {
    for operand in operands.iter().take_while(|word| word.bytes.start < end) {
        if operand.bytes.end > end || !text.get(at..operand.bytes.start).is_some_and(opening_text) {
            return false;
        }
        let role = operand.role;
        if !matches!(role, NativeOperandRole::EnclosureStart { .. })
            && !role.display_only()
            && !opening_text(&text[operand.bytes.clone()])
        {
            return matches!(role, NativeOperandRole::ProgramName(_));
        }
        at = operand.bytes.end;
    }
    false
}
