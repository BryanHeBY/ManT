//! Assign distinct reading ranges to independently observed display occurrences.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    LiteralOrigin, ObservedBlock, ObservedDisplays, Point, ScopeKind, ScopeOccurrence,
    SourceDisplay, SourceDisplays, has_glyph,
};

#[path = "cell_alignment.rs"]
mod cell_alignment;

use cell_alignment::{DeletionWitnesses, MissingRows, deletion_witnesses, same_slot};

pub(crate) fn compare(
    expected: &SourceDisplays,
    observed: &ObservedDisplays,
    violations: &mut Vec<String>,
) {
    let observations = observed.matching_blocks(&expected.scopes);
    let mut witnesses = RowWitnesses::collect(&observations);
    let mut matched = vec![0; expected.owners.len()];
    let mut claimed_blocks = BTreeSet::new();
    let uncertain = claim_cells(expected, &observations, &mut matched, violations);
    let exact = row_witness_obligations(&expected.owners);
    // Resolve literal rows before claiming unsplit blocks. Two displays
    // in one column can claim different rows of the same block; no source
    // occurrence may claim a range already used by another occurrence.
    for (index, text) in &expected.source_rows {
        if exact[*index]
            && !in_column_cell(&expected.owners[*index])
            && let Some(block) = witnesses.claim(&expected.owners[*index], text)
        {
            matched[*index] += 1;
            claimed_blocks.insert(block);
        }
    }
    let mut blocks = BTreeMap::<_, Blocks>::new();
    for (index, block) in observations.iter().enumerate() {
        if block.literal
            && let Some(point) = block.point
            && !claimed_blocks.contains(&index)
        {
            let origin = LiteralOrigin {
                path: block.path.clone(),
                point,
            };
            blocks.entry(origin).or_default().available += 1;
        }
    }
    let cells = &expected.cells;
    let expected = &expected.owners;
    let mut claims = vec![None; expected.len()];
    let mut occurrences = BTreeMap::<Point, usize>::new();
    for (index, owner) in expected.iter().enumerate() {
        let occurrence = occurrences.entry(owner.opening).or_default();
        *occurrence += 1;
        let covered = if cells
            .get(&owner.path)
            .is_some_and(|plan| !plan.verified.contains(&index))
        {
            matched[index] > 0
        } else if exact[index] && owner.rows > 0 {
            matched[index] == owner.rows
        } else {
            match_block(index, expected, &mut blocks, &mut claims)
        };
        if !covered && !uncertain.contains(&index) {
            let (line, column) = owner.opening;
            let missing = owner.rows.saturating_sub(matched[index]);
            let detail = if in_column_cell(owner) && missing > 1 {
                format!(" ({missing} missing rows)")
            } else {
                String::new()
            };
            violations.push(format!(
                "literal-displays: missing sourced block for {line}:{column} occurrence {occurrence}{detail}"
            ));
        }
    }
}

