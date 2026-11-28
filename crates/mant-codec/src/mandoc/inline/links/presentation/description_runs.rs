//! Accepted physical description membership, private to one output publication.

use std::ops::Range;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PhysicalWriter {
    Authored,
    Earlier(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PhysicalRun {
    pub(super) scalars: Range<usize>,
    pub(super) writer: PhysicalWriter,
}

/// Capture one surviving description wrapper, never all fragments sharing
/// an authored occurrence. Empty native seam wrappers have no local paint.
#[derive(Default)]
pub(super) struct AcceptedDescription {
    cursor: usize,
    preceding: Vec<PhysicalRun>,
    tail: Option<PhysicalRun>,
    disqualified: bool,
    requires_restriction: bool,
}

impl AcceptedDescription {
    pub(super) fn mark_uncertain(&mut self) {
        self.requires_restriction = true;
    }

    pub(super) fn proven_scalars(&self) -> Option<usize> {
        (!self.disqualified).then_some(self.cursor)
    }

    pub(super) const fn requires_restriction(&self) -> bool {
        self.requires_restriction
    }

    pub(super) fn authored_members(&self) -> Vec<Range<usize>> {
        if self.disqualified {
            return Vec::new();
        }
        self.preceding
            .iter()
            .chain(&self.tail)
            .filter(|run| run.writer == PhysicalWriter::Authored)
            .map(|run| run.scalars.clone())
            .collect()
    }

    pub(super) fn record_text(&mut self, writer: PhysicalWriter, value: &str) {
        if value.is_empty() {
            return;
        }
        self.requires_restriction |= matches!(writer, PhysicalWriter::Earlier(_));
        if !self.disqualified {
            // Byte length is a quick bound; don't scan a huge leaf just to reject it.
            if value.len() > crate::mandoc::link_layout::MAX_LABEL {
                self.disqualified = true;
            } else {
                self.record(writer, value.chars().count());
            }
        }
    }

    pub(super) fn record(&mut self, writer: PhysicalWriter, count: usize) {
        if count == 0 {
            return;
        }
        self.requires_restriction |= matches!(writer, PhysicalWriter::Earlier(_));
        if self.disqualified {
            return;
        }
        let start = self.cursor;
        self.cursor += count;
        if self.cursor > crate::mandoc::link_layout::MAX_LABEL
            || self.preceding.len() >= crate::mandoc::link_layout::MAX_RUNS * 2
        {
            self.disqualified = true;
            return;
        }
        if let Some(last) = &mut self.tail
            && last.writer == writer
        {
            last.scalars.end = self.cursor;
        } else if let Some(previous) = self.tail.replace(PhysicalRun {
            scalars: start..self.cursor,
            writer,
        }) {
            self.preceding.push(previous);
        }
    }
}

#[cfg(test)]
pub(super) use trace::{capture, observe};

#[cfg(test)]
mod trace {
    use super::super::{AcceptedLabel, AnnotationKey};
    use super::PhysicalRun;
    use std::collections::HashMap;

    type ReceiptTrace = Vec<(AnnotationKey, Vec<PhysicalRun>)>;

    std::thread_local! {
        static TRACE: std::cell::RefCell<Option<ReceiptTrace>> =
            const { std::cell::RefCell::new(None) };
    }

    pub(in crate::mandoc::inline::links::presentation) fn observe(
        labels: &HashMap<AnnotationKey, AcceptedLabel>,
    ) {
        TRACE.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.extend(
                    labels
                        .iter()
                        .filter(|(_, label)| !label.descriptions.is_empty())
                        .map(|(&key, label)| {
                            (
                                key,
                                label
                                    .descriptions
                                    .values()
                                    .flat_map(|fragment| {
                                        fragment.preceding.iter().chain(&fragment.tail)
                                    })
                                    .cloned()
                                    .collect(),
                            )
                        }),
                );
            }
        });
    }

    pub(in crate::mandoc::inline::links::presentation) fn capture<T>(
        run: impl FnOnce() -> T,
    ) -> (T, ReceiptTrace) {
        TRACE.set(Some(Vec::new()));
        let output = run();
        let trace = TRACE.take().unwrap();
        (output, trace)
    }
}
