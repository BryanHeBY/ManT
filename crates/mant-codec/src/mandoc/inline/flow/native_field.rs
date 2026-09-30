//! Upstream pad/break flags and the one shared row-close rule from the
//! pinned CVS `term.c`. Every definition list kind configures one
//! [`FieldFlags`] at HEAD entry, exactly as `mdoc_term.c::termp_it_pre()`
//! does (mdoc_term.c:795-860); the flags plus `row_continues` are the
//! single source of head-row decisions, replacing per-kind arithmetic.

/// One `TERMP_*` pad/break control that `termp_it_pre()` can set on a
/// definition HEAD field. The meaning of each is documented in
/// `term_flushln()` (term.c:96-253).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum FieldFlag {
    /// `TERMP_NOBREAK`: the next field follows on the same line unless the
    /// field overran (term.c:250-252).
    NoBreak,
    /// `TERMP_HANG`: the next field always follows on the same line.
    Hang,
    /// `TERMP_BRTRSP`: trailing whitespace counts toward the field width
    /// when deciding whether the field fits (term.c:186-196).
    BrTrsp,
    /// `TERMP_BRIND`: an automatic line break restarts at the right margin
    /// instead of the offset (term.c:229-232).
    Brind,
}

impl FieldFlag {
    const fn bit(self) -> u8 {
        1 << (match self {
            Self::NoBreak => 0,
            Self::Hang => 1,
            Self::BrTrsp => 2,
            Self::Brind => 3,
        })
    }
}

/// The set of [`FieldFlag`]s active on one definition HEAD field, plus the
/// field's significant trailing whitespace. The byte is private storage;
/// every read goes through [`FieldFlags::contains`] or a named predicate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::mandoc) struct FieldFlags {
    bits: u8,
    /// `p->trailspace`: significant trailing whitespace kept for the next
    /// field; restored into `minbl` by `term_flushln()` (term.c:236).
    trailspace: u8,
}

impl FieldFlags {
    /// `mdoc_term.c::termp_it_pre()`: `LIST_hang` (mdoc_term.c:800-804) —
    /// NoBreak|Brind|Hang, trailspace 1.
    pub(in crate::mandoc) const fn hang() -> Self {
        Self::new(&[FieldFlag::NoBreak, FieldFlag::Brind, FieldFlag::Hang], 1)
    }

    /// `mdoc_term.c::termp_it_pre()`: `LIST_tag` (mdoc_term.c:805-814) —
    /// NoBreak|BrTrsp|Brind, trailspace 2, plus Hang exactly when the item
    /// has no BODY child.
    pub(in crate::mandoc) const fn tag(body_empty: bool) -> Self {
        let mut flags = Self::new(
            &[FieldFlag::NoBreak, FieldFlag::BrTrsp, FieldFlag::Brind],
            2,
        );
        if body_empty {
            flags = flags.with(FieldFlag::Hang);
        }
        flags
    }

    /// `LIST_column` BODY pre (mdoc_term.c:817-824): only non-last
    /// fields keep NOBREAK, with trailspace one; no BRIND or HANG.
    pub(in crate::mandoc) const fn column(last: bool) -> Self {
        if last {
            Self::new(&[], 0)
        } else {
            Self::new(&[FieldFlag::NoBreak], 1)
        }
    }

    /// `mdoc_term.c::termp_it_pre()`: `LIST_diag` (mdoc_term.c:827-831) —
    /// NoBreak|Brind without Hang, trailspace 1.
    pub(in crate::mandoc) const fn diag() -> Self {
        Self::new(&[FieldFlag::NoBreak, FieldFlag::Brind], 1)
    }

    /// `mdoc_term.c::termp_it_pre()`: `LIST_inset` (the `default` arm) sets
    /// no pad/break flags at all; the body separator is an ordinary
    /// buffered word.
    pub(in crate::mandoc) const fn inset() -> Self {
        Self::new(&[], 0)
    }

