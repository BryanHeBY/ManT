//! Source-preserving geometry for one logical hard row.
//!
//! Preparation resolves grapheme and TAB advances once, before viewport
//! wrapping. Styles, links, generated padding and painting remain consumers'
//! responsibilities. No terminal escape sequences are interpreted here.

use std::ops::Range;

/// How a viewport may divide a logical hard row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapMode {
    /// Prefer internal breakable whitespace, retaining its exact source seam.
    Word,
    /// Divide only between complete graphemes or expanded TAB cells.
    Character,
}

/// The source relationship of one indivisible display unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    /// A complete source grapheme; control sanitization belongs to painting.
    Grapheme,
    /// One space in a TAB's expansion, with zero-based part and total width.
    ///
    /// Every part retains the same original byte and scalar ranges. A TAB
    /// spanning visual rows retains one authored scalar identity for search.
    Tab {
        /// Zero-based space within this TAB's expansion.
        part: usize,
        /// Total spaces charged to this TAB in the logical hard row.
        total: usize,
    },
}

/// One complete source grapheme or one displayed space of an expanded TAB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceUnit {
    /// Half-open UTF-8 bytes in the original hard row.
    pub bytes: Range<usize>,
    /// Half-open Unicode scalar positions in the original hard row.
    pub scalars: Range<usize>,
    /// Terminal columns charged to this display unit.
    pub columns: usize,
    /// Whether this unit may belong to an internal word-wrap seam.
    pub breakable: bool,
    /// Grapheme or exact TAB expansion part.
    pub kind: UnitKind,
}

/// One visual row borrowing units and source from a [`PreparedLine`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowPlan {
    /// Half-open display-unit indices in [`PreparedLine::units`].
    pub units: Range<usize>,
    /// Original UTF-8 bytes touched by the painted units.
    ///
    /// Adjacent rows can overlap this range only when a TAB is divided into
    /// visual space parts. Use each unit's scalar identity to deduplicate it.
    pub source: Range<usize>,
    /// Exact unpainted internal separator consumed after this row.
    ///
    /// An empty range means no soft seam. Authored leading/trailing whitespace
    /// and whitespace-only hard rows never become unpainted seams.
    pub seam_after: Range<usize>,
    /// Resolved viewport-local origin for this visual row.
    pub origin: usize,
    /// Terminal columns occupied after the origin, excluding the soft seam.
    pub columns: usize,
}

/// Borrowed hard-row text and its reusable source-to-cell geometry.
///
/// TAB stops are eight columns apart in the block/cell's local coordinates.
/// `tab_origin` is the original hard row's structural origin. Source graphemes
/// advance that logical cursor once; viewport continuation origins, table
/// rails, column placement and later alignment do not change the TAB phase.
#[derive(Debug)]
pub struct PreparedLine<'text> {
    source: &'text str,
    units: Vec<SourceUnit>,
    columns: usize,
}

impl<'text> PreparedLine<'text> {
    /// Prepare one hard row, excluding its newline delimiter.
    ///
    /// Segment the complete row so graphemes can cross source-style boundaries.
    /// Consumers retain their own source inspection budgets and styling maps.
    #[must_use]
    pub fn new(source: &'text str, tab_origin: usize) -> Self {
        let mut units = Vec::new();
        let mut scalar = 0;
        let mut phase = tab_origin % 8;
        let mut columns = 0_usize;
        for grapheme in crate::cells::graphemes(source) {
            #[cfg(test)]
            workload::visit(1);
            let scalars = scalar..scalar + grapheme.text().chars().count();
            scalar = scalars.end;
            if grapheme.text() == "\t" {
                let total = 8 - phase;
                units.extend((0..total).map(|part| {
                    #[cfg(test)]
                    workload::visit(1);
                    SourceUnit {
                        bytes: grapheme.bytes(),
                        scalars: scalars.clone(),
                        columns: 1,
                        breakable: true,
                        kind: UnitKind::Tab { part, total },
                    }
                }));
                columns = columns.saturating_add(total);
                phase = 0;
            } else {
                let width = if grapheme.text().chars().any(char::is_control) {
                    1
                } else {
                    grapheme.columns()
                };
                units.push(SourceUnit {
                    bytes: grapheme.bytes(),
                    scalars,
                    columns: width,
                    breakable: grapheme.text().chars().all(soft_separator),
                    kind: UnitKind::Grapheme,
                });
                columns = columns.saturating_add(width);
                phase = (phase + width % 8) % 8;
            }
        }
        Self {
            source,
            units,
            columns,
        }
    }

