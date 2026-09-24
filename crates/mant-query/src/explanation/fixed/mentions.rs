//! Literal evidence from the same bounded native units used by visible search.
//! Only unit ordinals and text ranges survive planning. The selected page
//! reconstructs display slices after class ordering and body reservations.

use std::{collections::BTreeMap, num::NonZeroU32};

use mant_ir::{DisplayRole, FixedBody, RegionKind};
use mant_protocol::EvidenceClass;

use super::{FixedCandidate, FixedHit, IndexedOwner};
use crate::search::{
    SearchError,
    fixed_visible::{FixedVisibleUnits, MAX_FIXED_SCAN_BYTES, SelectionKind},
};

pub(super) fn collect(
    fixed: &FixedBody,
    indexed: &BTreeMap<NonZeroU32, IndexedOwner>,
    direct: &BTreeMap<NonZeroU32, FixedCandidate>,
    requested: &str,
) -> Result<(Vec<FixedCandidate>, bool), super::ExplanationError> {
    let ancestry = OwnerAncestry::new(fixed, indexed)?;
    let units = match FixedVisibleUnits::new(fixed) {
        Ok(units) => units,
        Err(SearchError::ResourceLimit) => return Ok((Vec::new(), true)),
        Err(_) => return Err(super::ExplanationError::InvalidFixed),
    };
    let mut entry = BTreeMap::<NonZeroU32, FixedCandidate>::new();
    let mut weak_count = 0usize;
    let mut ordinary = Vec::new();
    let mut scanned = 0usize;
    let mut truncated = false;
    for (ordinal, unit) in units.iter().enumerate() {
        let Some((owner, section)) = ownership(&ancestry, unit.pieces) else {
            continue;
        };
        let materialized = match unit.materialize(fixed) {
            Ok(value) => value,
            Err(SearchError::ResourceLimit) => {
                truncated = true;
                break;
            }
            Err(_) => return Err(super::ExplanationError::InvalidFixed),
        };
        scanned = scanned.saturating_add(materialized.text.len());
        if scanned > MAX_FIXED_SCAN_BYTES {
            truncated = true;
            break;
        }
        let Some(range) = super::super::literal::first_match(&materialized.text, requested) else {
            continue;
        };
        let hit = FixedHit {
            unit: ordinal,
            range,
        };
        if let Some(key) = owner {
            if !entry.contains_key(&key) {
                let matched_direct = matched_direct_owner(indexed, key);
                if matched_direct && !direct.contains_key(&key) {
                    truncated = true;
                    continue;
                }
                if !matched_direct {
                    if weak_count == mant_protocol::MAX_EXPLANATION_CANDIDATES {
                        truncated = true;
                        continue;
                    }
                    weak_count += 1;
                }
            }
            let record = entry.entry(key).or_insert_with(|| FixedCandidate {
                key: Some(key),
                section,
                class: EvidenceClass::EntryMention,
                hits: Vec::new(),
            });
            if record.hits.len() < 2 {
                record.hits.push(hit);
            }
        } else if ordinary.len() == mant_protocol::MAX_EXPLANATION_CANDIDATES {
            truncated = true;
        } else {
            ordinary.push(FixedCandidate {
                key: None,
                section,
                class: EvidenceClass::ContextMention,
                hits: vec![hit],
            });
        }
    }
    let mut candidates = entry.into_values().collect::<Vec<_>>();
    candidates.extend(ordinary);
    Ok((candidates, truncated))
}

fn matched_direct_owner(indexed: &BTreeMap<NonZeroU32, IndexedOwner>, key: NonZeroU32) -> bool {
    indexed.get(&key).is_some_and(|selected| {
        let (name, form, id, path) = selected.matched;
        name || form || id || path
    })
}

/// Accept only text that a native body/region owns. A heading/term or drawn
/// rule is not prose, even if its final glyphs happen to spell the query.
fn ownership(
    ancestry: &OwnerAncestry,
    pieces: &[crate::search::fixed_visible::units::Piece],
) -> Option<(Option<NonZeroU32>, Option<NonZeroU32>)> {
    let first = pieces.first()?;
    let section = first.section;
    let owner = ancestry.semantic_owner(first.owner, first.selection_owner)?;
    for piece in pieces {
        let body_kind = matches!(
            piece.kind,
            SelectionKind::OwnerBody
                | SelectionKind::HeadingBody
                | SelectionKind::Region(
                    RegionKind::Unsectioned
                        | RegionKind::HeadingBody
                        | RegionKind::OwnerBody
                        | RegionKind::List
                        | RegionKind::Literal
                        | RegionKind::TableCell
                        | RegionKind::Equation
                )
        );
        if !body_kind || piece.display_role != DisplayRole::Body || piece.section != section {
            return None;
        }
        if ancestry.semantic_owner(piece.owner, piece.selection_owner)? != owner {
            return None;
        }
    }
    Some((
        match owner {
            OwnerResolution::Semantic(key) => Some(key),
            OwnerResolution::Ordinary => None,
        },
        section,
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnerResolution {
    Semantic(NonZeroU32),
    Ordinary,
}

/// Precompute nearest checked semantic owners and ancestry intervals once.
/// Deep native nesting must not multiply every visible run's attribution work.
struct OwnerAncestry {
    nearest: Vec<Option<NonZeroU32>>,
    enter: Vec<usize>,
    leave: Vec<usize>,
}

impl OwnerAncestry {
    fn new(
        fixed: &FixedBody,
        indexed: &BTreeMap<NonZeroU32, IndexedOwner>,
    ) -> Result<Self, super::ExplanationError> {
        let count = fixed.owners.len();
        let mut nearest = vec![None; count + 1];
        let mut children = vec![Vec::new(); count + 1];
        for owner in &fixed.owners {
            let key = owner.key.get() as usize;
            let parent = owner.parent.map_or(0, |value| value.get() as usize);
            if key > count || parent >= key {
                return Err(super::ExplanationError::InvalidFixed);
            }
            children[parent].push(key);
            nearest[key] = if indexed.contains_key(&owner.key) {
                Some(owner.key)
            } else {
                nearest[parent]
            };
        }
        let mut enter = vec![0; count + 1];
        let mut leave = vec![0; count + 1];
        let mut tick = 0usize;
        let mut stack = vec![(0usize, false)];
        while let Some((node, exiting)) = stack.pop() {
            if exiting {
                leave[node] = tick;
            } else {
                enter[node] = tick;
                tick += 1;
                stack.push((node, true));
                stack.extend(children[node].iter().rev().map(|&child| (child, false)));
            }
        }
        Ok(Self {
            nearest,
            enter,
            leave,
        })
    }

    /// The final run label and the containing selection remain distinct.
    /// Their relation and the closest semantic ancestor are O(1) lookups.
    fn semantic_owner(
        &self,
        run: Option<NonZeroU32>,
        selection: Option<NonZeroU32>,
    ) -> Option<OwnerResolution> {
        if let (Some(run), Some(selection)) = (run, selection)
            && run != selection
        {
            let run = run.get() as usize;
            let selection = selection.get() as usize;
            if !(self.enter.get(selection)? <= self.enter.get(run)?
                && self.enter[run] < *self.leave.get(selection)?)
            {
                return None;
            }
        }
        match self
            .nearest
            .get(run.or(selection).map_or(0, |key| key.get() as usize))?
        {
            Some(key) => Some(OwnerResolution::Semantic(*key)),
            None => Some(OwnerResolution::Ordinary),
        }
    }
}
