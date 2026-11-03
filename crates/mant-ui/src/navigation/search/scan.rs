//! Bounded, resumable matching over complete logical outline fields.
use std::{ops::Range, sync::Arc};

use super::{FieldHit, OutlineHit};
use crate::document::OutlineRecord;

pub(super) const MAX_RESULTS: usize = 4096;
pub(super) const MAX_RANGES: usize = 32768;

/// A bounded, resumable scan over complete outline fields. User folds and
/// display truncation never become inputs to matching.
#[derive(Debug)]
pub(crate) struct OutlineScan {
    record: usize,
    field: usize,
    stream: crate::document::PatternStream,
    ranges: Vec<Range<usize>>,
    fields: Vec<FieldHit>,
    remaining: usize,
}

impl OutlineScan {
    pub(crate) fn with_stream(stream: crate::document::PatternStream) -> Self {
        Self {
            record: 0,
            field: 0,
            stream,
            ranges: Vec::new(),
            fields: Vec::new(),
            remaining: MAX_RANGES,
        }
    }

    /// Returns whether the scan is finished and whether its results are limited.
    pub(crate) fn advance(
        &mut self,
        records: &[OutlineRecord],
        matches: &mut Vec<OutlineHit>,
        mut budget: usize,
    ) -> (bool, bool) {
        while budget > 0 && self.record < records.len() {
            let record = &records[self.record];
            if self.field == record.fields.len() {
                if !self.fields.is_empty() {
                    matches.push(OutlineHit {
                        node_index: record.node_index,
                        label: Arc::clone(&record.label),
                        fields: std::mem::take(&mut self.fields),
                    });
                }
                self.record += 1;
                self.field = 0;
                budget = budget.saturating_sub(1);
                continue;
            }
            let field = &record.fields[self.field];
            let ranges = self.stream.advance(&field.text, &mut budget);
            for range in ranges {
                if matches.len() == MAX_RESULTS || self.remaining == 0 {
                    if !self.ranges.is_empty() {
                        self.fields.push(FieldHit {
                            field: field.clone(),
                            ranges: std::mem::take(&mut self.ranges),
                        });
                    }
                    if !self.fields.is_empty() {
                        matches.push(OutlineHit {
                            node_index: record.node_index,
                            label: Arc::clone(&record.label),
                            fields: std::mem::take(&mut self.fields),
                        });
                    }
                    return (true, true);
                }
                self.remaining -= 1;
                self.ranges.push(range);
            }
            if self.stream.is_done() {
                if !self.ranges.is_empty() {
                    self.fields.push(FieldHit {
                        field: field.clone(),
                        ranges: std::mem::take(&mut self.ranges),
                    });
                }
                self.next_field();
                budget = budget.saturating_sub(1);
            }
        }
        (self.record == records.len(), false)
    }

    fn next_field(&mut self) {
        self.field += 1;
        self.stream.reset();
    }
}

#[cfg(test)]
pub(crate) fn find(records: &[OutlineRecord], query: &str) -> (Vec<OutlineHit>, bool) {
    let search = crate::document::LiteralSearch::new(query);
    let mut matches = Vec::new();
    let mut remaining = MAX_RANGES;
    let mut limited = false;
    for record in records {
        let mut fields = Vec::new();
        for field in &record.fields {
            let mut ranges = search.ranges(&field.text, remaining.saturating_add(1));
            if ranges.is_empty() {
                continue;
            }
            if matches.len() == MAX_RESULTS {
                limited = true;
                break;
            }
            if ranges.len() > remaining {
                ranges.truncate(remaining);
                limited = true;
            }
            remaining -= ranges.len();
            if !ranges.is_empty() {
                fields.push(FieldHit {
                    field: field.clone(),
                    ranges,
                });
            }
            if limited {
                break;
            }
        }
        if !fields.is_empty() {
            matches.push(OutlineHit {
                node_index: record.node_index,
                label: Arc::clone(&record.label),
                fields,
            });
        }
        if limited {
            break;
        }
    }
    (matches, limited)
}
