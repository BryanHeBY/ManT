//! Derive each source owner's accepted scalar and padding ranges from native passes.

use std::collections::BTreeMap;

use super::super::super::{
    NativeWordAnchor, WordProjectionPolicy,
    field_buffer::{FieldBuffer, FieldCell, FillBoundary, FillPass},
};
use super::super::INTERNAL_FIELD_WORD;

#[cfg(test)]
std::thread_local! {
    pub(super) static OWNER_PASS_INTERSECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug, Default)]
pub(in crate::mandoc::inline::flow) struct OwnerAcceptance {
    pub(in crate::mandoc::inline::flow::output) starts_accepted_row: bool,
    pub(in crate::mandoc::inline::flow) scalars: usize,
    pub(in crate::mandoc::inline::flow) native_cells: bool,
    /// The first accepted interval printed this word's deferred device pad.
    /// An accepted NBRZW prefix alone cannot print it (term.c:389-427).
    pub(in crate::mandoc::inline::flow) prefix_printed: bool,
    /// Source scalars in an accepted pass with no surviving device cell.
    /// They can be unprinted padding or covered glyphs.
    pub(in crate::mandoc::inline::flow) omitted_scalars: Vec<ScalarOmission>,
    pub(in crate::mandoc::inline::flow::output) ordered_glyphs: Vec<OrderedGlyph>,
}

#[derive(Clone, Debug)]
pub(in crate::mandoc::inline::flow::output) struct OrderedGlyph {
    pub(in crate::mandoc::inline::flow::output) scalar: usize,
    pub(in crate::mandoc::inline::flow::output) character: char,
    pub(in crate::mandoc::inline::flow::output) receipts: Vec<String>,
}

#[derive(Clone, Debug)]
pub(in crate::mandoc::inline::flow) struct ScalarOmission {
    pub(in crate::mandoc::inline::flow) range: std::ops::Range<usize>,
    pub(in crate::mandoc::inline::flow::output) covered: bool,
}

impl OwnerAcceptance {
    pub(in crate::mandoc::inline::flow) fn has_ink_edits(&self) -> bool {
        !self.omitted_scalars.is_empty() || !self.ordered_glyphs.is_empty()
    }

    fn collect_ordered_glyphs(
        &mut self,
        buffer: &FieldBuffer,
        order: &BTreeMap<usize, Vec<String>>,
        interval: std::ops::Range<usize>,
        offset: usize,
    ) {
        let start = interval.start;
        for (&cell, receipts) in order.range(interval) {
            let character = match buffer.cells()[cell] {
                FieldCell::Graph { text, .. } => text,
                FieldCell::Hyphen => '-',
                _ => continue,
            };
            self.ordered_glyphs.push(OrderedGlyph {
                scalar: offset + buffer.projection_length(start, cell),
                character,
                receipts: receipts.clone(),
            });
        }
    }

    fn omit_cell(
        &mut self,
        buffer: &FieldBuffer,
        origin: (usize, usize),
        cell: usize,
        covered: bool,
    ) {
        let (start, offset) = origin;
        let first = offset + buffer.projection_length(start, cell);
        let last = offset + buffer.projection_length(start, cell + 1);
        self.omit(first, last, covered);
    }

    fn omit(&mut self, first: usize, last: usize, covered: bool) {
        if first < last {
            if let Some(previous) = self.omitted_scalars.last_mut()
                && previous.range.end == first
                && previous.covered == covered
            {
                previous.range.end = last;
            } else {
                self.omitted_scalars.push(ScalarOmission {
                    range: first..last,
                    covered,
                });
            }
        }
    }

    fn omit_skipped_gap(
        &mut self,
        buffer: &FieldBuffer,
        gap: std::ops::Range<usize>,
        owner: std::ops::Range<usize>,
    ) {
        if !buffer.projects_skipped_gap(gap.start, gap.end) {
            return;
        }
        let first = gap.start.max(owner.start);
        let last = gap.end.min(owner.end);
        if first < last {
            let next = self.scalars + buffer.projection_length(first, last);
            self.omit(self.scalars, next, false);
            self.scalars = next;
        }
    }
}

