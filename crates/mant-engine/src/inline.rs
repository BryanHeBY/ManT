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

/// Decide whether definition terms fit beside their first description line.
pub(crate) fn terms_fit_inline(terms: &[Vec<Inline>], max_width: usize) -> bool {
    mant_protocol::geometry::definition_run_in_width(terms)
        .is_some_and(|width| (1..=max_width).contains(&width))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_fit_uses_visible_cells_inside_styles_and_original_hard_lines() {
        let terms = |text: &str| {
            vec![vec![Inline::Strong {
                children: vec![Inline::Text { value: text.into() }],
            }]]
        };
        assert!(!terms_fit_inline(&terms("日本日本"), 7));
        assert!(terms_fit_inline(&terms("日本日本"), 8));
        assert!(terms_fit_inline(&terms("e\u{301}"), 1));
        assert!(terms_fit_inline(&terms("😀"), 2));
        assert!(!terms_fit_inline(&terms("😀"), 1));
        assert!(terms_fit_inline(&terms("abc\ndef"), 3));
    }
}
