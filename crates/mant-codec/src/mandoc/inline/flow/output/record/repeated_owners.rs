//! Bound redundant receipts left by repeated edits to a detached run-in.
use std::collections::HashMap;

use super::super::super::{NativeWordAnchor, WordProjectionPolicy};

/// Only remove an interior copy of the same source receipt in a contiguous
/// zero-length run. Every interval removed ends at this same coordinate, so
/// it cannot paint, own a separator, or mark an accepted row. The unchanged
/// first and last copies preserve source-resolving and reverse word lookup
/// ties; keep the original relative sequence of all other receipts.
pub(super) fn compact_neutral_copies(anchors: &mut Vec<NativeWordAnchor>) {
    if anchors.len() < 64 {
        return;
    }
    let mut drop = vec![false; anchors.len()];
    let mut next = 0;
    while let Some(first) = anchors.get(next) {
        let start = next;
        next += 1;
        if !neutral(first) {
            continue;
        }
        while anchors
            .get(next)
            .is_some_and(|anchor| anchor.content == first.content && neutral(anchor))
        {
            next += 1;
        }
        retain_run_endpoints(&anchors[start..next], &mut drop[start..next]);
    }
    let mut mask = drop.into_iter();
    anchors.retain(|_| !mask.next().expect("unchanged receipt count"));
}

fn neutral(anchor: &NativeWordAnchor) -> bool {
    anchor.start == anchor.content
        && anchor.projected_device_padding == 0
        && !anchor.projected_field_prefix
        && anchor.projected_gap.cell.is_none()
        && anchor.projected_gap.motion_span.is_none()
        && !anchor.projected_gap.superseded
        && anchor.projection == WordProjectionPolicy::Printed
}

fn retain_run_endpoints(run: &[NativeWordAnchor], drop: &mut [bool]) {
    if run.len() < 3 {
        return;
    }
    let mut first_and_latest = HashMap::<&str, (usize, usize)>::new();
    for (index, anchor) in run.iter().enumerate() {
        let (first, pending) = first_and_latest
            .entry(&anchor.owner)
            .or_insert((index, index));
        if first != pending {
            drop[*pending] = true;
        }
        *pending = index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mandoc::inline::flow::ProjectedNativeGap;

    fn receipt(owner: &str, coordinate: usize) -> NativeWordAnchor {
        NativeWordAnchor {
            start: coordinate,
            owner: owner.to_owned(),
            content: coordinate,
            projected_device_padding: 0,
            projected_field_prefix: false,
            projected_gap: ProjectedNativeGap::default(),
            projection: WordProjectionPolicy::Printed,
        }
    }

    #[test]
    fn preserves_both_ties_distinct_empty_ids_and_earlier_live_fragment() {
        let mut before = vec![receipt("earlier", 0)];
        before.extend((0..65).flat_map(|_| [receipt("alias", 3), receipt("word", 3)]));
        let mut gap = receipt("word", 3);
        gap.projected_gap.cell = Some(1);
        before.push(gap);
        before.extend((0..65).map(|_| receipt("word", 3)));
        before.push(receipt("inkless", 3));
        let mut after = before.clone();
        compact_neutral_copies(&mut after);
        assert_eq!(
            after
                .iter()
                .map(|item| item.owner.as_str())
                .collect::<Vec<_>>(),
            [
                "earlier", "alias", "word", "alias", "word", "word", "word", "word", "inkless"
            ],
        );
        let lookup = |anchors: &[NativeWordAnchor], cell: usize, reverse: bool| {
            let mut ties = anchors.iter().filter(|anchor| anchor.content <= cell);
            (if reverse {
                ties.next_back()
            } else {
                ties.next()
            })
            .map(|anchor| anchor.owner.clone())
        };
        for cell in 0..8 {
            for reverse in [true, false] {
                assert_eq!(
                    lookup(&before, cell, reverse),
                    lookup(&after, cell, reverse)
                );
            }
        }
        assert_eq!(after[5].projected_gap.cell, Some(1));
    }
}
