//! Formatter-local inline output before it is committed to the content store.
//!
//! The terminal-compatible formatter needs to roll back, trim, and replace
//! speculative output. These values are execution scratch state only: once a
//! structural owner accepts them, `mandoc::content` transfers their visible
//! payload into the document's single `ContentStoreBuilder`.

use mant_ir::{LinkTarget, NodeId, SourceSpan};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum DraftInline {
    Text {
        value: String,
    },
    Strong {
        children: Vec<Self>,
    },
    Emphasis {
        children: Vec<Self>,
    },
    Code {
        value: String,
    },
    Link {
        target: LinkTarget,
        title: Option<String>,
        children: Vec<Self>,
    },
    Anchor {
        id: NodeId,
        owner_source: Option<SourceSpan>,
    },
    LineBreak,
}

impl DraftInline {
    #[cfg(test)]
    pub(in crate::mandoc) fn anchor(id: impl Into<NodeId>) -> Self {
        Self::Anchor {
            id: id.into(),
            owner_source: None,
        }
    }

    pub(in crate::mandoc) fn anchor_at(
        id: impl Into<NodeId>,
        owner_source: Option<SourceSpan>,
    ) -> Self {
        Self::Anchor {
            id: id.into(),
            owner_source,
        }
    }
}

pub(in crate::mandoc) fn plain_text(nodes: &[DraftInline]) -> String {
    let mut output = String::new();
    visit_text(nodes, &mut |part| output.push_str(part));
    output
}

pub(in crate::mandoc) fn first_visible_character(nodes: &[DraftInline]) -> Option<char> {
    nodes.iter().find_map(|node| match node {
        DraftInline::Text { value } | DraftInline::Code { value } => value.chars().next(),
        DraftInline::Strong { children }
        | DraftInline::Emphasis { children }
        | DraftInline::Link { children, .. } => first_visible_character(children),
        DraftInline::Anchor { .. } => None,
        DraftInline::LineBreak => Some('\n'),
    })
}

pub(in crate::mandoc) fn last_visible_character(nodes: &[DraftInline]) -> Option<char> {
    nodes.iter().rev().find_map(|node| match node {
        DraftInline::Text { value } | DraftInline::Code { value } => value.chars().next_back(),
        DraftInline::Strong { children }
        | DraftInline::Emphasis { children }
        | DraftInline::Link { children, .. } => last_visible_character(children),
        DraftInline::Anchor { .. } => None,
        DraftInline::LineBreak => Some('\n'),
    })
}

pub(in crate::mandoc) fn has_printable_character(nodes: &[DraftInline]) -> bool {
    nodes.iter().any(|node| match node {
        DraftInline::Text { value } | DraftInline::Code { value } => {
            value.chars().any(|character| character != '\n')
        }
        DraftInline::Strong { children }
        | DraftInline::Emphasis { children }
        | DraftInline::Link { children, .. } => has_printable_character(children),
        DraftInline::Anchor { .. } | DraftInline::LineBreak => false,
    })
}

pub(in crate::mandoc) fn terms_fit_inline(terms: &[Vec<DraftInline>], max_width: usize) -> bool {
    let Some(final_term) = terms.iter().rev().find(|term| {
        term.iter().any(|node| match node {
            DraftInline::Anchor { .. } => false,
            DraftInline::Strong { children }
            | DraftInline::Emphasis { children }
            | DraftInline::Link { children, .. } => !plain_text(children).is_empty(),
            DraftInline::Text { value } | DraftInline::Code { value } => !value.is_empty(),
            DraftInline::LineBreak => true,
        })
    }) else {
        return false;
    };
    let text = plain_text(final_term);
    if text.ends_with('\n') {
        return false;
    }
    let row = text.rsplit('\n').next().unwrap_or_default();
    let width = mant_ir::geometry::text_width(row);
    (1..=max_width).contains(&width)
}

fn visit_text<'a>(nodes: &'a [DraftInline], emit: &mut impl FnMut(&'a str)) {
    for node in nodes {
        match node {
            DraftInline::Text { value } | DraftInline::Code { value } => emit(value),
            DraftInline::Strong { children }
            | DraftInline::Emphasis { children }
            | DraftInline::Link { children, .. } => visit_text(children, emit),
            DraftInline::Anchor { .. } => {}
            DraftInline::LineBreak => emit("\n"),
        }
    }
}