fn claim_cells(
    expected: &SourceDisplays,
    observations: &[ObservedBlock<'_>],
    matched: &mut [usize],
    violations: &mut Vec<String>,
) -> BTreeSet<usize> {
    // Each native It BODY is an independent cell. Ordinary text on either
    // side can share the IR Preformatted block and even the exact spelling
    // of the display. Reserve those rows before admitting any display ink.
    let mut cells = BTreeMap::<_, Vec<_>>::new();
    for block in observations {
        if expected.cells.contains_key(&block.path) {
            cells
                .entry(&block.path)
                .or_default()
                .extend(block.rows.iter().map(|text| (text.as_str(), block.literal)));
        }
    }
    let mut uncertain = BTreeSet::new();
    for (path, plan) in &expected.cells {
        let physical = cells.get(path).map_or(&[][..], Vec::as_slice);
        if !plan.complete {
            claim_known_prefix(plan, physical, matched);
        } else if physical.len() == plan.rows.len() {
            // A replacement leaves the slot in place. A damaged ordinary
            // row does not invalidate an unchanged display after it.
            for (row, &observed) in plan.rows.iter().zip(physical) {
                if let Some(owner) = row.owner
                    && same_slot(row, observed)
                {
                    matched[owner] += 1;
                }
            }
        } else if let Some(witnesses) = deletion_witnesses(&plan.rows, physical) {
            claim_after_row_deletions(
                plan,
                witnesses,
                expected,
                matched,
                &mut uncertain,
                violations,
            );
        } else {
            review_unproved_alignment(
                plan,
                physical,
                expected,
                matched,
                &mut uncertain,
                violations,
            );
        }
    }
    // An unknown range is reported as source Review. An empty cell or a
    // projection to ordinary paragraphs still proves the literal is absent.
    for (i, owner) in expected.owners.iter().enumerate() {
        if expected
            .cells
            .get(&owner.path)
            .is_some_and(|plan| !plan.verified.contains(&i))
            && cells
                .get(&owner.path)
                .is_some_and(|rows| rows.iter().any(|(_, literal)| *literal))
        {
            matched[i] = 1;
        }
    }
    uncertain
}

fn claim_known_prefix(
    plan: &super::rows::CellPlan,
    physical: &[(&str, bool)],
    matched: &mut [usize],
) {
    // Later equal ink cannot pay for a row after an unclassified boundary.
    for (want, &observed) in plan
        .rows
        .iter()
        .filter(|row| has_glyph(&row.text))
        .zip(physical.iter().filter(|(text, _)| has_glyph(text)))
    {
        if want.owner.is_none() && want.text != observed.0 {
            break;
        }
        if let Some(owner) = want.owner
            && same_slot(want, observed)
        {
            matched[owner] += 1;
        }
    }
}

fn claim_after_row_deletions(
    plan: &super::rows::CellPlan,
    witnesses: DeletionWitnesses,
    expected: &SourceDisplays,
    matched: &mut [usize],
    uncertain: &mut BTreeSet<usize>,
    violations: &mut Vec<String>,
) {
    for owner in plan.rows.iter().filter_map(|row| row.owner) {
        matched[owner] += 1;
    }
    let mut exact = 0;
    let mut candidates = Vec::new();
    for (owner, count) in witnesses.displays {
        if count.min == count.max {
            matched[owner] -= count.min;
            exact += count.min;
        } else {
            uncertain.insert(owner);
            candidates.push(owner);
        }
    }
    if !candidates.is_empty() {
        let count = MissingRows {
            min: witnesses.total.min - exact,
            max: witnesses.total.max - exact,
        };
        let count = if count.min == count.max {
            count.min.to_string()
        } else {
            format!("{}..={}", count.min, count.max)
        };
        let locations = cell_locations(candidates, &expected.owners);
        violations.push(format!(
            "literal-displays: {count} missing row(s) among column display candidates {locations}"
        ));
    }
}

fn review_unproved_alignment(
    plan: &super::rows::CellPlan,
    physical: &[(&str, bool)],
    expected: &SourceDisplays,
    matched: &mut [usize],
    uncertain: &mut BTreeSet<usize>,
    violations: &mut Vec<String>,
) {
    let prefix = plan
        .rows
        .iter()
        .zip(physical)
        .take_while(|(row, actual)| same_slot(row, **actual))
        .count();
    let suffix = plan.rows[prefix..]
        .iter()
        .rev()
        .zip(physical[prefix..].iter().rev())
        .take_while(|(row, actual)| same_slot(row, **actual))
        .count();
    let end = plan.rows.len() - suffix;
    for owner in plan.rows[..prefix]
        .iter()
        .chain(&plan.rows[end..])
        .filter_map(|row| row.owner)
    {
        matched[owner] += 1;
    }
    if physical[prefix..physical.len() - suffix]
        .iter()
        .all(|(_, literal)| !literal)
    {
        // An empty observation or a nonliteral replacement cannot
        // witness any of the unmatched displays.
        return;
    }
    let candidates = plan.rows[prefix..end]
        .iter()
        .filter_map(|row| row.owner)
        .collect::<BTreeSet<_>>();
    if !candidates.is_empty() {
        uncertain.extend(&candidates);
        let locations = cell_locations(candidates, &expected.owners);
        violations.push(format!(
            "literal-displays: unresolved column slot alignment for candidates {locations}"
        ));
    }
}

fn cell_locations(candidates: impl IntoIterator<Item = usize>, owners: &[SourceDisplay]) -> String {
    candidates
        .into_iter()
        .map(|id| {
            let (line, column) = owners[id].opening;
            format!("{line}:{column}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn row_witness_obligations(owners: &[SourceDisplay]) -> Vec<bool> {
    let mut exact = vec![false; owners.len()];
    let mut first = BTreeMap::new();
    for (index, owner) in owners.iter().enumerate() {
        exact[index] |= in_column_cell(owner) || owner.short;
        for origin in &owner.origins {
            if let Some(previous) = first.insert(origin, index) {
                // At a shared coordinate the emitted fragments of an
                // earlier display are no longer unique owner evidence.
                exact[index] = true;
                exact[previous] = true;
            }
        }
    }
    exact
}

fn in_column_cell(owner: &SourceDisplay) -> bool {
    owner
        .path
        .last()
        .is_some_and(|parent| matches!(parent.scope.kind, ScopeKind::ColumnCell { .. }))
}

type RowPosition = (usize, usize);

struct RowWitnesses<'a> {
    by_text: BTreeMap<(&'a [ScopeOccurrence], &'a str), Vec<RowPosition>>,
    after: BTreeMap<Vec<ScopeOccurrence>, RowPosition>,
    observations: &'a [ObservedBlock<'a>],
}

impl<'a> RowWitnesses<'a> {
    fn collect(observations: &'a [ObservedBlock<'a>]) -> Self {
        let mut by_text = BTreeMap::<_, Vec<_>>::new();
        for (block_index, block) in observations
            .iter()
            .enumerate()
            .filter(|(_, block)| block.literal)
        {
            for (row_index, row) in block.rows.iter().enumerate() {
                if has_glyph(row) {
                    by_text
                        .entry((block.path.as_slice(), row.as_str()))
                        .or_default()
                        .push((block_index, row_index));
                }
            }
        }
        Self {
            by_text,
            after: BTreeMap::new(),
            observations,
        }
    }

    fn claim(&mut self, owner: &SourceDisplay, row: &str) -> Option<usize> {
        let cell = in_column_cell(owner);
        let after = self.after.get(&owner.path).copied();
        let candidates = self.by_text.get(&(owner.path.as_slice(), row))?;
        let start =
            candidates.partition_point(|position| after.is_some_and(|after| *position <= after));
        let position = candidates[start..]
            .iter()
            .copied()
            .find(|&(block_index, _)| {
                let block = &self.observations[block_index];
                cell || block.point.is_some_and(|point| {
                    owner.origins.contains(&LiteralOrigin {
                        path: owner.path.clone(),
                        point,
                    })
                })
            })?;
        self.after.insert(owner.path.clone(), position);
        Some(position.0)
    }
}

#[derive(Default)]
struct Blocks {
    available: usize,
    owners: BTreeSet<usize>,
}

fn match_block(
    owner: usize,
    expected: &[SourceDisplay],
    blocks: &mut BTreeMap<LiteralOrigin, Blocks>,
    claims: &mut [Option<LiteralOrigin>],
) -> bool {
    // Equal positions occur in user macro expansions. Each block can pay
    // once. If more than one point fits an owner, look for an alternate
    // assignment without allowing a neighboring display to pay twice.
    let mut pending = vec![owner];
    let mut paths = BTreeMap::new();
    while let Some(owner) = pending.pop() {
        for origin in &expected[owner].origins {
            let Some(block) = blocks.get(origin) else {
                continue;
            };
            let std::collections::btree_map::Entry::Vacant(path) = paths.entry(origin.clone())
            else {
                continue;
            };
            path.insert(owner);
            if block.available == 0 {
                pending.extend(block.owners.iter().copied());
                continue;
            }
            let mut open = Some(origin.clone());
            while let Some(origin) = open {
                let owner = paths[&origin];
                let block = blocks.get_mut(&origin).expect("matched literal origin");
                block.available -= 1;
                block.owners.insert(owner);
                open = claims[owner].replace(origin);
                if let Some(previous) = &open {
                    let old = blocks.get_mut(previous).expect("prior literal origin");
                    old.available += 1;
                    old.owners.remove(&owner);
                }
            }
            return true;
        }
    }
    false
}
