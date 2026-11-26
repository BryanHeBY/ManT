//! Accepted generated BODY words observe their existing native cell interval.

use std::{cell::Cell, rc::Rc};

use super::super::field_buffer::{FieldBuffer, FieldCell, FlushReceipt, MAX_EDITABLE_APPEND_CELLS};
use super::super::{InlineBuilder, InlineExecutionState};
use super::NativeFieldDevice;

#[cfg(test)]
std::thread_local! {
    static RECEIPT_WORK: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug)]
pub(in crate::mandoc) struct RunInBoundaryReceipt {
    id: u64,
    facts: Rc<Cell<RunInBoundaryFacts>>,
}

#[derive(Clone, Copy, Debug, Default)]
struct RunInBoundaryFacts {
    materialized: bool,
    unprojected_gap: u16,
    following_word_joined: bool,
    starts_after_authored_break: bool,
}

impl Clone for RunInBoundaryReceipt {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            facts: Rc::new(Cell::new(self.facts.get())),
        }
    }
}

impl RunInBoundaryReceipt {
    pub(in crate::mandoc) fn share(&self) -> Self {
        Self {
            id: self.id,
            facts: Rc::clone(&self.facts),
        }
    }

    pub(in crate::mandoc) const fn id(&self) -> u64 {
        self.id
    }

    pub(in crate::mandoc) fn materialized(&self) -> bool {
        self.facts.get().materialized
    }

    pub(in crate::mandoc) fn unprojected_gap(&self) -> u16 {
        self.facts.get().unprojected_gap
    }

    pub(in crate::mandoc) fn following_word_joined(&self) -> bool {
        self.facts.get().following_word_joined
    }

    pub(in crate::mandoc) fn starts_after_authored_break(&self) -> bool {
        self.facts.get().starts_after_authored_break
    }
}

#[derive(Clone)]
pub(in crate::mandoc::inline::flow) struct PendingRunInBoundary {
    start: usize,
    end: usize,
    receipt: RunInBoundaryReceipt,
    awaiting_following_word: bool,
}

impl PendingRunInBoundary {
    /// Read the actual next `term_word` separator, including empty operands.
    /// This records a boundary without selecting or changing any register.
    pub(in crate::mandoc::inline::flow) fn observe_following_word(&mut self, separator: usize) {
        if std::mem::take(&mut self.awaiting_following_word) {
            let mut facts = self.receipt.facts.get();
            facts.following_word_joined = separator == 0;
            self.receipt.facts.set(facts);
        }
    }
}

impl InlineBuilder {
    /// Called immediately after the real generated word, before any BODY
    /// child. A stable word and its at-most-two Unicode glyph cells are the
    /// witness; eventual acceptance decides whether this is a visible seam.
    pub(in crate::mandoc) fn observe_run_in_boundary(&mut self) -> Option<RunInBoundaryReceipt> {
        let state = self.execution.definition.as_mut()?;
        let owner = format!(
            "{}{}",
            super::super::output::INTERNAL_FIELD_WORD,
            self.execution.native_owner_serial
        );
        // When `ReplaceLivePrefix` relocates surviving HEAD ink after the
        // generated U+00A0 cells, `append_owner_intervals` appends resumed
        // anchors at the end of `field_word_anchors`. Locate the first anchor
        // of this run-in word within the bounded replay window so `start..end`
        // spans the actual generated U+00A0 slots.
        let mut index = state
            .field_word_anchors
            .iter()
            .rposition(|anchor| anchor.owner == owner)?;
        let floor = index.saturating_sub(MAX_EDITABLE_APPEND_CELLS);
        while index > floor {
            if let Some(earlier) = state.field_word_anchors[floor..index]
                .iter()
                .rposition(|anchor| anchor.owner == owner)
            {
                index = floor + earlier;
            } else {
                break;
            }
        }
        let start = state.field_word_anchors[index].content;
        let end = state
            .field_word_anchors
            .get(index + 1)
            .map_or(state.field_buffer.cells().len(), |next| {
                next.start.min(next.content)
            });
        let receipt = RunInBoundaryReceipt {
            id: self.execution.native_owner_serial,
            facts: Rc::new(Cell::new(RunInBoundaryFacts::default())),
        };
        state.run_in_boundaries.push(PendingRunInBoundary {
            start,
            end,
            receipt: receipt.share(),
            awaiting_following_word: true,
        });
        Some(receipt)
    }

