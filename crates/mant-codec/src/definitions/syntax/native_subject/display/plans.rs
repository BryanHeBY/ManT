//! Declaration groups bridge only proved enclosure or separator boundaries.
use super::scopes::{Boundary, BoundaryKind};
use crate::definitions::syntax::eligibility::{intersecting_operands, operand_at};
use crate::definitions::{NativeOperand, NativeOperandRole};
use std::ops::Range;

pub(super) fn retained_intervals(
    text: &str,
    operands: &[NativeOperand],
    boundaries: Vec<Boundary>,
) -> Vec<(usize, Range<usize>)> {
    let mut plan = GroupPlan::default();
    let mut start = 0;
    for boundary in boundaries.into_iter().chain(std::iter::once(Boundary {
        bytes: text.len()..text.len(),
        kind: BoundaryKind::Display,
    })) {
        plan.retain(text, operands, start..boundary.bytes.start);
        plan.join = match boundary.kind {
            BoundaryKind::EnclosedDisplay => Join::Enclosure,
            BoundaryKind::Annotation => Join::AwaitSeparator,
            BoundaryKind::Display if plan.join == Join::AwaitSeparator => Join::AwaitSeparator,
            BoundaryKind::Display => Join::Independent,
        };
        start = boundary.bytes.end;
    }
    plan.intervals
}

#[derive(Default)]
struct GroupPlan {
    intervals: Vec<(usize, Range<usize>)>,
    group: usize,
    join: Join,
    has_ev: bool,
    ends_in_or: bool,
}

#[derive(Default, PartialEq, Eq)]
enum Join {
    #[default]
    Independent,
    Enclosure,
    AwaitSeparator,
}

impl GroupPlan {
    fn retain(&mut self, text: &str, operands: &[NativeOperand], mut bytes: Range<usize>) {
        let visible = text[bytes.clone()].trim();
        if visible.is_empty() {
            return;
        }
        let first = visible.as_ptr() as usize - text.as_ptr() as usize;
        let starts_in_or =
            visible.split_whitespace().next() == Some("or") && connector(text, operands, first);
        let joins =
            self.join == Join::Enclosure || self.has_ev && (starts_in_or || self.ends_in_or);
        if !joins && starts_in_or {
            // A connector with no proved left-hand declaration cannot
            // become a lexical head (e.g. "or NEXT" as an invocation).
            return;
        }
        if !joins && self.join == Join::AwaitSeparator {
            let Some(after) = text[first..bytes.end].strip_prefix([',', '|']) else {
                return;
            };
            bytes.start = bytes.end - after.len();
            self.join = Join::Independent;
            if after.trim().is_empty() {
                return;
            }
        }
        if !joins {
            self.group += 1;
            self.has_ev = false;
        }
        self.has_ev |= intersecting_operands(operands, bytes.clone())
            .any(|operand| matches!(operand.role, NativeOperandRole::Environment { .. }));
        self.ends_in_or = visible.ends_with("or")
            && connector(text, operands, first + visible.len() - "or".len());
        self.join = Join::Independent;
        let written = text[bytes.clone()].trim();
        let start = written.as_ptr() as usize - text.as_ptr() as usize;
        self.intervals
            .push((self.group, start..start + written.len()));
    }
}

fn connector(text: &str, operands: &[NativeOperand], start: usize) -> bool {
    operand_at(operands, start).is_some_and(|operand| {
        matches!(operand.role, NativeOperandRole::LiteralContinuation(_))
            && text[operand.bytes.clone()].trim() == "or"
    })
}
