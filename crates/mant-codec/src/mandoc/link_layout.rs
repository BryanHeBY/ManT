//! Seal native description receipts to final structural link locations.
use std::ops::{ControlFlow, Range};

use mant_ir::{
    Document, Inline, LinkLayout, LinkLayoutReceipt, NavigationEvent, NavigationScanOptions,
    ReferenceScanLimits, ReferenceScope, ReferenceWorkBudget, scan_navigation_scope_with_budget,
    visit::{self, VisitMut},
};

const TOKEN: &str = "\0mant:link-layout:";
const MAX_LABEL: usize = 4096;
const MAX_RUNS: usize = 64;
const MAX_RECEIPTS: usize = 256;

pub(super) fn marker(
    occurrence: impl std::fmt::Display,
    children: &[Inline],
    runs: &[Range<usize>],
) -> Option<String> {
    if !(2..=MAX_RUNS).contains(&runs.len()) {
        return None;
    }
    let expected = mant_ir::inline_plain_text(children);
    if expected.len() > MAX_LABEL {
        return None;
    }
    let mut marker = format!("{TOKEN}{occurrence};");
    for range in runs {
        use std::fmt::Write;
        let _ = write!(marker, "{}:{},", range.start, range.end);
    }
    marker.push(';');
    marker.push_str(&expected);
    Some(marker)
}

pub(super) fn is_marker(title: &str) -> bool {
    title.starts_with(TOKEN)
}

fn decode(title: &str, children: &[Inline]) -> Option<(u32, Vec<Range<usize>>)> {
    let mut fields = title.strip_prefix(TOKEN)?.splitn(3, ';');
    let occurrence = fields.next()?.parse().ok()?;
    let members = fields.next()?.strip_suffix(',')?;
    let expected = fields.next()?;
    if expected.len() > MAX_LABEL || expected != mant_ir::inline_plain_text(children) {
        return None;
    }
    let len = expected.chars().count();
    let mut ranges = Vec::new();
    for member in members.split(',') {
        let (start, end) = member.split_once(':')?;
        let (start, end) = (start.parse().ok()?, end.parse().ok()?);
        if start >= end
            || end > len
            || ranges
                .last()
                .is_some_and(|previous: &Range<usize>| previous.end >= start)
            || ranges.len() == MAX_RUNS
        {
            return None;
        }
        ranges.push(start..end);
    }
    (ranges.len() >= 2).then_some((occurrence, ranges))
}

pub(super) fn seal(document: &mut Document) -> LinkLayout {
    let mut receipts = Vec::new();
    let mut budget = ReferenceWorkBudget::new(ReferenceScanLimits::default());
    let report = scan_navigation_scope_with_budget(
        document,
        ReferenceScope::Document,
        &mut budget,
        NavigationScanOptions {
            targets: false,
            entry_sets: false,
            ..Default::default()
        },
        |event, _| {
            if let NavigationEvent::Link(link) = event
                && let Inline::Link {
                    title: Some(title),
                    children,
                    ..
                } = link.link
                && let Some((occurrence, members)) = decode(title, children)
                && let Some(origin) = link.location.to_owned()
            {
                if receipts.len() == MAX_RECEIPTS {
                    return ControlFlow::Break(());
                }
                receipts.push(LinkLayoutReceipt {
                    occurrence,
                    members,
                    origin,
                    target: link.target.clone(),
                    expected_children: children.clone(),
                });
            }
            ControlFlow::Continue(())
        },
    );
    Remove.visit_document_mut(document);
    if !report.complete() {
        receipts.clear();
    }
    let mut seen = std::collections::HashSet::new();
    if !receipts
        .iter()
        .all(|receipt| seen.insert(receipt.occurrence))
    {
        receipts.clear();
    }
    LinkLayout { receipts }
}

struct Remove;
impl VisitMut for Remove {
    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        if let Inline::Link { title, .. } = inline
            && title.as_deref().is_some_and(is_marker)
        {
            *title = None;
        }
        visit::walk_inline_mut(self, inline);
    }
}
