//! Borrowed grapheme boundaries and terminal-cell slices for display labels.
//!
//! Callers supply already sanitized, single-line display text and retain their
//! own input/inspection budgets. These helpers neither escape controls nor
//! modify source identities, copyable addresses, or original text coordinates.
//! They account for each extended grapheme separately, matching cell-based
//! terminal placement; this is not necessarily a whole-string shaping width.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One complete extended grapheme borrowed from a display label.
///
/// Its byte range belongs to the input passed to [`graphemes`], not to a
/// reconstructed line or a Unicode-scalar coordinate space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellGrapheme<'text> {
    text: &'text str,
    bytes: Range<usize>,
    columns: usize,
}

impl<'text> CellGrapheme<'text> {
    /// Borrow the entire grapheme, including combining and joining characters.
    #[must_use]
    pub const fn text(&self) -> &'text str {
        self.text
    }

    /// Return its half-open UTF-8 byte range in the original input.
    #[must_use]
    pub fn bytes(&self) -> Range<usize> {
        self.bytes.clone()
    }

    /// Terminal columns charged for this grapheme independently of its neighbors.
    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }
}

/// Iterate complete extended graphemes with original byte ranges and cell widths.
///
/// The iterator borrows the input without collecting it or allocating styled
/// fragments. Consumers should retain continuous style runs instead of creating
/// a separately styled terminal span for every Unicode scalar.
///
/// An individual grapheme may contain many combining characters. Callers must
/// retain any existing byte/scalar inspection bounds before invoking this API;
/// a cell-width limit alone is not a bound on source work.
#[must_use]
pub fn graphemes(text: &str) -> impl DoubleEndedIterator<Item = CellGrapheme<'_>> {
    text.grapheme_indices(true)
        .map(|(start, grapheme)| CellGrapheme {
            text: grapheme,
            bytes: start..start + grapheme.len(),
            columns: grapheme.width(),
        })
}

/// Borrow the longest complete-grapheme prefix within a terminal-column budget.
///
/// A zero budget always returns an empty slice, including for all-zero-width
/// input. An overwide first grapheme is not partially returned or replaced;
/// substitution and wrapping policy belong to the caller.
#[must_use]
pub fn prefix_columns(text: &str, width: usize) -> &str {
    if width == 0 {
        return &text[..0];
    }
    let mut used = 0;
    let mut end = 0;
    for grapheme in graphemes(text) {
        if grapheme.columns() > width - used {
            break;
        }
        used += grapheme.columns();
        end = grapheme.bytes.end;
    }
    &text[..end]
}

/// Borrow the longest complete-grapheme suffix within a terminal-column budget.
///
/// Unlike [`prefix_columns`], a zero budget retains any trailing zero-width
/// graphemes. This preserves the existing suffix-trimming contract. An overwide
/// final grapheme yields an empty slice rather than a partial cluster.
#[must_use]
pub fn suffix_columns(text: &str, width: usize) -> &str {
    let mut used = 0;
    let mut start = text.len();
    for grapheme in graphemes(text).rev() {
        if grapheme.columns() > width - used {
            break;
        }
        used += grapheme.columns();
        start = grapheme.bytes.start;
    }
    &text[start..]
}

