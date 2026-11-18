//! Declaration groups bridge only proved enclosure or separator boundaries.
use super::scopes::{Boundary, BoundaryKind, HeadPartition, Separator};
use crate::definitions::syntax::eligibility::intersecting_operands;
use crate::definitions::{NativeOperand, NativeOperandRole};
use std::ops::Range;

pub(super) fn retained_intervals(
    text: &str,
    operands: &[NativeOperand],
    partition: HeadPartition,
) -> Vec<(usize, Range<usize>)> {
    let mut plan = GroupPlan::default();
    let mut start = 0;
    for boundary in partition
        .omitted
        .into_iter()
        .chain(std::iter::once(Boundary {
            bytes: text.len()..text.len(),
            kind: BoundaryKind::Display,
        }))
    {
        plan.retain(
            text,
            operands,
            &partition.separators,
            start..boundary.bytes.start,
        );
        plan.join = match boundary.kind {
            BoundaryKind::EnclosedDisplay => Join::Enclosure,
            BoundaryKind::Annotation => Join::AwaitSeparator,
            BoundaryKind::Display if plan.join == Join::AwaitSeparator => Join::AwaitSeparator,
            BoundaryKind::Display => Join::AcrossDisplay,
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
    admits_alternative: bool,
    trailing_separator: bool,
}

#[derive(Default, PartialEq, Eq)]
enum Join {
    #[default]
    AcrossDisplay,
    Enclosure,
    AwaitSeparator,
}

impl GroupPlan {
    fn retain(
        &mut self,
        text: &str,
        operands: &[NativeOperand],
        separators: &[Separator],
        mut bytes: Range<usize>,
    ) {
        let visible = text[bytes.clone()].trim();
        if visible.is_empty() {
            return;
        }
        let first = visible.as_ptr() as usize - text.as_ptr() as usize;
        let index = separators.partition_point(|separator| separator.bytes.start < first);
        let leading = separators
            .get(index)
            .filter(|separator| separator.bytes.start == first);
        let joins = self.join == Join::Enclosure
            || self.admits_alternative && leading.is_some_and(|sep| sep.alternative)
            || self.join == Join::AcrossDisplay
                && self.group > 0
                && (self.trailing_separator || leading.is_some_and(|sep| !sep.alternative));
        if !joins
            && (self.join == Join::AwaitSeparator || leading.is_some_and(|sep| sep.alternative))
        {
            // A rejected connector or annotation cannot start a name, but
            // a later top-level separator can introduce a fresh sibling.
            let Some(separator) = separators[index..]
                .iter()
                .take_while(|separator| separator.bytes.end <= bytes.end)
                .find(|separator| !separator.alternative)
            else {
                return;
            };
            bytes.start = separator.bytes.end;
            self.join = Join::AcrossDisplay;
            if text[bytes.clone()].trim().is_empty() {
                return;
            }
        }
        if !joins {
            self.group += 1;
            self.admits_alternative = false;
        }
        self.admits_alternative |= intersecting_operands(operands, bytes.clone()).any(|operand| {
            matches!(
                operand.role,
                NativeOperandRole::Environment { .. } | NativeOperandRole::ExplicitOption
            )
        });
        let end = first + visible.len();
        let after = separators.partition_point(|separator| separator.bytes.end <= end);
        self.trailing_separator = separators[..after].last().is_some_and(|separator| {
            separator.bytes.end == end && (!separator.alternative || self.admits_alternative)
        });
        self.join = Join::AcrossDisplay;
        let written = text[bytes.clone()].trim();
        let start = written.as_ptr() as usize - text.as_ptr() as usize;
        self.intervals
            .push((self.group, start..start + written.len()));
    }
}