    /// Only a true flush supplies acceptance. Projection-only owner drains
    /// never update this receipt. Ordered ranges and pass ends advance once,
    /// so nested run-in words cost O(words + passes), not their product.
    pub(in crate::mandoc::inline::flow) fn capture_consumed_run_in_boundaries(
        &mut self,
        device: &NativeFieldDevice,
    ) {
        let Some(state) = &mut self.execution.definition else {
            return;
        };
        let boundaries = std::mem::take(&mut state.run_in_boundaries);
        let (passes, rejected_from) = match &device.receipt {
            FlushReceipt::UnmodeledRewind => return,
            FlushReceipt::Accepted { passes, .. } => (passes, None),
            FlushReceipt::Rejected {
                passes,
                rejected_from,
                ..
            } => (passes, Some(*rejected_from)),
        };
        let mut pass = 0;
        let buffer = &state.field_buffer;
        let mut covered = None;
        for boundary in boundaries {
            #[cfg(test)]
            RECEIPT_WORK.with(|work| work.set(work.get() + 1));
            let mut facts = boundary.receipt.facts.get();
            // term_flushln() can close the HEAD at an authored loop endline
            // before term_fill() rejects the entire run-in seam (term.c:
            // 143-146, 205-220). The first BODY word's NOSPACE cannot join
            // later output to that already closed HEAD row.
            if device.row_closed_by_author()
                && rejected_from.is_some_and(|start| start <= boundary.start)
            {
                facts.starts_after_authored_break = true;
                boundary.receipt.facts.set(facts);
                continue;
            }
            // The generated word enters with NOSPACE and writes only one
            // or two encode1(U+00A0) glyphs plus possible backlines. The
            // post-write content anchor excludes automatic separators and
            // device padding; the cell check excludes pairing backlines.
            for cell in boundary.start..boundary.end.min(buffer.cells().len()) {
                #[cfg(test)]
                RECEIPT_WORK.with(|work| work.set(work.get() + 1));
                if !matches!(
                    buffer.cells()[cell],
                    FieldCell::Graph { text: '\u{a0}', .. }
                ) {
                    continue;
                }
                while pass < passes.len() && passes[pass].end <= cell {
                    #[cfg(test)]
                    RECEIPT_WORK.with(|work| work.set(work.get() + 1));
                    pass += 1;
                }
                let Some(printed) = passes.get(pass).filter(|printed| printed.start <= cell) else {
                    continue;
                };
                // term_flushln skips breakable blanks between accepted
                // passes, never an encode1 U+00A0 graph (term.c:205-207).
                if buffer.projection_length(cell, cell + 1) > 0 {
                    facts.materialized = true;
                    break;
                }
                let covered = covered.get_or_insert_with(|| buffer.covered_pass_retreats(passes));
                // A `PlacedBlank` U+00A0 has `projection_length == 0` until
                // `deferred_blanks` materializes it at flush; if it is not
                // covered by a backline, mark the boundary materialized so
                // `unprojected_gap` does not double-count the separator.
                if buffer.has_deferred_blank_at(cell) && covered.binary_search(&cell).is_err() {
                    facts.materialized = true;
                    break;
                }
                // If `append_run_in_cells` relocated surviving HEAD ink into
                // BODY after this cell and `.br` flushed before any BODY word
                // ran, `TERMP_NOSPACE` (`mdoc_term.c:772`) still joined that
                // ink to the run-in seam with no automatic word separator.
                if boundary.awaiting_following_word
                    && has_local_ink(buffer, covered, boundary.end..printed.end)
                {
                    facts.following_word_joined = true;
                }
                // An accepted U+00A0 can outlive its projection, for
                // example when later input rewrites the tail after an
                // escape marker (term.c:659-661,701-714). It contributes
                // reading separation only between surviving HEAD/BODY
                // ink of the same pass. A BACKBEFORE before the next
                // glyph covers the space even when it owns no scalar.
                if covered.binary_search(&cell).is_err()
                    && gap_reaches_next_graph(&buffer.cells()[cell + 1..printed.end])
                    && has_local_ink(buffer, covered, (printed.start..boundary.start).rev())
                    && has_local_ink(buffer, covered, boundary.end..printed.end)
                {
                    facts.unprojected_gap = facts.unprojected_gap.saturating_add(1);
                }
            }
            boundary.receipt.facts.set(facts);
        }
    }
}