/// Borrow the remainder after a count of whole extended graphemes.
///
/// This offset is neither a byte count nor a terminal-column count. Out-of-range
/// offsets return an empty slice; no copied string or glyph index is built.
#[must_use]
pub fn after_graphemes(text: &str, offset: usize) -> &str {
    let start = text
        .grapheme_indices(true)
        .nth(offset)
        .map_or(text.len(), |(start, _)| start);
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::{after_graphemes, graphemes, prefix_columns, suffix_columns};

    #[test]
    fn graphemes_keep_borrowed_utf8_ranges_and_cluster_widths() {
        let text = "e\u{301}👩‍💻🇺🇳✈\u{fe0f}界";
        let expected = [
            ("e\u{301}", 0..3, 1),
            ("👩‍💻", 3..14, 2),
            ("🇺🇳", 14..22, 2),
            ("✈\u{fe0f}", 22..28, 2),
            ("界", 28..31, 2),
        ];
        let actual = graphemes(text).collect::<Vec<_>>();
        assert_eq!(actual.len(), expected.len());
        for (grapheme, (symbol, bytes, columns)) in actual.iter().zip(expected) {
            assert_eq!(grapheme.text(), symbol);
            assert_eq!(grapheme.bytes(), bytes);
            assert_eq!(grapheme.columns(), columns);
            assert!(std::ptr::eq(grapheme.text(), &raw const text[bytes]));
        }
        assert_eq!(
            graphemes(text).rev().map(|g| g.text()).collect::<Vec<_>>(),
            ["界", "✈\u{fe0f}", "🇺🇳", "👩‍💻", "e\u{301}"]
        );
    }

    #[test]
    fn column_slices_never_split_clusters_or_substitute_display_content() {
        for (text, width, prefix, suffix) in [
            ("", 0, "", ""),
            ("abc", 0, "", ""),
            ("abc", 1, "a", "c"),
            ("abc", usize::MAX, "abc", "abc"),
            ("👩‍💻x", 1, "", "x"),
            ("x👩‍💻", 1, "x", ""),
            ("e\u{301}x", 1, "e\u{301}", "x"),
            ("xe\u{301}", 1, "x", "e\u{301}"),
            ("e\u{301}👩‍💻Z", 3, "e\u{301}👩‍💻", "👩‍💻Z"),
            ("🇺🇳界", 2, "🇺🇳", "界"),
            ("✈\u{fe0f}x", 1, "", "x"),
            ("012345👩‍💻", 2, "01", "👩‍💻"),
        ] {
            let actual_prefix = prefix_columns(text, width);
            let actual_suffix = suffix_columns(text, width);
            assert_eq!(actual_prefix, prefix, "prefix {text:?}, width={width}");
            assert_eq!(actual_suffix, suffix, "suffix {text:?}, width={width}");
            assert!(std::ptr::eq(
                actual_prefix,
                &raw const text[..actual_prefix.len()]
            ));
            assert!(std::ptr::eq(
                actual_suffix,
                &raw const text[text.len() - actual_suffix.len()..]
            ));
        }
    }

    #[test]
    fn zero_width_graphemes_keep_distinct_prefix_and_suffix_contracts() {
        let zero = "\u{301}\u{200b}";
        assert_eq!(graphemes(zero).map(|g| g.columns()).sum::<usize>(), 0);
        assert_eq!(prefix_columns(zero, 0), "");
        assert_eq!(suffix_columns(zero, 0), zero);
        assert_eq!(prefix_columns(zero, 1), zero);
        assert_eq!(suffix_columns("a\u{200b}", 0), "\u{200b}");
        assert_eq!(prefix_columns("\u{301}ab", 1), "\u{301}a");
    }

    #[test]
    fn horizontal_offsets_count_graphemes_and_saturate_at_input_end() {
        let text = "12345678901e\u{301}👩‍💻TAIL";
        for (offset, expected) in [
            (0, text),
            (11, "e\u{301}👩‍💻TAIL"),
            (12, "👩‍💻TAIL"),
            (13, "TAIL"),
            (17, ""),
            (usize::MAX, ""),
        ] {
            let actual = after_graphemes(text, offset);
            assert_eq!(actual, expected);
            assert!(std::ptr::eq(
                actual,
                &raw const text[text.len() - actual.len()..]
            ));
        }
        assert_eq!(after_graphemes("", 0), "");
        assert_eq!(after_graphemes("", usize::MAX), "");
    }

    #[test]
    fn cell_placement_does_not_recompute_whole_string_ligature_widths() {
        // Lam and alef are separate extended graphemes. Their whole-string
        // shaping width must not replace the terminal's per-grapheme charges.
        let text = "لا";
        assert_eq!(
            graphemes(text)
                .map(|g| (g.text(), g.columns()))
                .collect::<Vec<_>>(),
            [("ل", 1), ("ا", 1)]
        );
        assert_eq!(prefix_columns(text, 1), "ل");
        assert_eq!(suffix_columns(text, 1), "ا");
    }
}
