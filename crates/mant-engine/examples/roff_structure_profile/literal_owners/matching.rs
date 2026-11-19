//! Assign distinct reading ranges to independently observed display occurrences.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    LiteralOrigin, ObservedBlock, ObservedDisplays, Point, ScopeKind, ScopeOccurrence,
    SourceDisplay, SourceDisplays, has_glyph,
};

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
            violations.push(format!(
                "literal-displays: missing sourced block for {line}:{column} occurrence {occurrence}"
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
        if !plan.complete || physical.len() == plan.rows.len() {
            // When later source is unproved, only the exact initial rows
            // can be claimed. Later equal ink cannot replace that prefix.
            let observed = physical
                .iter()
                .filter(|(text, _)| plan.complete || has_glyph(text));
            let expected_rows = plan
                .rows
                .iter()
                .filter(|row| plan.complete || has_glyph(&row.text));
            for (want, &(text, literal)) in expected_rows.zip(observed) {
                if want.owner.is_none() && text != want.text {
                    break;
                }
                if literal
                    && text == want.text
                    && let Some(id) = want.owner
                {
                    matched[id] += 1;
                }
            }
            continue;
        }
        let rows = physical
            .iter()
            .copied()
            .filter(|(text, _)| has_glyph(text))
            .collect::<Vec<_>>();
        let mut positions = BTreeMap::<_, Vec<_>>::new();
        for (i, &(text, _)) in rows.iter().enumerate() {
            positions.entry(text).or_default().push(i);
        }
        let mut bounds = vec![rows.len(); plan.rows.len()];
        let mut end = rows.len();
        for (i, want) in plan.rows.iter().enumerate().rev() {
            bounds[i] = end;
            if want.owner.is_none()
                && let Some(xs) = positions.get(want.text.as_str())
            {
                let stop = xs.partition_point(|&x| x < end);
                if let Some(last) = xs[..stop].last() {
                    end = *last;
                }
            }
        }
        let mut start = 0;
        for (want, bound) in plan.rows.iter().zip(bounds) {
            let Some(xs) = positions.get(want.text.as_str()) else {
                continue;
            };
            let from = xs.partition_point(|&x| x < start);
            if let Some(&i) = xs[from..]
                .iter()
                .take_while(|&&x| x < bound)
                .find(|&&x| want.owner.is_none() || rows[x].1)
            {
                start = i + 1;
                if let Some(id) = want.owner {
                    matched[id] += 1;
                }
            }
        }
        report_ambiguous_rows(plan, &expected.owners, matched, &mut uncertain, violations);
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

fn report_ambiguous_rows(
    plan: &super::rows::CellPlan,
    owners: &[SourceDisplay],
    matched: &[usize],
    uncertain: &mut BTreeSet<usize>,
    violations: &mut Vec<String>,
) {
    // A missing physical slot destroys correspondence between equal
    // display rows. Report the count and candidate intervals; the
    // order of equal strings cannot identify a damaged owner.
    let mut by_text = BTreeMap::<_, Vec<_>>::new();
    for row in &plan.rows {
        if let Some(id) = row.owner.filter(|&id| owners[id].rows == 1) {
            by_text.entry(row.text.as_str()).or_default().push(id);
        }
    }
    for candidates in by_text.values() {
        let missing = candidates.iter().filter(|&&id| matched[id] == 0).count();
        if missing == 0 || missing == candidates.len() {
            continue;
        }
        uncertain.extend(candidates);
        let locations = candidates
            .iter()
            .map(|&id| {
                let (line, column) = owners[id].opening;
                format!("{line}:{column}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        violations.push(format!(
            "literal-displays: {missing} missing row(s) among column display candidates {locations}"
        ));
    }
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
