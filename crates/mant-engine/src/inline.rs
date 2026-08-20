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

/// First character visible to a renderer without allocating flattened text.
pub(crate) fn first_visible_character(nodes: &[Inline]) -> Option<char> {
    nodes.iter().find_map(first_character)
}

/// Last character visible to a renderer without allocating flattened text.
pub(crate) fn last_visible_character(nodes: &[Inline]) -> Option<char> {
    nodes.iter().rev().find_map(last_character)
}

/// Whether an inline fragment contains content other than layout-only breaks.
pub(crate) fn has_printable_character(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Text { value } | Inline::Code { value } => {
            value.chars().any(|character| character != '\n')
        }
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => has_printable_character(children),
        Inline::Anchor { .. } | Inline::LineBreak => false,
    })
}

fn first_character(node: &Inline) -> Option<char> {
    match node {
        Inline::Text { value } | Inline::Code { value } => value.chars().next(),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => first_visible_character(children),
        Inline::Anchor { .. } => None,
        Inline::LineBreak => Some('\n'),
    }
}

fn last_character(node: &Inline) -> Option<char> {
    match node {
        Inline::Text { value } | Inline::Code { value } => value.chars().next_back(),
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => last_visible_character(children),
        Inline::Anchor { .. } => None,
        Inline::LineBreak => Some('\n'),
    }
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
