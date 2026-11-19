//! An accepted table can separate declarations only at the outer HEAD level.
use super::super::{
    annotations::{self, Boundary as MarkBoundary, Mark, OpenMark},
    invocation,
};
use crate::definitions::syntax::scan::{MAX_NESTING, closing, quote_closer};
use crate::definitions::{NativeOperand, NativeOperandRole};
use std::ops::Range;

pub(super) struct Boundary {
    pub(super) bytes: Range<usize>,
    pub(super) kind: BoundaryKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BoundaryKind {
    Display,
    EnclosedDisplay,
    Annotation,
}

pub(super) struct HeadPartition {
    pub(super) omitted: Vec<Boundary>,
    pub(super) separators: Vec<Separator>,
}

pub(super) struct Separator {
    pub(super) bytes: Range<usize>,
    pub(super) alternative: bool,
    pub(super) invocation_start: Option<usize>,
}

pub(super) fn display_boundaries(
    text: &str,
    operands: &[NativeOperand],
    bodies: &[Range<usize>],
) -> HeadPartition {
    let mut scan = ScopeScan {
        text,
        ..ScopeScan::default()
    };
    let mut end = 0;
    let mut bodies = bodies.iter().peekable();
    let mut active_body = None;
    let mut syntax = invocation::Syntax::new(text, operands);
    for operand in operands {
        if operand.bytes.start < end {
            continue;
        }
        // Scan ordinary syntax around the native BODY. Inside it, the
        // ordinary grammar sees the retained full scope in one group.
        // Generated Fn parentheses must not start a new annotation here.
        let mut gap = active_body.as_ref().map_or(end, |body: &&Range<usize>| {
            end.max(body.end.min(operand.bytes.start))
        });
        if active_body.is_none_or(|body| body.end <= operand.bytes.start) {
            active_body = None;
            while bodies
                .next_if(|body| body.end <= operand.bytes.start)
                .is_some()
            {}
            if let Some(body) = bodies.next_if(|body| body.contains(&operand.bytes.start)) {
                scan.literal_marks(gap..body.start.max(gap));
                if scan.scopes.is_empty() && scan.has_prefix_before(body.start) {
                    scan.retain_prefix();
                    end = body.end;
                    scan.cut(body.clone(), BoundaryKind::Annotation);
                    continue;
                }
                active_body = Some(body);
                gap = operand.bytes.start;
            }
        }
        scan.literal_marks(gap..operand.bytes.start);
        scan.operand(operand, active_body.is_some(), &mut syntax);
        end = operand.bytes.end;
    }
    scan.literal_marks(active_body.map_or(end, |body| end.max(body.end))..text.len());
    if !scan.scopes.is_empty() && scan.opaque {
        scan.cut(scan.start..text.len(), BoundaryKind::Annotation);
    }
    HeadPartition {
        omitted: scan.boundaries,
        separators: scan.separators,
    }
}

#[derive(Default)]
struct ScopeScan<'a> {
    text: &'a str,
    boundaries: Vec<Boundary>,
    separators: Vec<Separator>,
    scopes: Vec<OpenMark<'a>>,
    start: usize,
    top_level_start: usize,
    invocation_start: usize,
    signature_depth: usize,
    slot: SlotPrefix,
    opaque: bool,
    exhausted: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum SlotPrefix {
    #[default]
    Empty,
    Text,
    Option,
}

impl<'a> ScopeScan<'a> {
    fn cut(&mut self, bytes: Range<usize>, kind: BoundaryKind) {
        if kind != BoundaryKind::EnclosedDisplay {
            self.top_level_start = bytes.end;
        }
        self.boundaries.push(Boundary { bytes, kind });
    }

    fn has_prefix_before(&self, end: usize) -> bool {
        self.slot != SlotPrefix::Empty
            || self
                .text
                .get(self.top_level_start..end)
                .is_some_and(|text| !text.trim().is_empty())
    }

    fn retain_prefix(&mut self) {
        if self.slot == SlotPrefix::Empty {
            self.slot = SlotPrefix::Text;
        }
    }

    fn separator(&mut self, bytes: Range<usize>, alternative: bool, continues_invocation: bool) {
        let invocation_start = continues_invocation.then_some(self.invocation_start);
        if invocation_start.is_none() && (!alternative || self.slot == SlotPrefix::Option) {
            // The next proved native BODY starts a new declaration slot.
            // Older words only turn a lexical scope into an annotation
            // when they still belong to the current slot.
            self.top_level_start = bytes.end;
            self.invocation_start = bytes.end;
            self.slot = SlotPrefix::Empty;
        }
        self.separators.push(Separator {
            bytes,
            alternative,
            invocation_start,
        });
    }

