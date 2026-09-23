//! Claim each final run byte once, favoring the most specific native owner.

use super::{
    DisplayRole, FixedBody, MAX_FIXED_SEARCH_BYTES, NonZeroU32, OutputSlice, Range, SearchError,
    SourceSpan, TextJoin, TextSelection,
};

#[derive(Clone, Copy)]
struct Candidate<'a> {
    selection: &'a TextSelection,
    section: Option<NonZeroU32>,
    source: Option<SourceSpan>,
    priority: u8,
}

#[derive(Clone)]
pub(super) struct Piece {
    pub(super) slice: OutputSlice,
    pub(super) group: usize,
    pub(super) join_before: Option<TextJoin>,
    pub(super) section: Option<NonZeroU32>,
    pub(super) source: Option<SourceSpan>,
}

#[allow(clippy::too_many_lines)] // One pass assigns each final byte to its most specific surviving owner.
pub(super) fn pieces(fixed: &FixedBody) -> Result<Vec<Piece>, SearchError> {
    let mut candidates = Vec::new();
    for owner in &fixed.owners {
        candidates.push(Candidate {
            selection: &owner.head,
            section: owner.section,
            source: owner.source,
            priority: 0,
        });
        candidates.push(Candidate {
            selection: &owner.direct_body,
            section: owner.section,
            source: owner.source,
            priority: 0,
        });
    }
    for heading in &fixed.headings {
        candidates.push(Candidate {
            selection: &heading.title,
            section: Some(heading.key),
            source: heading.source,
            priority: 1,
        });
        candidates.push(Candidate {
            selection: &heading.direct_body,
            section: Some(heading.key),
            source: heading.source,
            priority: 1,
        });
    }
    for region in &fixed.regions {
        candidates.push(Candidate {
            selection: &region.selection,
            section: region
                .owner
                .and_then(|owner| fixed.owners.get((owner.get() - 1) as usize)?.section),
            source: region.source,
            priority: 2,
        });
    }
    candidates.sort_by_key(|candidate| candidate.priority);
    let mut occupied: Vec<Vec<Range<u64>>> = vec![Vec::new(); fixed.surface.runs.len()];
    let mut output = Vec::new();
    let mut work = 0usize;
    for (group, candidate) in candidates.iter().enumerate() {
        let mut prior_full_part = None;
        for (index, part) in candidate.selection.parts.iter().enumerate() {
            if fixed.surface.runs[(part.run.get() - 1) as usize].label.role == DisplayRole::Layout {
                prior_full_part = None;
                continue;
            }
            work = work.checked_add(1).ok_or(SearchError::ResourceLimit)?;
            if work > MAX_FIXED_SEARCH_BYTES {
                return Err(SearchError::ResourceLimit);
            }
            let claims = occupied
                .get_mut((part.run.get() - 1) as usize)
                .ok_or(SearchError::ContentProjection)?;
            let free = unclaimed(part.start_byte..part.end_byte, claims, &mut work)?;
            let full = free.len() == 1 && free[0] == (part.start_byte..part.end_byte);
            for (piece_index, range) in free.into_iter().enumerate() {
                let join_before =
                    if piece_index == 0 && full && prior_full_part == index.checked_sub(1) {
                        index
                            .checked_sub(1)
                            .map(|prior| candidate.selection.joins[prior].clone())
                    } else {
                        None
                    };
                output.push(Piece {
                    slice: OutputSlice {
                        run: part.run,
                        start_byte: range.start,
                        end_byte: range.end,
                    },
                    group,
                    join_before,
                    section: candidate.section,
                    source: candidate.source,
                });
                claims.push(range);
            }
            claims.sort_unstable_by_key(|range| range.start);
            prior_full_part = full.then_some(index);
        }
    }
    for run in &fixed.surface.runs {
        if run.label.role == DisplayRole::Layout {
            continue;
        }
        let claims = &occupied[(run.key.get() - 1) as usize];
        for range in unclaimed(0..run.byte_count, claims, &mut work)? {
            let group = candidates
                .len()
                .checked_add(run.key.get() as usize)
                .ok_or(SearchError::ResourceLimit)?;
            output.push(Piece {
                slice: OutputSlice {
                    run: run.key,
                    start_byte: range.start,
                    end_byte: range.end,
                },
                group,
                join_before: None,
                section: None,
                source: None,
            });
        }
    }
    output.sort_unstable_by_key(|piece| (piece.slice.run, piece.slice.start_byte));
    Ok(output)
}

fn unclaimed(
    part: Range<u64>,
    claims: &[Range<u64>],
    work: &mut usize,
) -> Result<Vec<Range<u64>>, SearchError> {
    let mut cursor = part.start;
    let mut free = Vec::new();
    for claim in claims {
        *work = work.checked_add(1).ok_or(SearchError::ResourceLimit)?;
        if *work > MAX_FIXED_SEARCH_BYTES {
            return Err(SearchError::ResourceLimit);
        }
        if claim.end <= cursor {
            continue;
        }
        if claim.start >= part.end {
            break;
        }
        if cursor < claim.start {
            free.push(cursor..claim.start.min(part.end));
        }
        cursor = cursor.max(claim.end);
        if cursor >= part.end {
            break;
        }
    }
    if cursor < part.end {
        free.push(cursor..part.end);
    }
    Ok(free)
}
