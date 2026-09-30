// Copyright (c) 2017, 2021, 2025 Ingo Schwarze <schwarze@openbsd.org>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! The pinned CVS `term_tab.c` stop lists without its expanding lookup cache.
//!
//! Positions stay in character-device basic units (24 units per EN). The
//! absolute list is followed by repetitions of the periodic list. Lookup
//! computes the necessary repetition directly, so a distant tab does not
//! allocate or visit all earlier repetitions.

use crate::mandoc::layout::Distance;

const UNITS_PER_CELL: usize = 24;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct StopList {
    /// Only a new prefix maximum can be the first stop greater than a
    /// lookup position. Keeping these records preserves input order even
    /// for non-increasing lists; sorting the supplied stops would not.
    records: Vec<usize>,
    /// The previous supplied position, including a non-increasing one.
    /// `+` arguments and the periodic origin use this, not the maximum.
    last: usize,
}

impl StopList {
    fn push(&mut self, position: usize) {
        self.last = position;
        if self.records.last().is_none_or(|last| position > *last) {
            self.records.push(position);
        }
    }

    fn next(&self, previous: usize) -> Option<usize> {
        self.records
            .get(self.records.partition_point(|stop| *stop <= previous))
            .copied()
    }
}

/// Native absolute and periodic tab positions. The lists have at most one
/// record per supplied operand; generated periodic positions are never kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mandoc) struct TabStops {
    absolute: StopList,
    periodic: StopList,
}

impl Default for TabStops {
    fn default() -> Self {
        // mdoc_term.c:257-259 and man_term.c:160-162 install T .5i,
        // which ascii_hspan() converts to 120 basic units.
        Self::periodic_cells(5)
    }
}

impl TabStops {
    /// `termp_bd_pre()` installs T 8n for a literal BODY
    /// (mdoc_term.c:1460-1462). Its post handler does not restore tabs.
    pub(in crate::mandoc) fn literal() -> Self {
        Self::periodic_cells(8)
    }

    fn periodic_cells(cells: usize) -> Self {
        let mut periodic = StopList::default();
        periodic.push(cells * UNITS_PER_CELL);
        Self {
            absolute: StopList::default(),
            periodic,
        }
    }

    /// Execute a fresh `.ta` argument list (`roff_term.c:219-221`), using
    /// the T selection and + addition of `term_tab_set()` (term_tab.c:45-93).
    /// Invalid or out-of-bound literal distances are skipped as they are
    /// by the shared distance parser. Negative tab destinations are omitted.
    pub(in crate::mandoc) fn from_arguments<'a>(arguments: impl Iterator<Item = &'a str>) -> Self {
        let mut stops = Self {
            absolute: StopList::default(),
            periodic: StopList::default(),
        };
        let mut periodic = false;
        for argument in arguments {
            if argument == "T" {
                periodic = true;
                continue;
            }
            let (relative, argument) = argument
                .strip_prefix('+')
                .map_or((false, argument), |argument| (true, argument));
            let Some(position) = Distance::parse(argument)
                .and_then(|distance| usize::try_from(distance.basic_units()).ok())
            else {
                continue;
            };
            let list = if periodic {
                &mut stops.periodic
            } else {
                &mut stops.absolute
            };
            let position = if relative {
                list.last.saturating_add(position)
            } else {
                position
            };
            list.push(position);
        }
        stops
    }

    /// The first supplied stop greater than `previous`, then the first
    /// such stop in the earliest periodic repetition (`term_tab.c:96-125`).
    /// Empty and non-advancing periods return the current position instead
    /// of entering the native implementation's infinite extension loop.
    pub(in crate::mandoc) fn next_stop(&self, previous: usize) -> usize {
        if let Some(stop) = self.absolute.next(previous) {
            return stop;
        }
        let Some(maximum) = self.periodic.records.last().copied() else {
            return previous;
        };
        let relative = previous.saturating_sub(self.absolute.last);
        let repetitions = if relative < maximum {
            0
        } else if let Some(repetitions) = (relative - maximum).checked_div(self.periodic.last) {
            repetitions + 1
        } else {
            return previous;
        };
        // The earliest usable repetition always begins at or before the
        // lookup position: last <= maximum. Checked arithmetic also makes
        // the upper bound explicit if a caller supplies usize::MAX.
        let Some(base) = repetitions
            .checked_mul(self.periodic.last)
            .and_then(|delta| self.absolute.last.checked_add(delta))
        else {
            return usize::MAX;
        };
        self.periodic
            .next(previous.saturating_sub(base))
            .map_or(previous, |stop| base.saturating_add(stop))
    }
}