fn pass_projection_end(buffer: &FieldBuffer, pass: &FillPass) -> (usize, bool, usize, bool) {
    let printed_end = buffer.printed_content_end(pass.start, pass.end);
    let printed = printed_end > pass.start;
    let projected_cut = buffer.projected_pass_cut(pass.end);
    let projected = projected_cut.is_some();
    // Acknowledged endlines already removed trailing SP strictly before
    // `cut` from the projection. Do not charge those old scalar slots to
    // later output when term_field's receipt retires unprinted padding,
    // while keeping blanks in `cut..pass.end` that sit after the LineBreak
    // so owner acceptance still omits them (term.c:205-220).
    let mut projected_end = pass.end;
    if let Some(cut) = projected_cut {
        while projected_end > printed_end {
            let cell = projected_end - 1;
            if cell >= cut && buffer.projection_length(cell, projected_end) != 0 {
                break;
            }
            if !matches!(
                buffer.cells()[cell],
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
            ) && buffer.projection_length(cell, projected_end) != 0
            {
                break;
            }
            projected_end = cell;
        }
    }
    (printed_end, printed, projected_end, projected)
}

/// Accepted projection range of each stable source word. A delayed glyph
/// can re-enter that owner after a later word marker without becoming its
/// rejected suffix. Native cell intervals, not append order, decide limits.
pub(in crate::mandoc::inline::flow) fn accepted_owner_lengths(
    buffer: &FieldBuffer,
    anchors: &[NativeWordAnchor],
    passes: &[FillPass],
    preserve_literal_whitespace: bool,
) -> BTreeMap<String, OwnerAcceptance> {
    let mut lengths = BTreeMap::new();
    let (covered, order) = collect_native_overprints(buffer, anchors, passes);
    let printed_ends: Vec<_> = passes
        .iter()
        .map(|pass| pass_projection_end(buffer, pass))
        .collect();
    let mut pass_index = 0;
    for (index, anchor) in anchors.iter().enumerate() {
        let owner = &anchor.owner;
        let content = &anchor.content;
        #[cfg(test)]
        OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
        // BACKBEFORE may replace the previous trailing blank before this
        // word's original buffer end. Its landing, not that old end, owns
        // the replacement graph (term.c:901-908).
        let end = anchors
            .get(index + 1)
            .map_or(buffer.cells().len(), |next| next.start.min(next.content));
        if end <= *content {
            // An empty word can be followed by BACKBEFORE replacing its
            // trailing native blank. Its zero-length projection interval
            // cannot consume a pass needed by that replacement's owner.
            let acceptance: &mut OwnerAcceptance = lengths.entry(owner.clone()).or_default();
            acceptance.starts_accepted_row |=
                starts_projected_pass(buffer, passes, anchor.start..end);
            continue;
        }
        while passes
            .get(pass_index)
            .is_some_and(|pass| pass.end <= *content)
        {
            #[cfg(test)]
            OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
            pass_index += 1;
        }
        let acceptance: &mut OwnerAcceptance = lengths.entry(owner.clone()).or_default();
        while let Some(pass) = passes.get(pass_index) {
            #[cfg(test)]
            OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
            let start = (*content).max(pass.start);
            let accepted_end = end.min(pass.end);
            let (printed_end, printed, projected_end, projected) = printed_ends[pass_index];
            // Include skipped scalar positions even when a pending owner
            // fragment consists entirely of the skipped native spaces.
            if let Some(previous) = pass_index.checked_sub(1).map(|i| &passes[i]) {
                acceptance.omit_skipped_gap(buffer, previous.end..pass.start, *content..end);
            }
            if start < accepted_end {
                let projected_end = projected_end.min(accepted_end);
                let offset = acceptance.scalars;
                let origin = (start, offset);
                acceptance.collect_ordered_glyphs(
                    buffer,
                    &order,
                    start..projected_end.max(start),
                    offset,
                );
                let covered_start = covered.partition_point(|&cell| cell < start);
                for &cell in covered[covered_start..]
                    .iter()
                    .take_while(|&&cell| cell < projected_end)
                {
                    acceptance.omit_cell(buffer, origin, cell, true);
                }
                let source_literal = preserve_literal_whitespace
                    && anchor.projection == WordProjectionPolicy::SourceLiteralWhitespace;
                let padding_start = if source_literal && !printed {
                    // A source literal row remains nonempty authored content
                    // even when term_field prints no deferred SP/TAB. This
                    // never changes native acceptance or revives rejected cells.
                    accepted_end
                } else {
                    start.max(printed_end)
                };
                for cell in padding_start..projected_end {
                    // An already projected literal TAB is source content.
                    // In a filled word the same deferred device cell is
                    // still omitted when term_field() did not print it.
                    if source_literal && projected && matches!(buffer.cells()[cell], FieldCell::Tab)
                    {
                        continue;
                    }
                    if matches!(
                        buffer.cells()[cell],
                        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank | FieldCell::Tab
                    ) {
                        acceptance.omit_cell(buffer, origin, cell, false);
                    }
                }
                acceptance.scalars += buffer.projection_length(start, projected_end);
                acceptance.native_cells = true;
                if start == *content {
                    acceptance.prefix_printed |=
                        buffer.printed_columns(start, accepted_end, 0, 0).is_some();
                }
            }
            if pass.end >= end {
                break;
            }
            pass_index += 1;
        }
        if !acceptance.native_cells {
            acceptance.starts_accepted_row |=
                starts_projected_pass(buffer, passes, anchor.start..end);
        }
    }
    resolve_buffered_prefixes(anchors, passes, &printed_ends, &covered, &mut lengths);
    lengths
}

