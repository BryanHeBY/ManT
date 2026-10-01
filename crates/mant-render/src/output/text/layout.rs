//! One text layout result, measured independently of zero-width decoration.

use super::super::table::TableText;

/// Both representations come from the same inline visit and share all layout
/// operations. The visible side is local measurement material, not a second
/// renderer or a copy of the document model. Keeping complete rows also lets
/// Unicode width account for combining sequences crossing style boundaries.
#[derive(Clone, Debug, Default)]
pub(in crate::output) struct LayoutText {
    pub(super) rendered: String,
    pub(super) visible: String,
}

impl LayoutText {
    pub(super) fn decorated(visible: &str, rendered: String) -> Self {
        // The public decorator contract preserves newlines. An invalid
        // callback must not make the row zipper silently discard source text.
        let rendered = if visible.bytes().filter(|byte| *byte == b'\n').count()
            == rendered.bytes().filter(|byte| *byte == b'\n').count()
        {
            rendered
        } else {
            visible.to_owned()
        };
        Self {
            rendered,
            visible: visible.to_owned(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    pub(super) fn append(&mut self, other: &Self) {
        self.rendered.push_str(&other.rendered);
        self.visible.push_str(&other.visible);
    }

    pub(super) fn push_plain(&mut self, text: &str) {
        self.rendered.push_str(text);
        self.visible.push_str(text);
    }

    pub(super) fn prefixed(mut self, prefix: &str) -> Self {
        self.rendered.insert_str(0, prefix);
        self.visible.insert_str(0, prefix);
        self
    }

    pub(super) fn strip_prefix(mut self, prefix: &str) -> Self {
        if self.visible.starts_with(prefix) && self.rendered.starts_with(prefix) {
            self.visible.drain(..prefix.len());
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
        Self {
            rendered: visible.clone(),
            visible,
        }
    }
}

impl From<&str> for LayoutText {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl TableText for LayoutText {
    fn plain(value: &str) -> Self {
        value.into()
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn line_count(&self) -> usize {
        if self.is_empty() {
            1
        } else {
            self.visible.split_terminator('\n').count()
        }
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
