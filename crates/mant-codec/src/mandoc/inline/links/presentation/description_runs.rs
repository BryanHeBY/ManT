//! Accepted physical description membership, private to one output publication.

use std::ops::Range;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PhysicalWriter {
    Authored,
    Earlier(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PhysicalRun {
    pub(super) fragment: usize,
    pub(super) scalars: Range<usize>,
    pub(super) writer: PhysicalWriter,
}

/// The enclosing map is keyed by source occurrence, never by the URI.
/// Coordinates are local to original surviving description fragments after
/// native paint, before releasing existing legacy link wrappers.
/// No run spans a physical fragment/cut. This is local publication evidence,
/// not a globally addressable span: a cut may disable the native receiver,
/// so absence of an Earlier run is not proof that an arbitrary fragment can
/// be safely republished under a narrower annotation.
#[derive(Default)]
pub(super) struct AcceptedDescription {
    fragment: usize,
    cursor: usize,
    preceding: Vec<PhysicalRun>,
    tail: Option<PhysicalRun>,
    disqualified: bool,
}

impl AcceptedDescription {
    pub(super) fn enter(&mut self) {
        self.fragment += 1;
        self.cursor = 0;
    }

    pub(super) fn authored_members(&self) -> Vec<Range<usize>> {
        if self.disqualified || self.fragment != 1 || self.preceding.is_empty() {
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
        if self.disqualified || count == 0 {
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
            && last.fragment == self.fragment
            && last.writer == writer
        {
            last.scalars.end = self.cursor;
        } else if let Some(previous) = self.tail.replace(PhysicalRun {
            fragment: self.fragment,
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
                        .filter(|(_, label)| label.description.fragment != 0)
                        .map(|(&key, label)| {
                            (
                                key,
                                label
                                    .description
                                    .preceding
                                    .iter()
                                    .chain(&label.description.tail)
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
