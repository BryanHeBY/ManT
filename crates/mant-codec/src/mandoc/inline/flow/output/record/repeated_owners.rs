//! Bound redundant receipts left by repeated edits to a detached run-in.
use std::collections::HashMap;

use super::super::super::{NativeWordAnchor, WordProjectionPolicy};

/// Keep the old checkpoint, but do not revisit clean owner history.
pub(super) fn compact_at_checkpoint(
    anchors: &mut Vec<NativeWordAnchor>,
    dirty: &mut Option<usize>,
    serial: u64,
) {
    if serial.is_multiple_of(16)
        && anchors.len() >= 64
        && let Some(from) = dirty.take()
    {
        compact_neutral_copies(anchors, from);
    }
}

pub(super) fn widen_dirty_suffix(dirty: &mut Option<usize>, changed: Option<usize>) {
    if let Some(changed) = changed {
        *dirty = Some(dirty.map_or(changed, |prior| prior.min(changed)));
    }
}

/// Only remove an interior copy of the same source receipt in a contiguous
/// zero-length run. Every interval removed ends at this same coordinate, so
/// it cannot paint, own a separator, or mark an accepted row. The unchanged
/// first and last copies preserve source-resolving and reverse word lookup
/// ties; keep the original relative sequence of all other receipts.
fn compact_neutral_copies(anchors: &mut Vec<NativeWordAnchor>, mut from: usize) {
    debug_assert!(from < anchors.len());
    // A dirty key can extend an earlier run across the suffix boundary.
    // Include the entire matching run so its original first/reverse ties
    // still arbitrate duplicates. All other earlier runs are unaffected.
    if let Some(key) = neutral_key(&anchors[from]) {
        while from > 0 && neutral_key(&anchors[from - 1]) == Some(key) {
            from -= 1;
        }
    }
    let scanned = anchors.len() - from;
    let mut drop = vec![false; scanned];
    let mut next = from;
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
        retain_run_endpoints(&anchors[start..next], &mut drop[start - from..next - from]);
    }
    // Do not rewrite or traverse the prefix with Vec::retain. Stable
    // forward swaps leave the undropped suffix in its original order.
    let mut write = from;
    #[cfg(test)]
    let mut moved = 0;
    for (read, drop) in (from..anchors.len()).zip(drop) {
        if !drop {
            if write != read {
                anchors.swap(write, read);
                #[cfg(test)]
                {
                    moved += 1;
                }
            }
            write += 1;
        }
    }
    #[cfg(test)]
    tests::count_compaction(scanned, write - from, moved);
    anchors.truncate(write);
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
        mask_slots: usize,
        moved: usize,
    }

    thread_local! {
        static COSTS: std::cell::Cell<Option<Costs>> = const { std::cell::Cell::new(None) };
    }

    pub(super) fn count_compaction(before: usize, after: usize, moved: usize) {
        COSTS.with(|slot| {
            if let Some(mut count) = slot.get() {
                count.calls += 1;
                count.scanned += before;
                count.mask_slots += before;
                count.moved += moved;
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
        assert!(short.scanned <= 372 && long.scanned <= 3549);

        // The old periodic scan kept visiting ordinary appended words
        // forever after a single replay. Pending dirt may cross the small
        // tape threshold, but no subsequent fresh owner restarts scanning.
        let once = ".No \"\\h'|0n'X\"\n";
        let once_short = parse_costs(head, once, ".No Y\n", 384);
        let once_long = parse_costs(head, once, ".No Y\n", 4096);
        assert!(once_short.calls <= 1, "{once_short:?}");
        assert_eq!(once_short, once_long);
    }

    #[test]
    fn sparse_local_rewind_does_not_scan_or_move_unrelated_receipts() {
        let initial = "Y\n".repeat(64);
        for retreat in [3, 4] {
            let repaint = format!("{}.No \"\\h'-{retreat}n'X\"\n", "Y\n".repeat(7));
            for repeats in [8, 32, 128, 512, 2048] {
                let costs = parse_costs("No HEAD", &initial, &repaint, repeats);
                eprintln!("retreat={retreat} groups={repeats} {costs:?}");
                assert!(costs.calls > 0, "{retreat} {repeats}: {costs:?}");
                assert!(
                    costs.scanned <= 48 * costs.calls,
                    "{retreat} {repeats}: {costs:?}"
                );
                assert_eq!(costs.scanned, costs.mask_slots);
                assert_eq!((costs.removed, costs.moved), (0, 0));
            }
        }
    }

    fn receipt(owner: &str, coordinate: usize) -> NativeWordAnchor {
        NativeWordAnchor {
            start: coordinate,
            owner: owner.to_owned(),
            content: coordinate,
            may_relocate_navigation_target: false,
            writer_epoch: 0,
            projected_device_padding: 0,
            projected_field_prefix: false,
            projected_gap: ProjectedNativeGap::default(),
            projection: WordProjectionPolicy::Printed,
        }
    }

    #[test]
    fn a_run_crossing_the_dirty_boundary_keeps_complete_ties() {
        let mut before: Vec<_> = (0..128)
            .map(|cell| receipt(&format!("earlier-{cell}"), cell))
            .collect();
        before.extend([
            receipt("same", 130),
            receipt("independent", 130),
            receipt("same", 130),
        ]);
        let from = before.len();
        before.extend([receipt("same", 130), receipt("later", 131)]);
        let mut whole = before.clone();
        compact_neutral_copies(&mut whole, 0);
        compact_neutral_copies(&mut before, from);
        assert_eq!(
            before
                .iter()
                .map(|item| (&item.owner, item.content))
                .collect::<Vec<_>>(),
            whole
                .iter()
                .map(|item| (&item.owner, item.content))
                .collect::<Vec<_>>(),
        );
        assert_eq!(before.len(), 132);
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
        compact_neutral_copies(&mut after, 0);
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
