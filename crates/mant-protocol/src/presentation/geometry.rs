//! Pure text geometry shared by document frontends.
//!
//! Distances here are resolved display cells, never byte/scalar match offsets,
//! source requests, terminal styling, or viewport rows. A parent origin is
//! composed before rendering a leaf, not applied to an already placed subtree.

use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

mod table;
pub use table::table_requires_origin_preserving_stack;

mod gaps;
pub use gaps::has_bounded_gap;

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
pub fn rebase_origin(relative: i32, old_parent: i32, new_parent: i32) -> i32 {
    let relative = i64::from(old_parent) + i64::from(relative) - i64::from(new_parent);
    i32::try_from(relative).unwrap_or(if relative < 0 { i32::MIN } else { i32::MAX })
}

/// Bounded final padding at a logical leaf, after all parent displacements.
/// This is not a viewport clipping policy and must not be applied recursively
/// to containers: a negative container may have positive child displacements.
#[must_use]
pub fn padding(origin: i32) -> usize {
    usize::try_from(origin.clamp(0, 4096)).unwrap_or(0)
}

/// Convert an in-memory cell count to a signed coordinate without wrapping.
#[must_use]
pub fn coordinate(cells: usize) -> i32 {
    i32::try_from(cells).unwrap_or(i32::MAX)
}

/// Space between a visible marker and its first paragraph, or `None` when
/// their composed origins require separate lines. Decide after final padding:
/// clipping a negative container is not the same as adding its child's delta.
#[must_use]
pub fn marker_run_in_gap(origin: i32, marker_width: usize, child_delta: i32) -> Option<usize> {
    if child_delta < 0 {
        return None;
    }
    let body = compose_origin(
        compose_origin(origin, coordinate(marker_width)),
        child_delta,
    );
    padding(body).checked_sub(padding(origin).saturating_add(marker_width))
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
    /// Append an independently owned, already resolved IR boundary fact.
    /// Unlike [`Self::request`], this does not deduplicate projections: source
    /// producers must give each request exactly one IR consumption point.
    pub fn append_resolved(&mut self, rows: u16) {
        let total = u32::from(self.explicit.unwrap_or(0)) + u32::from(rows);
        self.bounded |= total > u32::from(MAX_GAP_ROWS);
        self.explicit = Some(u16::try_from(total).unwrap_or(u16::MAX).min(MAX_GAP_ROWS));
    }
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

    /// Whether resolving this default would lose geometry, including a
    /// bounded inherited gap when no explicit request was supplied.
    #[must_use]
    pub fn resolution_is_bounded(&self, default: u16) -> bool {
        self.bounded || (self.explicit.is_none() && default > MAX_GAP_ROWS)
    }

    /// Whether row or request-count limits prevented complete geometry.
    #[must_use]
    pub const fn is_bounded(&self) -> bool {
        self.bounded
    }
}

/// Resolved rows owned by a block's leading boundary. Zero is an explicit
/// tight boundary, not an instruction for a renderer to invent paragraph
/// spacing. Source and Markdown producers resolve their defaults before IR.
#[must_use]
pub const fn block_gap(block: &mant_ir::Block) -> u16 {
    use mant_ir::Block;
    match block {
        Block::Paragraph { layout, .. }
        | Block::Preformatted { layout, .. }
        | Block::List { layout, .. }
        | Block::DefinitionList { layout, .. }
        | Block::Table { layout, .. }
        | Block::Equation { layout, .. }
        | Block::Unsupported { layout, .. } => layout.spacing_before_lines,
        Block::VerticalSpace { lines, .. } => *lines,
        Block::ThematicBreak { .. } => 0,
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
        assert_eq!(rebase_origin(1, i32::MAX, i32::MAX), 1);
        assert_eq!(rebase_origin(-1, i32::MIN, i32::MIN), -1);
        assert_eq!(marker_run_in_gap(-5, 2, 4), None);
        assert_eq!(marker_run_in_gap(-5, 2, 10), Some(5));
        assert_eq!(marker_run_in_gap(5, 2, -1), None);
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
        assert!(gap.resolution_is_bounded(u16::MAX));
        assert!(!gap.resolution_is_bounded(1));
        assert_eq!(gap.rows(1), 1);
        gap.request(0, 0).unwrap();
        assert!(!gap.resolution_is_bounded(u16::MAX));
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
