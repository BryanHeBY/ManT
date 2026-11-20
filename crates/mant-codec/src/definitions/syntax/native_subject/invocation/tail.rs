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
    leading_offset: Option<usize>,
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
    leading_delimiters: Vec<(u32, char, usize)>,
    outer_delimiters: Vec<usize>,
    root_scope: Option<u32>,
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
        let state = ScopeScan::read(text, &span, operands, &edges, None, bare);
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
            pipeline: state.top_level_pipe,
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
    fn read(
        text: &str,
        span: &Range<usize>,
        operands: &[NativeOperand],
        edges: &ParameterEdges,
        root_scope: Option<u32>,
        bare: bool,
    ) -> Self {
        let mut state = Self {
            root_scope,
            ..Self::default()
        };
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
            state.authored_text(&text[cursor..start], cursor);
            let role = operand.role;
            let word = text[start..end].trim();
            match role {
                NativeOperandRole::EnclosureStart { body, parent } => {
                    state.malformed |= parent != state.native_body();
                    state.open(InvocationGroup::Native(body), word == "\"");
                    state.enter_leading_delimiters(body);
                }
                NativeOperandRole::EnclosureEnd(body) => state.close_native(body),
                NativeOperandRole::LiteralDelimiter { glyph, enclosure } => {
                    if !edges.absorbs(start) {
                        state.authored_delimiter(
                            glyph,
                            enclosure,
                            end - text[start..end].trim_start().len(),
                        );
                    }
                }
                NativeOperandRole::Literal
                | NativeOperandRole::LiteralContinuation(_)
                | NativeOperandRole::EnclosureContent(_)
                    if is_bracket_text(word) =>
                {
                    state.authored_text(&text[start..end], start);
                }
                NativeOperandRole::ExplicitOption | NativeOperandRole::Placeholder { .. } => {
                    state.argument();
                    // A bracket attached to Ar's spelling did not go
                    // through mdoc_isdelim(). Accepted edge glyphs can
                    // still enclose arguments in the visible invocation.
                    if matches!(role, NativeOperandRole::Placeholder { .. }) {
                        for (offset, glyph) in edges.within(start..end) {
                            if edges.exposes(offset, state.native_body(), state.authored_closer()) {
                                state.bracket(glyph, offset);
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
        state.authored_text(&text[cursor..span.end], cursor);
        state.malformed |= !state.groups.is_empty() || !state.leading_delimiters.is_empty();
        state.top_level_pipe |= possible_pipeline && state.malformed;
        state
    }

    fn authored_delimiter(&mut self, glyph: char, enclosure: Option<u32>, offset: usize) {
        if let Some(body) = enclosure
            && Some(body) != self.native_body()
            && matches!(glyph, '(' | '[')
        {
            self.leading_delimiters.push((body, glyph, offset));
        } else {
            self.bracket(glyph, offset);
        }
    }

    fn enter_leading_delimiters(&mut self, body: u32) {
        for (scope, glyph, offset) in std::mem::take(&mut self.leading_delimiters) {
            if scope == body {
                self.bracket(glyph, offset);
                if let Some(group) = self.groups.last_mut() {
                    group.leading_offset = Some(offset);
                }
            } else {
                self.malformed = true;
            }
        }
    }

    fn close_native(&mut self, body: u32) {
        let trailing = self
            .groups
            .iter()
            .rev()
            .take_while(|group| group.leading_offset.is_some())
            .count();
        let Some(index) = self.groups.len().checked_sub(trailing + 1) else {
            self.malformed = true;
            return;
        };
        if self.groups[index].kind != InvocationGroup::Native(body) {
            self.malformed = true;
            return;
        }
        // blk_part_exp() emits these delimiters before the BODY opens.
        // A close executed inside can pair with them there; otherwise they
        // remain open in the parent's sequence across the entire BODY.
        let native = self.groups.remove(index);
        let parent = index
            .checked_sub(1)
            .map_or(self.root_scope, |at| self.groups[at].native_body);
        for group in &mut self.groups[index..] {
            if parent == self.root_scope {
                self.outer_delimiters.extend(group.leading_offset.take());
            }
            group.native_body = parent;
        }
        if native.argument {
            self.argument();
        } else {
            self.unproved_prose();
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

    fn authored_text(&mut self, text: &str, start: usize) {
        if is_bracket_text(text.trim()) {
            for (offset, ch) in text.char_indices().filter(|(_, ch)| !ch.is_whitespace()) {
                self.bracket(ch, start + offset);
            }
        } else if !text.trim().is_empty() {
            self.unproved_prose();
        }
    }

    fn bracket(&mut self, glyph: char, offset: usize) {
        match glyph {
            '(' => self.open(InvocationGroup::Authored(')'), false),
            '[' => self.open(InvocationGroup::Authored(']'), false),
            ')' | ']' => self.close_authored(glyph, offset),
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
            leading_offset: None,
        });
    }

    fn native_body(&self) -> Option<u32> {
        self.groups
            .last()
            .map_or(self.root_scope, |group| group.native_body)
    }

    fn close_authored(&mut self, closer: char, offset: usize) {
        let Some(index) = self
            .groups
            .iter()
            .rposition(|group| matches!(group.kind, InvocationGroup::Authored(_)))
        else {
            self.malformed = true;
            return;
        };
        // The native BODY can execute the close of an already-open
        // authored group. Native openers do not close that punctuation
        // (mdoc_macro.c::blk_part_exp).
        let group = &self.groups[index];
        if group.kind != InvocationGroup::Authored(closer) {
            self.malformed = true;
            return;
        }
        if group.native_body == self.root_scope && self.native_body() != self.root_scope {
            self.outer_delimiters.push(offset);
        }
        let group = self.groups.remove(index);
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

pub(in crate::definitions::syntax::native_subject) fn outer_authored_delimiters(
    text: &str,
    span: Range<usize>,
    operands: &[NativeOperand],
    edges: &ParameterEdges,
    enclosure: Option<u32>,
) -> Vec<usize> {
    if !operands
        .iter()
        .take_while(|operand| operand.bytes.start < span.end)
        .any(|operand| {
            matches!(
                operand.role,
                NativeOperandRole::LiteralDelimiter {
                    glyph: '(' | '[',
                    ..
                }
            )
        })
    {
        return Vec::new();
    }
    let mut result =
        ScopeScan::read(text, &span, operands, edges, enclosure, false).outer_delimiters;
    result.sort_unstable();
    result
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