#[cfg(test)]
mod tests {
    use super::{TabStops, UNITS_PER_CELL};

    fn positions(arguments: &str, columns: &[usize]) -> Vec<usize> {
        let stops = TabStops::from_arguments(arguments.split_whitespace());
        columns
            .iter()
            .map(|column| stops.next_stop(column * UNITS_PER_CELL) / UNITS_PER_CELL)
            .collect()
    }

    #[test]
    fn absolute_relative_and_periodic_operands_follow_native_order() {
        // Before writing these assertions, pristine CVS -Tascii ran the
        // exact `.TH TAB 1 / .SH DESCRIPTION / .nf / .ta <arguments> /
        // A<TAB>B<TAB>C<TAB>D<TAB>E<TAB>F / .fi` input. Its body was:
        // 2n: "A BCDEF"; T 3n: "A  B  C  D  E  F";
        // 2n +3n: "A B  CDEF"; 2n 4n T 3n +2n: "A B C  D E  F".
        // term_tab.c:76-93 adds + to the selected list's last position;
        // 96-125 starts its periodic repetitions after the absolute list.
        assert_eq!(positions("2n", &[1, 3, 4]), [2, 3, 4]);
        assert_eq!(positions("T 3n", &[1, 4, 7]), [3, 6, 9]);
        assert_eq!(positions("2n +3n", &[1, 3, 6]), [2, 5, 6]);
        assert_eq!(
            positions("2n 4n T 3n +2n", &[1, 3, 5, 8, 10]),
            [2, 4, 7, 9, 12]
        );
    }

    #[test]
    fn non_increasing_stops_keep_first_match_and_raw_period_origin() {
        // The same exact oracle input with 4n 2n produced "A   BCDEF";
        // T 4n 2n produced "A   B C D E F"; 4n 2n T 3n produced
        // "A   B   C  D  E  F". term_tab_next() chooses the
        // first greater stop in source order, not the numeric minimum.
        assert_eq!(positions("4n 2n", &[1, 5]), [4, 5]);
        assert_eq!(positions("T 4n 2n", &[1, 5, 7, 9]), [4, 6, 8, 10]);
        assert_eq!(positions("4n 2n T 3n", &[1, 5, 6]), [4, 8, 8]);
    }

    #[test]
    fn native_default_and_literal_periods_are_distinct() {
        // The pristine reference's default no-fill A<TAB>B<TAB>C row
        // was "A    B    C"; the mdoc literal display's corresponding
        // row was "A       B       C". Its 8n period is established by
        // mdoc_term.c:1460-1462, without an .Ed restore.
        assert_eq!(TabStops::default().next_stop(24), 120);
        assert_eq!(TabStops::literal().next_stop(24), 192);
    }

    #[test]
    fn finite_lookup_does_not_expand_periodic_history() {
        let stops = TabStops::from_arguments("T 3n +2n".split_whitespace());
        assert_eq!(stops.next_stop(1_000_000 * 120), 1_000_000 * 120 + 72);
        assert_eq!(stops.absolute.records.len(), 0);
        assert_eq!(stops.periodic.records.len(), 2);
        assert_eq!(stops.next_stop(usize::MAX), usize::MAX);
    }

    #[test]
    fn empty_zero_and_unsupported_stops_cannot_loop_or_overflow() {
        // Exact pristine `.ta 0n` input produced "ABCDEF"; a zero T
        // period instead hung and was terminated by the probe timeout.
        // Its bounded handling here is an algorithm guard, not a claim
        // that the native renderer successfully accepts that period.
        assert_eq!(positions("", &[0, 1, 100]), [0, 1, 100]);
        assert_eq!(positions("0n", &[0, 1]), [0, 1]);
        assert_eq!(positions("T 0n", &[0, 1]), [0, 1]);
        assert_eq!(positions("T 4n 0n", &[1, 5]), [4, 5]);
        assert_eq!(positions("invalid -2n 65535n", &[0, 1]), [0, 1]);
    }
}
