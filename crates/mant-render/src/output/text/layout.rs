//! One text layout result, measured independently of zero-width decoration.

use super::super::table::TableText;
mod alignment;
use alignment::RowAlignment;

/// Both representations come from the same inline visit and share all layout
/// operations. The visible side is local measurement material, not a second
/// renderer or a copy of the document model. Keeping complete rows also lets
/// Unicode width account for combining sequences crossing style boundaries.
#[derive(Clone, Debug, Default)]
pub(in crate::output) struct LayoutText {
    pub(super) rendered: String,
    pub(super) visible: String,
    alignments: Vec<RowAlignment>,
    row_breaks: usize,
}

impl LayoutText {
    pub(super) fn decorated(visible: &str, rendered: String) -> Self {
        // The public decorator contract preserves newlines. An invalid
        // callback must not make the row zipper silently discard source text.
        let row_breaks = visible.bytes().filter(|byte| *byte == b'\n').count();
        let rendered = if row_breaks == rendered.bytes().filter(|byte| *byte == b'\n').count() {
            rendered
        } else {
            visible.to_owned()
        };
        Self {
            rendered,
            visible: visible.to_owned(),
            alignments: Vec::new(),
            row_breaks,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    pub(super) fn append(&mut self, other: &Self) {
        let row = self.row_breaks;
        self.alignments
            .extend(other.alignments.iter().map(|receipt| RowAlignment {
                row: row.saturating_add(receipt.row),
                ..*receipt
            }));
        self.rendered.push_str(&other.rendered);
        self.visible.push_str(&other.visible);
        self.row_breaks = self.row_breaks.saturating_add(other.row_breaks);
    }

    pub(super) fn push_plain(&mut self, text: &str) {
        self.rendered.push_str(text);
        self.visible.push_str(text);
        self.row_breaks = self
            .row_breaks
            .saturating_add(text.bytes().filter(|byte| *byte == b'\n').count());
    }

    pub(super) fn prefixed(mut self, prefix: &str) -> Self {
        self.prefix_alignments(prefix, false);
        self.rendered.insert_str(0, prefix);
        self.visible.insert_str(0, prefix);
        self.row_breaks = self
            .row_breaks
            .saturating_add(prefix.bytes().filter(|byte| *byte == b'\n').count());
        self
    }

    pub(super) fn strip_prefix(mut self, prefix: &str) -> Self {
        if self.visible.starts_with(prefix) && self.rendered.starts_with(prefix) {
            self.prefix_alignments(prefix, true);
            self.visible.drain(..prefix.len());
            self.row_breaks = self
                .row_breaks
                .saturating_sub(prefix.bytes().filter(|byte| *byte == b'\n').count());
            self.rendered.drain(..prefix.len());
        }
        self
    }

    /// Empty logical rows do not gain indentation from ANSI bytes on that row.
    pub(super) fn indented(self, columns: usize) -> Self {
        if columns == 0 {
            return self;
        }
        let prefix = " ".repeat(columns);
        Self::join(
            self.split(false).into_iter().map(|row| {
                if row.is_empty() {
                    row
                } else {
                    row.prefixed(&prefix)
                }
            }),
            "\n",
        )
    }

    pub(super) fn split(&self, completed: bool) -> Vec<Self> {
        let mut rows = self
            .visible
            .split('\n')
            .zip(self.rendered.split('\n'))
            .map(|(visible, rendered)| Self::decorated(visible, rendered.to_owned()))
            .collect::<Vec<_>>();
        for receipt in &self.alignments {
            if let Some(row) = rows.get_mut(receipt.row) {
                row.alignments.push(RowAlignment { row: 0, ..*receipt });
            }
        }
        if completed && !self.visible.is_empty() && self.visible.ends_with('\n') {
            // Removing a completed row's delimiter does not remove decoration
            // after it (for example an ANSI reset following the final newline).
            let tail = rows.pop().expect("trailing delimiter row");
            rows.last_mut().expect("preceding row").append(&tail);
        }
        rows
    }

    pub(super) fn join(values: impl IntoIterator<Item = Self>, separator: &str) -> Self {
        let mut result = Self::default();
        for (index, value) in values.into_iter().enumerate() {
            if index > 0 {
                result.push_plain(separator);
            }
            result.append(&value);
        }
        result
    }

    pub(super) fn trim_newlines(&self) -> Self {
        let rows = self.split(false);
        let first = rows
            .iter()
            .position(|row| !row.is_empty())
            .unwrap_or(rows.len());
        let last = rows
            .iter()
            .rposition(|row| !row.is_empty())
            .map_or(first, |i| i + 1);
        let mut prefix = Self::default();
        let mut suffix = Self::default();
        let mut content = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            if index < first {
                prefix.append(&row);
            } else if index >= last {
                suffix.append(&row);
            } else {
                content.push(row);
            }
        }
        Self::join([prefix, Self::join(content, "\n"), suffix], "")
    }
}

impl From<String> for LayoutText {
    fn from(visible: String) -> Self {
        let row_breaks = visible.bytes().filter(|byte| *byte == b'\n').count();
        Self {
            rendered: visible.clone(),
            visible,
            alignments: Vec::new(),
            row_breaks,
        }
    }
}

impl From<&str> for LayoutText {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl TableText for LayoutText {
    fn has_alignment(&self) -> bool {
        !self.alignments.is_empty()
    }

    fn align_within(self, width: Option<usize>) -> Self {
        self.position_alignments(width)
    }
    fn plain(value: &str) -> Self {
        value.into()
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn ends_with_break(&self) -> bool {
        self.visible.ends_with('\n')
    }

    fn width(&self) -> usize {
        mant_ir::geometry::text_width(&self.visible)
    }

    fn physical_lines(&self) -> Vec<Self> {
        if self.is_empty() {
            vec![self.clone()]
        } else {
            self.split(true)
        }
    }

    fn join(values: &[Self], separator: &str) -> Self {
        Self::join(values.iter().cloned(), separator)
    }

    fn prefixed(self, prefix: &str) -> Self {
        self.prefixed(prefix)
    }

    fn append(&mut self, other: &Self) {
        self.append(other);
    }
}