    const fn new(flags: &[FieldFlag], trailspace: u8) -> Self {
        let mut bits = 0;
        let mut index = 0;
        while index < flags.len() {
            bits |= flags[index].bit();
            index += 1;
        }
        Self { bits, trailspace }
    }

    const fn with(self, flag: FieldFlag) -> Self {
        Self {
            bits: self.bits | flag.bit(),
            trailspace: self.trailspace,
        }
    }

    pub(in crate::mandoc) const fn contains(self, flag: FieldFlag) -> bool {
        self.bits & flag.bit() != 0
    }

    pub(in crate::mandoc) const fn without(self, flag: FieldFlag) -> Self {
        Self {
            bits: self.bits & !flag.bit(),
            trailspace: self.trailspace,
        }
    }

    /// An It pre ORs its pad/break bits into the live formatter flags, but
    /// can replace trailspace independently (mdoc_term.c:789-831). Nested
    /// list HEADs must retain the surrounding field's other active bits.
    pub(in crate::mandoc) const fn combine(self, added: Self, trailspace: u8) -> Self {
        Self {
            bits: self.bits | added.bits,
            trailspace,
        }
    }

    /// `TERMP_NOBREAK` without `TERMP_HANG`: the wrappable-field view.
    pub(in crate::mandoc) const fn wraps(self) -> bool {
        self.contains(FieldFlag::NoBreak) && !self.contains(FieldFlag::Hang)
    }

    pub(in crate::mandoc) const fn trailspace(self) -> usize {
        self.trailspace as usize
    }
}

/// The `term_flushln()` tail rule (term.c:236-253): after the buffer loop,
/// the next field follows on the same line exactly when Hang is set, or
/// `NoBreak` is set and the printed field did not overrun. The comparison
/// carries the half-EN tolerance (`term_len(p, 1) / 2`, term.c:251) — half
/// a column at 24 basic units per cell — kept in the integer form
/// `2*vbr + 2*trailspace > 2*vfield + 1`.
pub(in crate::mandoc) fn row_continues(flags: FieldFlags, vbr: usize, vfield: usize) -> bool {
    flags.contains(FieldFlag::Hang)
        || flags.contains(FieldFlag::NoBreak) && 2 * vbr + 2 * flags.trailspace() <= 2 * vfield + 1
}

#[cfg(test)]
mod term_contract_tests {
    use super::*;

    #[test]
    fn overrun_closes_nobreak_rows_and_hang_never_closes() {
        let tag = FieldFlags::tag(false);
        let hang = FieldFlags::hang();
        let inset = FieldFlags::inset();
        // term.c:250-252: vbr + trailspace > vfield closes a NOBREAK row.
        assert!(!row_continues(tag, 4, 4)); // 4+2 > 4
        assert!(row_continues(tag, 2, 4)); // 2+2 <= 4
        assert!(row_continues(hang, 99, 4)); // Hang ignores overrun
        assert!(!row_continues(inset, 0, 4)); // no flags: always closes
    }

    #[test]
    fn kind_constructors_match_termp_it_pre() {
        // mdoc_term.c:800-814, 827-831, and the default arm.
        let hang = FieldFlags::hang();
        assert!(hang.contains(FieldFlag::NoBreak) && hang.contains(FieldFlag::Hang));
        let tag = FieldFlags::tag(false);
        assert!(
            tag.contains(FieldFlag::NoBreak)
                && tag.contains(FieldFlag::BrTrsp)
                && tag.contains(FieldFlag::Brind)
                && !tag.contains(FieldFlag::Hang)
        );
        assert!(FieldFlags::tag(true).contains(FieldFlag::Hang));
        let diag = FieldFlags::diag();
        assert!(diag.contains(FieldFlag::NoBreak) && diag.contains(FieldFlag::Brind));
        assert!(!diag.contains(FieldFlag::Hang) && !diag.contains(FieldFlag::BrTrsp));
        assert_eq!(FieldFlags::inset(), FieldFlags::default());
    }
}
