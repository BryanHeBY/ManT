//! Bound redundant receipts left by repeated edits to a detached run-in.
use std::collections::HashMap;

use super::super::super::{NativeWordAnchor, WordProjectionPolicy};

/// Keep the old checkpoint, but do not revisit clean owner history.
pub(super) fn compact_at_checkpoint(
    anchors: &mut Vec<NativeWordAnchor>,
    dirty: &mut bool,
    serial: u64,
) {
    if *dirty && serial.is_multiple_of(16) && anchors.len() >= 64 {
        compact_neutral_copies(anchors);
        *dirty = false;
    }
}

/// Only remove an interior copy of the same source receipt in a contiguous
/// zero-length run. Every interval removed ends at this same coordinate, so
/// it cannot paint, own a separator, or mark an accepted row. The unchanged
/// first and last copies preserve source-resolving and reverse word lookup
/// ties; keep the original relative sequence of all other receipts.
pub(super) fn compact_neutral_copies(anchors: &mut Vec<NativeWordAnchor>) {
    if anchors.len() < 64 {
        return;
    }
    #[cfg(test)]
    let before = anchors.len();
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
    #[cfg(test)]
    tests::count_compaction(before, anchors.len());
}

pub(super) fn neutral_key(anchor: &NativeWordAnchor) -> Option<usize> {
    neutral(anchor).then_some(anchor.content)
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

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Costs {
        calls: usize,
        scanned: usize,
        removed: usize,
    }

    thread_local! {
        static COSTS: std::cell::Cell<Option<Costs>> = const { std::cell::Cell::new(None) };
    }

    pub(super) fn count_compaction(before: usize, after: usize) {
        COSTS.with(|slot| {
            if let Some(mut count) = slot.get() {
                count.calls += 1;
                count.scanned += before;
                count.removed += before - after;
                slot.set(Some(count));
            }
        });
    }

    fn parse_costs(head: &str, initial: &str, words: &str, repeats: usize) -> Costs {
        let mut page = format!(
            ".Dd November 27, 2026\n.Dt GLYPHS 1\n.Os\n.Sh NAME\n.Nm glyphs\n\
             .Nd small roff inputs\n.Sh DESCRIPTION\n.ft I\n.Bl -inset\n.It {head}\n{initial}",
        );
        for _ in 0..repeats {
            page.push_str(words);
        }
        page.push_str(".El\n.Sh ENDTEST\nDone.\n");
        COSTS.with(|slot| slot.set(Some(Costs::default())));
        let parsed =
            crate::mandoc::parse_plain_manual(std::path::Path::new("glyphs.1"), page.as_bytes());
        let costs = COSTS.with(|slot| slot.take().expect("set before parsing"));
        assert!(parsed.is_ok(), "{parsed:?}");
        costs
    }

    #[test]
    fn detached_body_scans_only_after_an_owner_receipt_edit() {
        for repeats in [1024, 4096] {
            for (initial, words) in [("", "Y\n"), ("", ".No Y\n"), (".br\n", "Y\n")] {
                assert_eq!(
                    parse_costs("No HEAD", initial, words, repeats),
                    Costs::default()
                );
            }
            assert_eq!(
                parse_costs("No HEAD", "", ".No \"\\h'0n'Y\"\n", repeats),
                Costs::default(),
            );
        }

        let head = "No Q\\z Ap No A";
        let repaint = ".No Y\n.No \"\\h'|0n'X\"\n";
        let short = parse_costs(head, "", repaint, 24);
        let long = parse_costs(head, "", repaint, 96);
        assert_eq!((short.calls, short.removed), (2, 210), "{short:?}");
        assert_eq!((long.calls, long.removed), (11, 1074), "{long:?}");
        assert_eq!((short.scanned, long.scanned), (372, 3549));

        // The old periodic scan kept visiting ordinary appended words
        // forever after a single replay. Pending dirt may cross the small
        // tape threshold, but no subsequent fresh owner restarts scanning.
        let once = ".No \"\\h'|0n'X\"\n";
        let once_short = parse_costs(head, once, ".No Y\n", 384);
        let once_long = parse_costs(head, once, ".No Y\n", 4096);
        assert!(once_short.calls <= 1, "{once_short:?}");
        assert_eq!(once_short, once_long);
    }

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