    /// Borrow the unchanged original hard row.
    #[must_use]
    pub const fn source(&self) -> &'text str {
        self.source
    }

    /// Borrow the complete source-to-cell map, including every TAB space part.
    #[must_use]
    pub fn units(&self) -> &[SourceUnit] {
        &self.units
    }

    /// Width of the original hard row after local TAB expansion.
    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }

    /// Largest run that contains no word-wrap separator.
    #[must_use]
    pub fn longest_word_columns(&self) -> usize {
        self.units
            .split(|unit| unit.breakable)
            .map(|word| word.iter().map(|unit| unit.columns).sum())
            .max()
            .unwrap_or(0)
    }

    /// Plan visual rows within a viewport, retaining exact omitted separators.
    ///
    /// Origins are already resolved by the consumer's viewport rules. They
    /// reserve space on first/continuation rows without rephasing source TABs.
    /// An overwide first grapheme is kept whole; consumers may retain their
    /// existing bounded replacement or fallback policy when painting it.
    #[must_use]
    pub fn wrap(
        &self,
        width: usize,
        first_origin: usize,
        continuation_origin: usize,
        mode: WrapMode,
    ) -> Vec<RowPlan> {
        if self.units.is_empty() {
            return vec![self.row(0..0, 0..0, first_origin)];
        }
        let content_end = self
            .units
            .iter()
            .rposition(|unit| {
                #[cfg(test)]
                workload::visit(1);
                !unit.breakable
            })
            .map_or(0, |index| index + 1);
        let mut rows = Vec::new();
        let mut start = 0;
        while start < self.units.len() {
            let origin = if rows.is_empty() {
                first_origin
            } else {
                continuation_origin
            };
            let available = width.saturating_sub(origin).max(1);
            let mut used = 0_usize;
            let mut fit = start;
            while let Some(unit) = self.units.get(fit) {
                #[cfg(test)]
                workload::visit(1);
                if used.saturating_add(unit.columns) > available && fit > start {
                    break;
                }
                used = used.saturating_add(unit.columns);
                fit += 1;
            }
            let seam = if mode == WrapMode::Word && fit < self.units.len() {
                self.word_seam(start, fit, content_end)
            } else {
                None
            };
            if let Some(seam) = seam {
                rows.push(self.row(start..seam.start, seam.clone(), origin));
                start = seam.end;
            } else {
                rows.push(self.row(start..fit, fit..fit, origin));
                start = fit;
            }
        }
        rows
    }

    fn word_seam(&self, start: usize, fit: usize, content_end: usize) -> Option<Range<usize>> {
        let mut has_content = false;
        let mut seam: Option<Range<usize>> = None;
        let mut in_separator = false;
        for (relative, unit) in self.units[start..fit].iter().enumerate() {
            #[cfg(test)]
            workload::visit(1);
            let index = start + relative;
            if unit.breakable {
                if has_content && index < content_end {
                    if in_separator {
                        if let Some(seam) = &mut seam {
                            seam.end = index + 1;
                        }
                    } else {
                        seam = Some(index..index + 1);
                    }
                }
                in_separator = true;
            } else {
                has_content = true;
                in_separator = false;
            }
        }
        // A word can fill the row exactly, leaving its entire separator just
        // beyond the fitted prefix. Consume that source seam once as well.
        if has_content && fit < content_end && self.units[fit].breakable {
            let end = self.units[fit..]
                .iter()
                .position(|unit| {
                    #[cfg(test)]
                    workload::visit(1);
                    !unit.breakable
                })
                .map_or(self.units.len(), |offset| fit + offset);
            let from = if in_separator {
                seam.as_ref().map_or(fit, |seam| seam.start)
            } else {
                fit
            };
            seam = Some(from..end);
        }
        seam
    }

    fn row(&self, units: Range<usize>, seam: Range<usize>, origin: usize) -> RowPlan {
        let source = self.source_range(units.clone());
        let seam_after = if seam.is_empty() {
            source.end..source.end
        } else {
            self.source_range(seam)
        };
        let columns = self.units[units.clone()]
            .iter()
            .map(|unit| {
                #[cfg(test)]
                workload::visit(1);
                unit.columns
            })
            .sum();
        RowPlan {
            units,
            source,
            seam_after,
            origin,
            columns,
        }
    }

    fn source_range(&self, units: Range<usize>) -> Range<usize> {
        let start = self
            .units
            .get(units.start)
            .map_or(self.source.len(), |unit| unit.bytes.start);
        let end = units
            .end
            .checked_sub(1)
            .and_then(|index| self.units.get(index))
            .map_or(start, |unit| unit.bytes.end);
        start..end
    }
}

/// Unicode whitespace that permits a word-wrap seam.
#[must_use]
pub fn soft_separator(character: char) -> bool {
    character.is_whitespace() && !matches!(character, '\u{a0}' | '\u{2007}' | '\u{202f}')
}

#[cfg(test)]
mod workload {
    use std::cell::Cell;

    thread_local! {
        static VISITS: Cell<Option<usize>> = const { Cell::new(None) };
    }

    pub(super) fn visit(count: usize) {
        VISITS.with(|visits| {
            if let Some(current) = visits.get() {
                visits.set(Some(current + count));
            }
        });
    }

    pub(super) fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize) {
        struct Restore(Option<usize>);
        impl Drop for Restore {
            fn drop(&mut self) {
                VISITS.set(self.0);
            }
        }
        let restore = Restore(VISITS.replace(Some(0)));
        let result = operation();
        let visits = VISITS.get().expect("measuring line layout");
        drop(restore);
        (result, visits)
    }
}

#[cfg(test)]
mod tests;
