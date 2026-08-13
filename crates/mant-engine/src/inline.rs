//! Shared operations over source-independent inline IR nodes.

use mant_ir::Inline;

/// Flatten inline structure into the text visible to readers and search.
pub(crate) fn plain_text(nodes: &[Inline]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => output.push_str(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => output.push_str(&plain_text(children)),
            Inline::Anchor { .. } => {}
            Inline::LineBreak => output.push('\n'),
        }
    }
    output
}

/// Default visible width for a definition term that shares a description row.
pub(crate) const DEFAULT_INLINE_TERM_MAX_WIDTH: usize = 6;

/// Decide whether definition terms fit beside their first description line.
pub(crate) fn terms_fit_inline(terms: &[Vec<Inline>], max_width: usize) -> bool {
    let width = terms
        .iter()
        .map(|term| plain_text(term))
        .collect::<Vec<_>>()
        .join(", ")
        .trim()
        .chars()
        .count();
    (1..=max_width).contains(&width)
}
