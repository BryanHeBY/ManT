//! Keep generated row cells separate while annotation wrappers are encoded.

use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::encode::inline) enum PieceKind {
    Content,
    Layout,
    Navigation,
}

#[derive(Default)]
pub(in crate::encode::inline) struct RenderedInline {
    pub(in crate::encode::inline) text: String,
    marked: Vec<(Range<usize>, PieceKind)>,
}

impl RenderedInline {
    pub(in crate::encode::inline) fn plain(text: String) -> Self {
        Self {
            text,
            marked: Vec::new(),
        }
    }

    pub(in crate::encode::inline) fn has_padding(&self) -> bool {
        self.marked
            .iter()
            .any(|(_, kind)| *kind == PieceKind::Layout)
    }

    pub(in crate::encode::inline) fn navigation_only(&self) -> bool {
        let mut end = 0;
        for (range, kind) in &self.marked {
            if *kind != PieceKind::Navigation || range.start != end {
                return false;
            }
            end = range.end;
        }
        end > 0 && end == self.text.len()
    }

    pub(in crate::encode::inline) fn append(&mut self, text: &str, kind: PieceKind) {
        let start = self.text.len();
        self.text.push_str(text);
        if kind != PieceKind::Content && start != self.text.len() {
            self.marked.push((start..self.text.len(), kind));
        }
    }

    pub(in crate::encode::inline) fn mark(&mut self, start: usize, kind: PieceKind) {
        if kind != PieceKind::Content && start != self.text.len() {
            self.marked.push((start..self.text.len(), kind));
        }
    }

    pub(in crate::encode::inline) fn for_each_part(&self, mut append: impl FnMut(&str, PieceKind)) {
        let mut start = 0;
        for (range, kind) in &self.marked {
            if start < range.start {
                append(&self.text[start..range.start], PieceKind::Content);
            }
            append(&self.text[range.clone()], *kind);
            start = range.end;
        }
        if start < self.text.len() {
            append(&self.text[start..], PieceKind::Content);
        }
    }
}
