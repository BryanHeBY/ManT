//! Exact deletion witnesses for bounded column-cell row sequences.

use super::super::rows::CellRow;
use std::{collections::BTreeMap, ops::Range};

const MAX_DELETIONS: usize = 8;
const MAX_STATES: usize = 65_536;
type Band = [Option<MissingRows>; MAX_DELETIONS + 1];

#[derive(Clone, Copy, Default)]
pub(super) struct MissingRows {
    pub(super) min: usize,
    pub(super) max: usize,
}

impl MissingRows {
    fn include(slot: &mut Option<Self>, range: Self) {
        *slot = Some(slot.map_or(range, |other| Self {
            min: other.min.min(range.min),
            max: other.max.max(range.max),
        }));
    }

    fn increment(self, counted: bool) -> Self {
        let count = usize::from(counted);
        Self {
            min: self.min + count,
            max: self.max + count,
        }
    }
}

pub(super) struct DeletionWitnesses {
    pub(super) total: MissingRows,
    pub(super) displays: BTreeMap<usize, MissingRows>,
}

pub(super) fn same_slot(row: &CellRow, (text, literal): (&str, bool)) -> bool {
    row.text == text && (row.owner.is_none() || literal)
}

pub(super) fn deletion_witnesses(
    expected: &[CellRow],
    actual: &[(&str, bool)],
) -> Option<DeletionWitnesses> {
    let deleted = expected.len().checked_sub(actual.len())?;
    if !(1..=MAX_DELETIONS).contains(&deleted)
        || (expected.len() + 1).saturating_mul(MAX_DELETIONS + 1) > MAX_STATES
    {
        return None;
    }
    let alignment = Alignment {
        expected,
        actual,
        deleted,
    };
    let reachable = alignment.suffixes();
    if !reachable[0][0] {
        return None;
    }
    let mut forward = vec![[None; MAX_DELETIONS + 1]; expected.len() + 1];
    forward[0][0] = Some(MissingRows::default());
    let mut owners = BTreeMap::<_, Range<usize>>::new();
    for (at, row) in expected.iter().enumerate() {
        forward[at + 1] = alignment.step(at, &forward[at], row.owner.is_some());
        if let Some(owner) = row.owner {
            owners.entry(owner).or_insert(at..at + 1).end = at + 1;
        }
    }
    // An owner occupies one contiguous source interval, potentially
    // containing blank layout slots. The total work is linear in the
    // occupied intervals, even when many displays spell the same word.
    let displays = owners
        .into_iter()
        .filter_map(|(owner, rows)| {
            let mut band = forward[rows.start].map(|entry| entry.map(|_| MissingRows::default()));
            for at in rows.clone() {
                band = alignment.step(at, &band, expected[at].owner == Some(owner));
            }
            let mut range = None;
            for (state, cost) in band.into_iter().enumerate() {
                if let Some(cost) = cost
                    && reachable[rows.end][state]
                {
                    MissingRows::include(&mut range, cost);
                }
            }
            range
                .filter(|range| range.max != 0)
                .map(|range| (owner, range))
        })
        .collect();
    Some(DeletionWitnesses {
        total: forward[expected.len()][deleted]?,
        displays,
    })
}

struct Alignment<'a> {
    expected: &'a [CellRow],
    actual: &'a [(&'a str, bool)],
    deleted: usize,
}

impl Alignment<'_> {
    // After consuming `at` source rows and dropping `lost`, the
    // only possible observed position is `at - lost`.
    fn matches(&self, at: usize, lost: usize) -> bool {
        at.checked_sub(lost)
            .and_then(|pos| self.actual.get(pos))
            .is_some_and(|&row| same_slot(&self.expected[at], row))
    }

    fn step(&self, at: usize, previous: &Band, counted: bool) -> Band {
        let mut next = [None; MAX_DELETIONS + 1];
        for (lost, cost) in previous.iter().take(self.deleted + 1).enumerate() {
            if let Some(cost) = *cost {
                if self.matches(at, lost) {
                    MissingRows::include(&mut next[lost], cost);
                }
                if lost < self.deleted {
                    MissingRows::include(&mut next[lost + 1], cost.increment(counted));
                }
            }
        }
        next
    }

    fn suffixes(&self) -> Vec<[bool; MAX_DELETIONS + 1]> {
        let end = self.expected.len();
        let mut reachable = vec![[false; MAX_DELETIONS + 1]; end + 1];
        reachable[end][self.deleted] = true;
        for at in (0..end).rev() {
            for lost in 0..=at.min(self.deleted) {
                reachable[at][lost] = self.matches(at, lost) && reachable[at + 1][lost]
                    || lost < self.deleted && reachable[at + 1][lost + 1];
            }
        }
        reachable
    }
}
