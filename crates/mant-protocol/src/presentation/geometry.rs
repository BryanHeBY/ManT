//! Pure text geometry shared by document frontends.
//!
//! Distances here are resolved display cells, never byte/scalar match offsets,
//! source requests, terminal styling, or viewport rows. A parent origin is
//! composed before rendering a leaf, not applied to an already placed subtree.

use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

/// Maximum explicit gap in one logical block boundary. The producer can report
/// saturation through [`GapPlan::is_bounded`] without allocating blank rows.
pub const MAX_GAP_ROWS: u16 = 4096;

/// Display-cell width of the widest original logical line.
///
/// Styling and zero-width anchors must be removed by the inline projection,
/// not measured as escape sequences. Hard line breaks never count as cells.
#[must_use]
pub fn text_width(text: &str) -> usize {
    text.split('\n')
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
}

/// Compose a relative delta exactly once. Negative coordinates remain signed
/// until the final viewport/padding boundary; early clamping loses leftward
/// child offsets. Saturation only protects arithmetic on untrusted callers.
#[must_use]
pub const fn compose_origin(parent: i32, relative: i32) -> i32 {
    parent.saturating_add(relative)
}

/// Reparent one already-relative root while preserving its source position.
/// Descendants keep their own relative deltas and must not be rebased again.
#[must_use]
pub const fn rebase_origin(relative: i32, old_parent: i32, new_parent: i32) -> i32 {
    compose_origin(old_parent, relative).saturating_sub(new_parent)
}

/// A contradictory projection of the same source gap. Distinct requests must
/// have distinct operation-local identities, even at the same source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictingGap;

/// Resolve one boundary's independent requests, shared projections and default.
///
/// Identities live only for the source/projection operation; they are not a
/// protocol identity or an assertion that equal row counts are the same event.
/// Independent requests add, including repeated requests of equal magnitude.
/// One explicit zero suppresses a default. Once saturated, the result remains
/// bounded and no further identities are retained.
#[derive(Debug, Default)]
pub struct GapPlan {
    requests: BTreeMap<u64, u16>,
    explicit: Option<u16>,
    bounded: bool,
}

impl GapPlan {
    /// Contribute a source request or another projection of the same request.
    ///
    /// # Errors
    ///
    /// Returns [`ConflictingGap`] if an already retained identity has a
    /// different row count. A saturated boundary no longer validates identity
    /// conflicts: its `is_bounded` flag already discloses incomplete geometry.
    pub fn request(&mut self, identity: u64, rows: u16) -> Result<(), ConflictingGap> {
        if self.bounded {
            return Ok(());
        }
        if let Some(&previous) = self.requests.get(&identity) {
            return if previous == rows {
                Ok(())
            } else {
                Err(ConflictingGap)
            };
        }
        let total = u32::from(self.explicit.unwrap_or(0)) + u32::from(rows);
        self.bounded = total > u32::from(MAX_GAP_ROWS);
        self.explicit = Some(u16::try_from(total).unwrap_or(u16::MAX).min(MAX_GAP_ROWS));
        // A zero has no additive payload. Limit retained identities separately
        // so adversarial zero requests cannot make a boundary unbounded.
        if self.requests.len() < usize::from(MAX_GAP_ROWS) && !self.bounded {
            self.requests.insert(identity, rows);
        } else {
            self.bounded = true;
        }
        Ok(())
    }

    /// Resolve a default only when the source supplied no explicit request.
    #[must_use]
    pub fn rows(&self, default: u16) -> u16 {
        self.explicit.unwrap_or(default).min(MAX_GAP_ROWS)
    }

    /// Whether row or request-count limits prevented complete geometry.
    #[must_use]
    pub const fn is_bounded(&self) -> bool {
        self.bounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_width_is_not_bytes_or_scalars_and_respects_hard_lines() {
        for (value, width) in [
            ("", 0),
            ("日本日本", 8),
            ("e\u{301}", 1),
            ("😀", 2),
            ("ab\nc", 2),
            ("--a\n--long", 6),
        ] {
            assert_eq!(text_width(value), width, "{value}");
        }
    }

    #[test]
    fn translation_and_reparenting_preserve_leaf_positions() {
        for shift in [0, 2, 5] {
            for (parent, child) in [(0, 4), (10, -5), (4, 8)] {
                assert_eq!(
                    compose_origin(parent + shift, child),
                    compose_origin(parent, child) + shift
                );
                let moved = rebase_origin(child, parent, 20);
                assert_eq!(compose_origin(20, moved), compose_origin(parent, child));
                assert_eq!(
                    compose_origin(20, compose_origin(moved, 3)),
                    compose_origin(parent, compose_origin(child, 3))
                );
            }
        }
        assert_eq!(compose_origin(i32::MAX, 1), i32::MAX);
        assert_eq!(compose_origin(i32::MIN, -1), i32::MIN);
    }

    #[test]
    fn independent_gaps_add_shared_facts_do_not_and_zero_overrides_default() {
        for (a, b, total) in [(1, 2, 3), (0, 2, 2), (1, 1, 2)] {
            let mut gap = GapPlan::default();
            gap.request(10, a).unwrap();
            gap.request(10, a).unwrap();
            gap.request(11, b).unwrap();
            assert_eq!(gap.rows(9), total);
            assert!(!gap.is_bounded());
        }
        let mut gap = GapPlan::default();
        assert_eq!(gap.rows(1), 1);
        gap.request(0, 0).unwrap();
        assert_eq!(gap.rows(1), 0);
        assert_eq!(gap.request(0, 1), Err(ConflictingGap));
        gap.request(1, u16::MAX).unwrap();
        assert_eq!(gap.rows(1), MAX_GAP_ROWS);
        assert!(gap.is_bounded());
        for identity in 2..10_000 {
            gap.request(identity, 1).unwrap();
        }
        assert!(gap.requests.len() <= usize::from(MAX_GAP_ROWS));
    }
}