fn gap_reaches_next_graph(cells: &[FieldCell]) -> bool {
    // term_field() skips the native controls but emits either a glyph or
    // the backspace that can cover the generated cell (term.c:389-434).
    cells
        .iter()
        .take(MAX_EDITABLE_APPEND_CELLS)
        .find(|cell| {
            matches!(
                cell,
                FieldCell::Graph { .. } | FieldCell::Hyphen | FieldCell::Backline
            )
        })
        .is_some_and(|cell| *cell != FieldCell::Backline)
}

fn has_local_ink(
    buffer: &FieldBuffer,
    covered: &[usize],
    positions: impl Iterator<Item = usize>,
) -> bool {
    positions.take(MAX_EDITABLE_APPEND_CELLS).any(|cell| {
        matches!(
            buffer.cells()[cell],
            FieldCell::Graph { text, .. } if !text.is_whitespace()
        ) && buffer.projection_length(cell, cell + 1) > 0
            && covered.binary_search(&cell).is_err()
    })
}

impl InlineExecutionState {
    /// Clone-time rebinding borrows only the active unconsumed suffix.
    pub(in crate::mandoc) fn shared_run_in_boundaries(
        &self,
    ) -> impl Iterator<Item = RunInBoundaryReceipt> + '_ {
        self.definition
            .iter()
            .flat_map(|state| state.run_in_boundaries.iter())
            .map(|boundary| boundary.receipt.share())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mandoc::formatter::{AuthorFlow, FormatterState};

    fn builder() -> InlineBuilder {
        let mut builder = InlineBuilder::new();
        builder.inherit_author_execution(AuthorFlow::Automatic, false);
        builder.begin_definition_head_consumption();
        builder
    }

    #[test]
    fn only_a_surviving_accepted_generated_word_materializes_its_boundary() {
        // Full inset/diag pending-z controls first ran all five oracle
        // profiles. A one-cell generated word can be overstruck completely;
        // the two-cell word still retains its second U+00A0 scalar.
        for (cells, survives) in [(1, false), (2, true)] {
            let mut builder = builder();
            builder.append_text("HEAD");
            builder.inherit_zero_advance_armed(true);
            builder
                .definition_state_mut()
                .field_buffer
                .apply_writes(&[super::super::super::field_buffer::FieldWrite::ArmBackafter]);
            builder.append_run_in_cells(cells, cells, true);
            let boundary = builder.observe_run_in_boundary().unwrap();
            assert!(!boundary.materialized());
            assert!(!boundary.following_word_joined());
            builder.append_text("BODY");
            assert!(boundary.following_word_joined());
            builder.flush_native_field();
            assert_eq!(boundary.materialized(), survives);
            assert!(
                builder
                    .execution
                    .definition
                    .as_ref()
                    .unwrap()
                    .run_in_boundaries
                    .is_empty()
            );
        }
    }

    #[test]
    fn following_word_observation_is_inert_consumed_once_and_ends_at_flush() {
        for empty in [false, true] {
            let mut baseline = builder();
            let mut observed = builder();
            baseline.append_run_in_cells(1, 1, true);
            observed.append_run_in_cells(1, 1, true);
            let receipt = observed.observe_run_in_boundary().unwrap();
            if empty {
                baseline.execute_empty_word();
                observed.execute_empty_word();
            }
            baseline.append_text("BODY");
            observed.append_text("BODY");
            assert!(receipt.following_word_joined());
            assert_eq!(
                mant_ir::inline_plain_text(&baseline.nodes),
                mant_ir::inline_plain_text(&observed.nodes)
            );
            assert_eq!(
                baseline.definition_state_mut().field_buffer.cells(),
                observed.definition_state_mut().field_buffer.cells()
            );
        }
        let mut flushed = builder();
        flushed.append_run_in_cells(1, 1, true);
        let receipt = flushed.observe_run_in_boundary().unwrap();
        flushed.flush_native_field();
        flushed.append_text("BODY");
        assert!(!receipt.following_word_joined());
    }

