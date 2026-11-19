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
    let mut witnesses = RowWitnesses::collect(&observations, &expected.cell_prefixes);
    let mut matched = vec![0; expected.owners.len()];
    let mut claimed_blocks = BTreeSet::new();
    let exact = row_witness_obligations(&expected.owners);
    // Resolve literal rows before claiming unsplit blocks. Two displays
    // in one column can claim different rows of the same block; no source
    // occurrence may claim a range already used by another occurrence.
    for (index, text) in &expected.source_rows {
        if exact[*index]
            && let Some(block) = witnesses.claim(&expected.owners[*index], text)
        {
            matched[*index] += 1;
            claimed_blocks.insert(block);
        }
    }
    let mut blocks = BTreeMap::<_, Blocks>::new();
    for (index, block) in observations.iter().enumerate() {
        if let Some(point) = block.point
            && !claimed_blocks.contains(&index)
        {
            let origin = LiteralOrigin {
                path: block.path.clone(),
                point,
            };
            blocks.entry(origin).or_default().available += 1;
        }
    }
    let expected = &expected.owners;
    let mut claims = vec![None; expected.len()];
    let mut occurrences = BTreeMap::<Point, usize>::new();
    for (index, owner) in expected.iter().enumerate() {
        let occurrence = occurrences.entry(owner.opening).or_default();
        *occurrence += 1;
        let covered = if exact[index] && owner.rows > 0 {
            matched[index] == owner.rows
        } else {
            match_block(index, expected, &mut blocks, &mut claims)
        };
        if !covered {
            let (line, column) = owner.opening;
            violations.push(format!(
                "literal-displays: missing sourced block for {line}:{column} occurrence {occurrence}"
            ));
        }
    }
}

fn row_witness_obligations(owners: &[SourceDisplay]) -> Vec<bool> {
    let mut exact = vec![false; owners.len()];
    let mut first = BTreeMap::new();
    for (index, owner) in owners.iter().enumerate() {
        exact[index] |= in_column_cell(owner);
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
    fn collect(
        observations: &'a [ObservedBlock<'a>],
        prefixes: &BTreeMap<Vec<ScopeOccurrence>, (Point, String)>,
    ) -> Self {
        let mut by_text = BTreeMap::<_, Vec<_>>::new();
        let mut considered_paths = BTreeSet::new();
        for (block_index, block) in observations.iter().enumerate() {
            // Column lowering can retain the text before the first display
            // in its preformatted block. Its first row is not display ink.
            let skip_initial_row = considered_paths.insert(block.path.as_slice())
                && prefixes.get(&block.path).is_some_and(|(point, prefix)| {
                    block.point.is_none_or(|start| start == *point)
                        && block.rows.first() == Some(prefix)
                });
            for (row_index, row) in block
                .rows
                .iter()
                .enumerate()
                .skip(usize::from(skip_initial_row))
            {
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
