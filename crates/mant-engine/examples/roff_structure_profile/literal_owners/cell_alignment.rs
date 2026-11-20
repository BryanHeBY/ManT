//! Bounded edit witnesses for column-cell row sequences.

use super::super::rows::CellRow;
use std::{collections::BTreeMap, ops::Range};

const MAX_EDITS: usize = 8;
const BAND_SIZE: usize = MAX_EDITS * 2 + 1;
const MAX_STATES: usize = 65_536;
const UNREACHABLE: u8 = 9;
type Costs = [u8; BAND_SIZE];
type WitnessBand = [Option<MissingRows>; BAND_SIZE];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

    fn with_loss(self, counted: bool) -> Self {
        let count = usize::from(counted);
        Self {
            min: self.min + count,
            max: self.max + count,
        }
    }
}

pub(super) struct EditWitnesses {
    pub(super) total: MissingRows,
    pub(super) displays: BTreeMap<usize, MissingRows>,
}

pub(super) fn same_slot(row: &CellRow, (text, literal): (&str, bool)) -> bool {
    row.text == text && (row.owner.is_none() || literal)
}

pub(super) fn edit_witnesses(
    expected: &[CellRow],
    actual: &[(&str, bool)],
) -> Option<EditWitnesses> {
    // These linear paths have exact proofs regardless of the cell length.
    if expected.len() == actual.len() + 1
        && let Some(witnesses) = single_deletion(expected, actual)
    {
        return Some(witnesses);
    }
    if expected.len().abs_diff(actual.len()) > MAX_EDITS
        || (expected.len() + 1).saturating_mul(BAND_SIZE) > MAX_STATES
    {
        return None;
    }
    let alignment = Alignment::new(expected, actual);
    if alignment.cost(0, 0) >= UNREACHABLE {
        return None;
    }
    let mut forward = vec![[None; BAND_SIZE]; expected.len() + 1];
    forward[0][MAX_EDITS] = Some(MissingRows::default());
    let mut owners = BTreeMap::<_, Range<usize>>::new();
    for (at, row) in expected.iter().enumerate() {
        forward[at + 1] = alignment.step(at, forward[at], row.owner.is_some());
        if let Some(owner) = row.owner {
            owners.entry(owner).or_insert(at..at + 1).end = at + 1;
        }
    }
    let displays = owners
        .into_iter()
        .filter_map(|(owner, rows)| {
            let mut band = forward[rows.start].map(|entry| entry.map(|_| MissingRows::default()));
            for at in rows {
                band = alignment.step(at, band, expected[at].owner == Some(owner));
            }
            // All retained states can still reach the end on an optimal
            // path. Each display occupies a disjoint source interval.
            let range = bounds(&band)?;
            (range.max > 0).then_some((owner, range))
        })
        .collect();
    Some(EditWitnesses {
        total: bounds(&forward[expected.len()])?,
        displays,
    })
}

fn bounds(band: &WitnessBand) -> Option<MissingRows> {
    let mut range = None;
    for &cost in band.iter().flatten() {
        MissingRows::include(&mut range, cost);
    }
    range
}

fn single_deletion(expected: &[CellRow], actual: &[(&str, bool)]) -> Option<EditWitnesses> {
    let prefix = expected
        .iter()
        .zip(actual)
        .take_while(|(row, observed)| same_slot(row, **observed))
        .count();
    let suffix = expected
        .iter()
        .rev()
        .zip(actual.iter().rev())
        .take_while(|(row, observed)| same_slot(row, **observed))
        .count();
    let first = actual.len() - suffix;
    if first > prefix {
        return None;
    }
    let candidates = &expected[first..=prefix];
    let ordinary = candidates.iter().any(|row| row.owner.is_none());
    let mut displays: BTreeMap<_, _> = candidates
        .iter()
        .filter_map(|row| row.owner)
        .map(|owner| (owner, MissingRows { min: 0, max: 1 }))
        .collect();
    if !ordinary && displays.len() == 1 {
        displays.first_entry()?.get_mut().min = 1;
    }
    Some(EditWitnesses {
        total: MissingRows {
            min: usize::from(!ordinary),
            max: usize::from(!displays.is_empty()),
        },
        displays,
    })
}

struct Alignment<'a> {
    expected: &'a [CellRow],
    actual: &'a [(&'a str, bool)],
    suffixes: Vec<Costs>,
}

impl<'a> Alignment<'a> {
    fn new(expected: &'a [CellRow], actual: &'a [(&'a str, bool)]) -> Self {
        let mut result = Self {
            expected,
            actual,
            suffixes: vec![[UNREACHABLE; BAND_SIZE]; expected.len() + 1],
        };
        let (end, last) = (expected.len(), actual.len());
        for at in (0..=end).rev() {
            for j in (at.saturating_sub(MAX_EDITS)..=last.min(at + MAX_EDITS)).rev() {
                let cost = if (at, j) == (end, last) {
                    0
                } else {
                    let remove = result.cost(at + 1, j).saturating_add(1);
                    let insert = result.cost(at, j + 1).saturating_add(1);
                    let keep = if result.matches(at, j) {
                        result.cost(at + 1, j + 1)
                    } else {
                        UNREACHABLE
                    };
                    remove.min(insert).min(keep).min(UNREACHABLE)
                };
                result.suffixes[at][j + MAX_EDITS - at] = cost;
            }
        }
        result
    }

    fn cost(&self, at: usize, j: usize) -> u8 {
        (j <= self.actual.len())
            .then(|| (j + MAX_EDITS).checked_sub(at))
            .flatten()
            .and_then(|slot| self.suffixes.get(at)?.get(slot))
            .copied()
            .unwrap_or(UNREACHABLE)
    }

    fn matches(&self, at: usize, j: usize) -> bool {
        self.expected
            .get(at)
            .zip(self.actual.get(j))
            .is_some_and(|(row, &actual)| same_slot(row, actual))
    }

    fn step(&self, at: usize, mut previous: WitnessBand, counted: bool) -> WitnessBand {
        let mut next = [None; BAND_SIZE];
        for slot in 0..BAND_SIZE {
            let Some(witness) = previous[slot] else {
                continue;
            };
            let j = at + slot - MAX_EDITS;
            let distance = self.cost(at, j);
            if slot + 1 < BAND_SIZE && self.cost(at, j + 1) + 1 == distance {
                MissingRows::include(&mut previous[slot + 1], witness);
            }
            if slot > 0 && self.cost(at + 1, j) + 1 == distance {
                MissingRows::include(&mut next[slot - 1], witness.with_loss(counted));
            }
            if self.matches(at, j) && self.cost(at + 1, j + 1) == distance {
                MissingRows::include(&mut next[slot], witness);
            }
        }
        next
    }
}

#[cfg(test)]
#[path = "cell_alignment_tests.rs"]
mod tests;
