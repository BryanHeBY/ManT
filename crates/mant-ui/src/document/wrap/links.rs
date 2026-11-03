//! Ordered interval events preserve whole-grapheme link conflict semantics.

use std::collections::{BTreeMap, BTreeSet};

use super::super::{LinkTarget, LogicalLinkRange};

pub(super) struct LinkCursor<'links> {
    links: &'links [LogicalLinkRange],
    starts: Vec<usize>,
    ends: Vec<usize>,
    groups: Vec<usize>,
    counts: Vec<usize>,
    active: BTreeSet<usize>,
    active_groups: usize,
    next_start: usize,
    next_end: usize,
    #[cfg(test)]
    events: usize,
}

impl<'links> LinkCursor<'links> {
    pub(super) fn new(links: &'links [LogicalLinkRange]) -> Self {
        let mut targets: BTreeMap<&LinkTarget, usize> = BTreeMap::new();
        let groups = links
            .iter()
            .map(|link| {
                let next = targets.len();
                *targets.entry(&link.target).or_insert(next)
            })
            .collect();
        let mut starts: Vec<_> = (0..links.len())
            .filter(|index| links[*index].start_scalar < links[*index].end_scalar)
            .collect();
        let mut ends = starts.clone();
        starts.sort_unstable_by_key(|index| (links[*index].start_scalar, *index));
        ends.sort_unstable_by_key(|index| (links[*index].end_scalar, *index));
        Self {
            links,
            starts,
            ends,
            groups,
            counts: vec![0; targets.len()],
            active: BTreeSet::new(),
            active_groups: 0,
            next_start: 0,
            next_end: 0,
            #[cfg(test)]
            events: 0,
        }
    }

    /// Visit monotonically advancing, complete source graphemes. Intervals
    /// ending or beginning inside one glyph both participate in its decision.
    pub(super) fn grapheme(&mut self, start: usize, scalars: usize) -> Option<usize> {
        while let Some(index) = self.ends.get(self.next_end).copied()
            && self.links[index].end_scalar <= start
        {
            self.next_end += 1;
            if self.active.remove(&index) {
                let count = &mut self.counts[self.groups[index]];
                *count -= 1;
                if *count == 0 {
                    self.active_groups -= 1;
                }
            }
            #[cfg(test)]
            {
                self.events += 1;
            }
        }
        while let Some(index) = self.starts.get(self.next_start).copied()
            && self.links[index].start_scalar < start.saturating_add(scalars)
        {
            self.next_start += 1;
            // Ignore intervals before the visited input, for direct callers
            // starting after scalar zero as well as empty/invalid ranges.
            if self.links[index].end_scalar > start {
                self.active.insert(index);
                let count = &mut self.counts[self.groups[index]];
                if *count == 0 {
                    self.active_groups += 1;
                }
                *count += 1;
            }
            #[cfg(test)]
            {
                self.events += 1;
            }
        }
        (self.active_groups == 1)
            .then(|| self.active.first().copied())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str) -> LinkTarget {
        LinkTarget::Manual {
            name: name.into(),
            manual_section: Some("1".into()),
        }
    }

    #[test]
    fn interval_cursor_matches_full_scan_for_overlaps_and_shared_glyphs() {
        for length in 1..8 {
            for split in 0..length {
                let links = vec![
                    LogicalLinkRange {
                        target: target("a"),
                        start_scalar: split,
                        end_scalar: length,
                    },
                    LogicalLinkRange {
                        target: target("a"),
                        start_scalar: 0,
                        end_scalar: length,
                    },
                    LogicalLinkRange {
                        target: target("b"),
                        start_scalar: 1,
                        end_scalar: split,
                    },
                ];
                for cluster in 1..=3 {
                    let mut cursor = LinkCursor::new(&links);
                    for start in (0..length).step_by(cluster) {
                        let mut matches = links.iter().enumerate().filter(|(_, link)| {
                            link.start_scalar < start + cluster
                                && link.end_scalar > start
                                && link.start_scalar < link.end_scalar
                        });
                        let expected = matches.next().and_then(|(index, first)| {
                            matches
                                .all(|(_, other)| other.target == first.target)
                                .then_some(index)
                        });
                        assert_eq!(cursor.grapheme(start, cluster), expected);
                    }
                    assert!(cursor.events <= 2 * links.len());
                }
            }
        }
    }

    #[test]
    fn adjacent_and_fully_overlapping_links_use_bounded_interval_events() {
        for count in [256, 512, 4096] {
            for overlap in [false, true] {
                let links: Vec<_> = (0..count)
                    .map(|index| LogicalLinkRange {
                        target: target(&if overlap {
                            "same".into()
                        } else {
                            format!("target-{index}")
                        }),
                        start_scalar: if overlap { 0 } else { index * 4 },
                        end_scalar: if overlap { count * 4 } else { (index + 1) * 4 },
                    })
                    .collect();
                let mut cursor = LinkCursor::new(&links);
                for start in 0..count * 4 {
                    assert!(cursor.grapheme(start, 1).is_some());
                }
                assert!(cursor.events <= 2 * count);
            }
        }
    }
}
