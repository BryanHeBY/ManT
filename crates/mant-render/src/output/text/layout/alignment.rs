//! Sparse prepared-row alignment; final table slots consume these receipts.
use super::LayoutText;
use mant_ir::{TextAlignment, geometry::text_width};

#[derive(Clone, Copy, Debug)]
pub(super) struct RowAlignment {
    pub(super) row: usize,
    pub(super) prefix: usize,
    pub(super) placement: TextAlignment,
}

impl LayoutText {
    pub(in crate::output::text) fn defer_alignment(
        mut self,
        placement: TextAlignment,
        prefix: usize,
    ) -> Self {
        if placement != TextAlignment::Left && !self.is_empty() {
            self.alignments.push(RowAlignment {
                row: 0,
                prefix,
                placement,
            });
        }
        self
    }

    pub(super) fn prefix_alignments(&mut self, prefix: &str, remove: bool) {
        let rows = prefix.bytes().filter(|byte| *byte == b'\n').count();
        let width = text_width(prefix.rsplit('\n').next().unwrap_or_default());
        self.alignments.retain_mut(|receipt| {
            if remove {
                if receipt.row < rows {
                    return false;
                }
                if receipt.row == rows {
                    receipt.prefix = receipt.prefix.saturating_sub(width);
                }
                receipt.row -= rows;
            } else {
                if receipt.row == 0 {
                    receipt.prefix = receipt.prefix.saturating_add(width);
                }
                receipt.row = receipt.row.saturating_add(rows);
            }
            true
        });
    }

    pub(in crate::output::text) fn position_alignments(self, width: Option<usize>) -> Self {
        if self.alignments.is_empty() {
            return self;
        }
        let Some(width) = width else {
            // A nested table may still be inside intrinsic preparation.
            // Unknown local width is not proof that its final receiving
            // slot is unknown; retain the receipt until actual placement.
            // Final unbounded output simply uses the unchanged left text.
            return self;
        };
        let mut rows = self.split(false);
        for row in &mut rows {
            let receipt = row.alignments.first().copied();
            row.alignments.clear();
            let Some(receipt) = receipt else { continue };
            let used = text_width(&row.visible).saturating_sub(receipt.prefix);
            let padding = receipt
                .placement
                .padding(width.saturating_sub(receipt.prefix), used);
            if padding == 0 {
                continue;
            }
            let mut column = 0;
            let at = crate::cells::graphemes(&row.visible)
                .find_map(|glyph| {
                    if column >= receipt.prefix {
                        Some(glyph.bytes().start)
                    } else {
                        column += glyph.columns();
                        None
                    }
                })
                .unwrap_or(row.visible.len());
            let prefix = &row.visible[..at];
            // The receipt's prefix consists of generated origins/markers,
            // written outside source decoration. No authored text is reparsed.
            if row.rendered.starts_with(prefix) {
                let spaces = " ".repeat(padding);
                row.rendered.insert_str(at, &spaces);
                row.visible.insert_str(at, &spaces);
            }
        }
        Self::join(rows, "\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_preparation_width_keeps_the_later_slot_receipt() {
        let prepared = LayoutText::decorated("X", "\x1b[1mX\x1b[0m".into())
            .defer_alignment(TextAlignment::Center, 0)
            .position_alignments(None);
        assert_eq!(prepared.visible, "X", "intrinsic text stays unpadded");
        let positioned = prepared.position_alignments(Some(6));
        assert_eq!(positioned.visible, "  X");
        assert_eq!(positioned.rendered, "  \x1b[1mX\x1b[0m");
        assert!(
            positioned.alignments.is_empty(),
            "the final slot consumes it once"
        );
    }
}