fn starts_projected_pass(
    buffer: &FieldBuffer,
    passes: &[FillPass],
    range: std::ops::Range<usize>,
) -> bool {
    // A control-only owner may carry the projected endline before the
    // new pass even when its content cursor has no scalars (term.c:205-220).
    if range.start > range.end {
        return false;
    }
    let index = passes.partition_point(|pass| pass.start < range.start);
    index != 0
        && index < passes.len()
        && passes[index - 1].end <= range.end
        && passes[index - 1].boundary == FillBoundary::WordEndBreak
        && buffer.has_projected_pass(passes[index - 1].end)
}

fn collect_native_overprints(
    buffer: &FieldBuffer,
    anchors: &[NativeWordAnchor],
    passes: &[FillPass],
) -> (Vec<usize>, BTreeMap<usize, Vec<String>>) {
    let mut covered = Vec::new();
    let mut order = BTreeMap::<_, Vec<_>>::new();
    if let Some(first_word) = anchors
        .first()
        .and_then(|anchor| anchor.owner.strip_prefix(INTERNAL_FIELD_WORD))
    {
        let mut group = 0;
        buffer.visit_accepted_overprints(
            passes,
            |target, _| {
                if covered.last() != Some(&target) {
                    covered.push(target);
                }
            },
            |ordered| {
                let size = ordered.len();
                for (rank, cell) in ordered.into_iter().enumerate() {
                    order
                        .entry(cell)
                        .or_default()
                        .push(format!("{first_word}:{group}:{rank}:{size}"));
                }
                group += 1;
            },
        );
    }
    // Device encounter order belongs to the paint groups above. Coverage
    // queries use raw target positions even when a replay retreats left.
    covered.sort_unstable();
    covered.dedup();
    (covered, order)
}

fn resolve_buffered_prefixes(
    anchors: &[NativeWordAnchor],
    passes: &[FillPass],
    printed_ends: &[(usize, bool, usize, bool)],
    covered: &[usize],
    lengths: &mut BTreeMap<String, OwnerAcceptance>,
) {
    // term_field() releases a buffered gap at the next printing cell in
    // the same pass. That cell can belong to a later word after the gap's
    // original glyph was overwritten. Gaps skipped between passes never
    // reach term_field(), even if the original owner still has ink.
    for anchor in anchors {
        if let Some(gap) = anchor.projected_gap_cell() {
            let pass = passes.partition_point(|pass| pass.end <= gap);
            let printed = !anchor.projected_gap.superseded
                && passes
                    .get(pass)
                    .is_some_and(|pass_row| pass_row.start <= gap && gap < printed_ends[pass].0);
            lengths
                .entry(anchor.owner.clone())
                .or_default()
                .prefix_printed = printed && covered.binary_search(&gap).is_err();
        }
    }
}