    fn operand(
        &mut self,
        operand: &NativeOperand,
        within_native_body: bool,
        syntax: &mut invocation::Syntax<'_>,
    ) {
        let role = operand.role;
        let bytes = operand.bytes.clone();
        if role.display_only() {
            if self.signature_depth == 0 && self.scopes.is_empty() && !within_native_body {
                if self.has_prefix_before(bytes.start) {
                    self.retain_prefix();
                }
                self.cut(bytes, BoundaryKind::Display);
            } else if !self.opaque || self.scopes.is_empty() {
                self.cut(bytes, BoundaryKind::EnclosedDisplay);
            }
        } else if let NativeOperandRole::FunctionSignature { opening } = role {
            // mdoc_term.c::termp_fn_pre/termp_fo_pre write the signature's
            // own delimiters. They do not open a supplementary annotation.
            if opening {
                self.signature_depth += 1;
            } else {
                self.signature_depth = self.signature_depth.saturating_sub(1);
            }
        } else if self.signature_depth == 0 {
            if role == NativeOperandRole::ExplicitOption && !self.opaque {
                self.slot = SlotPrefix::Option;
            }
            self.accept_non_display(operand, bytes, within_native_body, syntax);
        }
    }

    fn accept_non_display(
        &mut self,
        operand: &NativeOperand,
        bytes: Range<usize>,
        within_native_body: bool,
        syntax: &mut invocation::Syntax<'_>,
    ) {
        let role = operand.role;
        let mark_start = bytes.end - self.text[bytes.clone()].trim_start().len();
        if let Some(mark) = annotations::marker(self.text, mark_start, operand) {
            self.mark(mark, bytes);
            return;
        }
        let word = self.text[mark_start..bytes.end].trim_end();
        if !within_native_body
            && self.scopes.is_empty()
            && matches!(
                role,
                NativeOperandRole::PipeDelimiter(None)
                    | NativeOperandRole::LiteralContinuation(None)
            )
            && matches!(word, "," | "|" | "or")
        {
            let alternative = word == "or";
            self.separator(
                mark_start..mark_start + word.len(),
                alternative,
                !alternative && syntax.protects(self.invocation_start, mark_start),
            );
            return;
        }
        if let Some(active) = self.scopes.last_mut()
            && let Some(body) = annotations::supplementary_body(role)
        {
            active.admits_scope(Some(body), true);
        }
        if !within_native_body
            && matches!(
                role,
                NativeOperandRole::Literal
                    | NativeOperandRole::LiteralContinuation(_)
                    | NativeOperandRole::PipeDelimiter(_)
                    | NativeOperandRole::LiteralPunctuation
                    | NativeOperandRole::EnclosureContent(_)
            )
        {
            self.literal_marks(bytes);
        }
    }

    fn mark(&mut self, mark: Mark<'a>, bytes: Range<usize>) {
        if self.exhausted {
            return;
        }
        match mark {
            Mark::Open(boundary, parent) => {
                if self.scopes.is_empty() {
                    self.start = bytes.start;
                    self.opaque = self.has_prefix_before(self.start);
                    if self.opaque {
                        self.retain_prefix();
                    }
                } else if let Some(active) = self.scopes.last_mut() {
                    active.admits_scope(parent, true);
                }
                if self.scopes.len() == MAX_NESTING {
                    self.exhausted = true;
                } else {
                    self.scopes.push(OpenMark::new(boundary, parent));
                }
            }
            Mark::Close(boundary)
                if self
                    .scopes
                    .last()
                    .is_some_and(|open| open.closes(boundary, true)) =>
            {
                self.scopes.pop();
                if self.scopes.is_empty() && std::mem::take(&mut self.opaque) {
                    self.cut(self.start..bytes.end, BoundaryKind::Annotation);
                }
            }
            Mark::Close(_) => {}
        }
    }

    fn literal_marks(&mut self, range: Range<usize>) {
        if self.signature_depth > 0 {
            return;
        }
        let text = self.text;
        for (index, glyph) in text[range.clone()].char_indices() {
            if self.exhausted {
                return;
            }
            let start = range.start + index;
            let end = start + glyph.len_utf8();
            if let Some(mark) = self.literal_mark(glyph, start, end) {
                self.mark(mark, start..end);
            }
        }
    }

    fn literal_mark(&self, glyph: char, start: usize, end: usize) -> Option<Mark<'a>> {
        let active = self.scopes.last();
        // An apostrophe inside an ordinary word supplies no quote boundary.
        if glyph == '\''
            && self.text[..start].ends_with(char::is_alphanumeric)
            && self.text[end..].starts_with(char::is_alphanumeric)
        {
            return None;
        }
        let close = MarkBoundary::Literal(active.and_then(OpenMark::enclosing), glyph);
        if active.is_some_and(|mark| mark.closes(close, true)) {
            Some(Mark::Close(close))
        } else if !active.is_some_and(OpenMark::is_literal_quote)
            && (closing(glyph).is_some() || quote_closer(glyph).is_some())
        {
            let scope = active.and_then(OpenMark::active_scope);
            Some(Mark::Open(MarkBoundary::Literal(scope, glyph), scope))
        } else {
            None
        }
    }
}
