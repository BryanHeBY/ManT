//! Checked logical-text and byte-range projection for surviving Fixed selections.
//!
//! These helpers borrow the final native surface; no second display body is stored.

use std::ops::Range;

use super::{FixedBody, OutputSlice, OwnerMark, TextJoin, TextSelection};

impl FixedBody {
    /// Check disjoint logical name ranges against final bold runs in one
    /// forward pass. A consumed separator has no glyph style and cannot be
    /// used as a name binding; no complete HEAD copy is made per name.
    pub(super) fn selection_ranges_bold(
        &self,
        selection: &TextSelection,
        form: &str,
        prefix: Range<usize>,
        names: &[Range<usize>],
    ) -> bool {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return false;
        }
        let mut ranges = Vec::with_capacity(names.len() + 1);
        ranges.push(prefix);
        ranges.extend_from_slice(names);
        let mut previous_end = 0;
        for range in &ranges {
            if range.start < previous_end
                || range.start >= range.end
                || form.get(range.clone()).is_none()
            {
                return false;
            }
            previous_end = range.end;
        }
        let mut cursor = 0usize;
        let mut target = 0usize;
        let mut covered = ranges[0].start;
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                let separator = match &selection.joins[index - 1] {
                    TextJoin::DirectContact => None,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        Some(text)
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return false,
                };
                if let Some(separator) = separator {
                    let Some(end) = cursor.checked_add(separator.len()) else {
                        return false;
                    };
                    if ranges
                        .get(target)
                        .is_some_and(|range| range.start < end && cursor < range.end)
                    {
                        return false;
                    }
                    cursor = end;
                }
            }
            let Some(start_byte) = usize::try_from(part.start_byte).ok() else {
                return false;
            };
            let Some(end_byte) = usize::try_from(part.end_byte).ok() else {
                return false;
            };
            let Some(run) = self.surface.runs.get((part.run.get() - 1) as usize) else {
                return false;
            };
            let Some(visible) = self
                .surface
                .run_text(part.run)
                .and_then(|text| text.get(start_byte..end_byte))
            else {
                return false;
            };
            let Some(end) = cursor.checked_add(visible.len()) else {
                return false;
            };
            while let Some(range) = ranges.get(target)
                && range.start < end
            {
                if range.end <= cursor {
                    return false;
                }
                let begin = range.start.max(cursor);
                let stop = range.end.min(end);
                if begin != covered || !run.label.style.bold {
                    return false;
                }
                covered = stop;
                if covered == range.end {
                    target += 1;
                    if let Some(next) = ranges.get(target) {
                        covered = next.start;
                    }
                } else {
                    break;
                }
            }
            cursor = end;
        }
        target == ranges.len() && cursor == form.len()
    }
    /// Project a checked final-display selection into logical text without
    /// copying the surface into a second stored body.
    #[must_use]
    pub fn selection_text(&self, selection: &TextSelection) -> Option<String> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return None;
        }
        let mut text = String::new();
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => text.push_str(separator),
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let run = self.surface.run_text(part.run)?;
            let start = usize::try_from(part.start_byte).ok()?;
            let end = usize::try_from(part.end_byte).ok()?;
            text.push_str(run.get(start..end)?);
        }
        Some(text)
    }

    /// Map a logical UTF-8 range back to surviving display slices. A range
    /// touching a consumed authored or generated separator has no final
    /// glyph to bind and is rejected; no byte or cell coordinate is inferred
    /// from layout.
    #[must_use]
    pub fn selection_subrange(
        &self,
        selection: &TextSelection,
        range: Range<usize>,
    ) -> Option<TextSelection> {
        let logical = self.selection_text(selection)?;
        if range.start >= range.end || logical.get(range.clone()).is_none() {
            return None;
        }
        let mut cursor = 0usize;
        let mut parts = Vec::new();
        let mut joins = Vec::new();
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => {
                        let end = cursor.checked_add(separator.len())?;
                        if range.start < end && cursor < range.end {
                            return None;
                        }
                        cursor = end;
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let length = usize::try_from(part.end_byte.checked_sub(part.start_byte)?).ok()?;
            let end = cursor.checked_add(length)?;
            let start_in_part = range.start.max(cursor);
            let end_in_part = range.end.min(end);
            if start_in_part < end_in_part {
                let start_byte = part
                    .start_byte
                    .checked_add(u64::try_from(start_in_part - cursor).ok()?)?;
                let end_byte = part
                    .start_byte
                    .checked_add(u64::try_from(end_in_part - cursor).ok()?)?;
                self.surface
                    .run_text(part.run)?
                    .get(usize::try_from(start_byte).ok()?..usize::try_from(end_byte).ok()?)?;
                if !parts.is_empty() {
                    joins.push(selection.joins[index - 1].clone());
                }
                parts.push(OutputSlice {
                    run: part.run,
                    start_byte,
                    end_byte,
                });
            }
            cursor = end;
        }
        (cursor == logical.len() && !parts.is_empty()).then_some(TextSelection { parts, joins })
    }

    /// Map disjoint declaration names against the already materialized HEAD.
    /// Unlike the public single-range helper, this private path walks the
    /// native selection once for all names. `form` is the unmodified result
    /// of `selection_text(selection)` in the same lexical proof operation.
    pub(super) fn selection_subranges_from_form(
        &self,
        selection: &TextSelection,
        form: &str,
        ranges: &[Range<usize>],
    ) -> Option<Vec<TextSelection>> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return None;
        }
        let mut previous_end = 0usize;
        for range in ranges {
            if range.start < previous_end
                || range.start >= range.end
                || form.get(range.clone()).is_none()
            {
                return None;
            }
            previous_end = range.end;
        }
        let mut found = ranges
            .iter()
            .map(|_| TextSelection {
                parts: Vec::new(),
                joins: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut current = 0usize;
        let mut cursor = 0usize;
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => {
                        let end = cursor.checked_add(separator.len())?;
                        if ranges
                            .get(current)
                            .is_some_and(|range| range.start < end && cursor < range.end)
                        {
                            return None;
                        }
                        cursor = end;
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let start_byte = usize::try_from(part.start_byte).ok()?;
            let end_byte = usize::try_from(part.end_byte).ok()?;
            let run_text = self.surface.run_text(part.run)?;
            let visible = run_text.get(start_byte..end_byte)?;
            let end = cursor.checked_add(visible.len())?;
            // The public mapper skips zero-width parts even in an otherwise
            // malformed mutable selection; never emit an empty output slice.
            if visible.is_empty() {
                continue;
            }
            while let Some(range) = ranges.get(current)
                && range.start < end
            {
                if range.end <= cursor {
                    return None;
                }
                let clip_start = range.start.max(cursor);
                let clip_end = range.end.min(end);
                let slice_start = start_byte.checked_add(clip_start - cursor)?;
                let slice_end = start_byte.checked_add(clip_end - cursor)?;
                run_text.get(slice_start..slice_end)?;
                let target = found.get_mut(current)?;
                if !target.parts.is_empty() {
                    target
                        .joins
                        .push(selection.joins.get(index.checked_sub(1)?)?.clone());
                }
                target.parts.push(OutputSlice {
                    run: part.run,
                    start_byte: u64::try_from(slice_start).ok()?,
                    end_byte: u64::try_from(slice_end).ok()?,
                });
                if range.end <= end {
                    current += 1;
                } else {
                    break;
                }
            }
            cursor = end;
        }
        (cursor == form.len()
            && current == ranges.len()
            && found.iter().all(|selection| !selection.parts.is_empty()))
        .then_some(found)
    }

    /// Read one complete surviving definition head without inferring bytes
    /// from neighboring rows, owners or unknown native joins.
    #[must_use]
    pub fn owner_complete_form(&self, owner: &OwnerMark) -> Option<String> {
        if !owner.has_complete_form() {
            return None;
        }
        let form = self.selection_text(&owner.head)?;
        (!form.trim().is_empty()).then_some(form)
    }
}
