//! Bounded borrowed units of final native ink, shared by visible search and
//! Fixed semantic evidence. Neither terminal adjacency nor a copied ancestor
//! reading view can establish a text join.

use std::ops::Range;

use super::{FixedBody, MAX_FIXED_SEARCH_BYTES, SearchError, TextJoin, candidates};

pub(crate) use candidates::Piece;

/// One request-local claim of each surviving run byte. Construction preserves
/// the existing search work bound; units borrow these claims, not body copies.
pub(crate) struct FixedVisibleUnits {
    pieces: Vec<Piece>,
    ranges: Vec<Range<usize>>,
}

impl FixedVisibleUnits {
    pub(crate) fn new(fixed: &FixedBody) -> Result<Self, SearchError> {
        let pieces = candidates::pieces(fixed)?;
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < pieces.len() {
            let mut end = start + 1;
            while end < pieces.len()
                && pieces[end].group == pieces[end - 1].group
                && matches!(
                    pieces[end].join_before,
                    Some(TextJoin::DirectContact | TextJoin::AuthoredSeparator(_))
                )
            {
                end += 1;
            }
            ranges.push(start..end);
            start = end;
        }
        Ok(Self { pieces, ranges })
    }

    pub(crate) fn iter(&self) -> FixedUnitIter<'_> {
        FixedUnitIter {
            pieces: &self.pieces,
            ranges: self.ranges.iter(),
        }
    }

    /// Stable final-display ordinal, used only to project a selected page.
    pub(crate) fn get(&self, ordinal: usize) -> Option<FixedVisibleUnit<'_>> {
        let range = self.ranges.get(ordinal)?;
        Some(FixedVisibleUnit {
            pieces: &self.pieces[range.clone()],
        })
    }
}

pub(crate) struct FixedUnitIter<'a> {
    pieces: &'a [Piece],
    ranges: std::slice::Iter<'a, Range<usize>>,
}

impl<'a> Iterator for FixedUnitIter<'a> {
    type Item = FixedVisibleUnit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let range = self.ranges.next()?;
        Some(FixedVisibleUnit {
            pieces: &self.pieces[range.clone()],
        })
    }
}

/// Exactly one logical unit. `Unknown`, `HardBoundary`, missing join evidence,
/// a different native selection, or partially claimed slices start a new unit.
#[derive(Clone, Copy)]
pub(crate) struct FixedVisibleUnit<'a> {
    pub(crate) pieces: &'a [Piece],
}

/// A checked correspondence between one final run slice and its range in the
/// bounded, temporary logical unit text.
pub(crate) struct FixedUnitPart<'a> {
    pub(crate) piece: &'a Piece,
    pub(crate) text_range: Range<usize>,
}

/// Request-local logical text plus exact native reverse mapping. This is
/// released after scanning the unit; it is not a second document body.
pub(crate) struct FixedUnitText<'a> {
    pub(crate) text: String,
    pub(crate) parts: Vec<FixedUnitPart<'a>>,
}

impl<'a> FixedVisibleUnit<'a> {
    pub(crate) fn materialize(self, fixed: &FixedBody) -> Result<FixedUnitText<'a>, SearchError> {
        let mut text = String::new();
        let mut parts = Vec::with_capacity(self.pieces.len());
        for (index, piece) in self.pieces.iter().enumerate() {
            if index != 0
                && let Some(TextJoin::AuthoredSeparator(separator)) = &piece.join_before
            {
                append_bounded(&mut text, separator)?;
            }
            let start = text.len();
            let run = fixed
                .surface
                .run_text(piece.slice.run)
                .ok_or(SearchError::ContentProjection)?;
            let slice = run
                .get(
                    usize::try_from(piece.slice.start_byte)
                        .map_err(|_| SearchError::ResourceLimit)?
                        ..usize::try_from(piece.slice.end_byte)
                            .map_err(|_| SearchError::ResourceLimit)?,
                )
                .ok_or(SearchError::ContentProjection)?;
            append_bounded(&mut text, slice)?;
            parts.push(FixedUnitPart {
                piece,
                text_range: start..text.len(),
            });
        }
        Ok(FixedUnitText { text, parts })
    }
}

pub(super) fn append_bounded(target: &mut String, addition: &str) -> Result<(), SearchError> {
    let new_len = target
        .len()
        .checked_add(addition.len())
        .ok_or(SearchError::ResourceLimit)?;
    if new_len > MAX_FIXED_SEARCH_BYTES {
        return Err(SearchError::ResourceLimit);
    }
    target.push_str(addition);
    Ok(())
}