    #[test]
    fn speculative_formatter_forks_rebind_only_their_active_body_observers() {
        let mut formatter = FormatterState::default();
        formatter.begin_definition_body(true);
        let mut output = Vec::new();
        let boundary = formatter.with_output_builder(&mut output, |builder| {
            builder.inherit_author_execution(AuthorFlow::Automatic, false);
            builder.begin_definition_head_consumption();
            builder.append_run_in_cells(1, 1, true);
            builder.observe_run_in_boundary()
        });
        formatter.note_definition_run_in_executed(boundary);
        let mut fork = formatter.clone();
        fork.with_output_builder(&mut output.clone(), |builder| {
            builder.flush_native_field();
        });
        assert!(fork.finish_definition_body().materialized_run_in_boundary());
        assert!(
            !formatter
                .finish_definition_body()
                .materialized_run_in_boundary()
        );
    }

    #[test]
    fn receipt_work_is_linear_in_the_active_native_suffix() {
        for count in [64, 256, 1024] {
            let mut builder = builder();
            let mut boundaries = Vec::new();
            for _ in 0..count {
                builder.append_run_in_cells(1, 1, true);
                boundaries.push(builder.observe_run_in_boundary().unwrap());
                builder.append_text("X");
            }
            RECEIPT_WORK.with(|work| work.set(0));
            builder.flush_native_field();
            assert!(boundaries.iter().all(RunInBoundaryReceipt::materialized));
            assert!(RECEIPT_WORK.with(Cell::get) <= count * 3 + 1);
            assert!(
                builder
                    .execution
                    .definition
                    .as_ref()
                    .unwrap()
                    .run_in_boundaries
                    .is_empty()
            );
        }
    }

    #[test]
    fn cross_pass_ranges_and_rejected_suffixes_visit_each_cell_once() {
        use super::super::super::field_buffer::{FillBoundary, FillPass, FlushReceipt};
        for count in [64, 256, 1024] {
            let mut builder = builder();
            let mut boundaries = Vec::new();
            for index in 0..count {
                let cells = 1 + index % 2;
                builder.append_run_in_cells(cells, cells, true);
                boundaries.push(builder.observe_run_in_boundary().unwrap());
                builder.append_text("X");
            }
            let mut device = builder.native_field_device(false).unwrap();
            let state = builder.execution.definition.as_ref().unwrap();
            let end = state.run_in_boundaries[count / 2].start;
            // Adversarial receipt: every native cell ends a separate pass,
            // including boundaries crossing pass ends. The second half is
            // a rejected suffix. This tests receipt consumption, not roff.
            device.receipt = FlushReceipt::Rejected {
                passes: (1..=end)
                    .map(|end| FillPass {
                        start: end - 1,
                        end,
                        width: 1,
                        units: 24,
                        boundary: FillBoundary::Width,
                    })
                    .collect(),
                rejected_from: end,
                definitive: true,
                native_cells: state.field_buffer.cells().to_vec(),
            };
            RECEIPT_WORK.with(|work| work.set(0));
            builder.capture_consumed_run_in_boundaries(&device);
            for (index, receipt) in boundaries.iter().enumerate() {
                assert_eq!(receipt.materialized(), index < count / 2);
            }
            assert!(RECEIPT_WORK.with(Cell::get) <= count * 6);
            let work = RECEIPT_WORK.with(Cell::get);
            builder.capture_consumed_run_in_boundaries(&device);
            assert_eq!(RECEIPT_WORK.with(Cell::get), work);
        }
    }

    #[test]
    fn empty_or_padding_only_intervals_never_materialize_a_generated_word() {
        let mut builder = builder();
        builder.append_run_in_cells(0, 0, true);
        let empty = builder.observe_run_in_boundary().unwrap();
        builder.append_text(" ");
        // A deliberately broad interval demonstrates that even accepted
        // automatic/authored ASCII padding cannot stand in for U+00A0.
        let padding = builder.observe_run_in_boundary().unwrap();
        builder.flush_native_field();
        assert!(!empty.materialized());
        assert!(!padding.materialized());
    }
}
