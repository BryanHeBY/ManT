//! Lexical evidence retained by a producer's grammar, before IR slice mapping.
use std::ops::Range;

#[derive(Clone, Debug)]
pub(crate) struct RecognizedName {
    pub(crate) name: String,
    pub(crate) parts: Vec<Range<usize>>,
}

impl RecognizedName {
    pub(crate) fn contiguous(name: &str, start: usize) -> Self {
        Self {
            name: name.to_owned(),
            parts: std::iter::once(start..start + name.len()).collect(),
        }
    }
}
