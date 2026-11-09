//! Filled inline routing and source-boundary recognition, not structural ownership.
use super::{Node, NodeKind, is_enclosure_macro, source_span, visible_text};
use crate::mandoc::adjacency::Siblings;
use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};

impl super::BlockLowerer<'_, '_> {
    pub(super) fn push_inline_node(&mut self, node: &Node, next: Option<&Node>) {
        self.push_inline_node_with_source(
            node,
            Siblings {
                raw_next: next,
                next,
                ..Default::default()
            },
        );
    }

    pub(super) fn push_inline_node_with_source(&mut self, node: &Node, mut siblings: Siblings<'_>) {
        // The block dispatcher already executed synopsis_pre. Retain both
        // next relations for macro post without replaying the predecessor.
        siblings.previous = None;
        // A native post may have consumed a display's field while leaving
        // its physical row open. Reuse that actual owner before another
        // filled word executes, independently of the presentation fill mode.
        self.resume_no_fill_row();
        let source = source_span(node);
        self.state.push_inline_with(
            source,
            starts_indented_filled_line(node),
            ends_with_line_continuation(node),
            |builder| {
                crate::mandoc::inline::append_inline_node_with_source(
                    builder,
                    node,
                    siblings,
                    self.context.default_name,
                );
            },
        );
    }
}

pub(super) fn is_inline_equation(node: &Node) -> bool {
    // roff_EQ sets NODE_LINE for both explicit and delimiter-origin EQ.
    // Older owned snapshots retain the previous source-line fallback.
    node.kind == NodeKind::Equation
        && node.equation_inline.unwrap_or(!node.flags.line_start)
        && node
            .equation
            .as_ref()
            .is_some_and(|value| !value.readable_text().trim().is_empty())
}

/// libmandoc terminates a quoted man-macro argument containing inline eqn by
/// moving the equation beside the macro and retaining the closing source quote
/// as a `\&"` text sibling. The quote is parser scaffolding, not output.
pub(super) fn is_inline_equation_quote_artifact(nodes: &[Node], index: usize) -> bool {
    let Some(node) = nodes.get(index) else {
        return false;
    };
    let Some(previous) = index.checked_sub(1).and_then(|index| nodes.get(index)) else {
        return false;
    };
    is_inline_equation(previous)
        && previous.line == node.line
        && node.kind == NodeKind::Text
        && node
            .text
            .as_deref()
            .is_some_and(|text| visible_text(text).trim() == "\"")
}

pub(super) fn follows_inline_equation_punctuation(nodes: &[Node], index: usize) -> bool {
    let Some(node) = nodes.get(index) else {
        return false;
    };
    let Some(previous) = index.checked_sub(1).and_then(|index| nodes.get(index)) else {
        return false;
    };
    is_inline_equation(previous)
        && previous.line == node.line
        && node.kind == NodeKind::Text
        && node
            .text
            .as_deref()
            .map(visible_text)
            .and_then(|text| text.chars().next())
            .is_some_and(|character| matches!(character, '.' | ',' | ':' | ';' | '!' | '?'))
}

/// Match the filled-text line-break rule used by libmandoc's terminal and
/// HTML renderers: a text node beginning an input line with whitespace starts
/// a new output line.  The first printable text can sit below an inline macro
/// wrapper, so inspect the semantic subtree rather than only direct text
/// siblings.
pub(super) fn starts_indented_filled_line(node: &Node) -> bool {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return false;
    }
    if node.kind == NodeKind::Text {
        return node.flags.line_start
            && node
                .text
                .as_deref()
                .is_some_and(|text| text.starts_with(char::is_whitespace));
    }
    node.children
        .iter()
        .find(|child| !child.flags.no_print && child.kind != NodeKind::Comment)
        .is_some_and(starts_indented_filled_line)
}

/// Whether the final printable fragment in this syntax subtree ends with the
/// roff `\c` escape. The parser retains this as source-boundary semantics so
/// filled and no-fill flows can make the same join decision.
pub(in crate::mandoc) fn ends_with_line_continuation(node: &Node) -> bool {
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return false;
    }
    if node.kind == NodeKind::Text {
        return node.flags.line_continuation;
    }
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Lk)) {
        // mdoc_term.c::termp_lk_pre() executes a descriptive link label,
        // generated colon, and URI in that order.  The source tree keeps the
        // URI first, so a `\\c` on the label is consumed by the colon and
        // must not join the following source line; a `\\c` on the URI is
        // executed last and does.  Inspecting the source subtree's final text
        // node would reverse that formatter contract.
        let children = crate::mandoc::inline::inline_children(node);
        let label_end = children
            .iter()
            .rposition(|child| !child.flags.delimiter_close)
            .map_or(1, |index| index + 1)
            .max(1);
        if label_end > 1 {
            return children.first().is_some_and(ends_with_line_continuation);
        }
    }
    node.children
        .iter()
        .rev()
        .find(|child| !child.flags.no_print && child.kind != NodeKind::Comment)
        .is_some_and(ends_with_line_continuation)
}

/// Whether a parsed node contributes to the current filled inline flow.
///
/// AST block shape is not itself a paragraph boundary. Fo is an inline
/// function scope in prose; the earlier synopsis declaration policy owns its
/// structural breaks. Xo/Xc have no macro pre/post in `mdoc_term.c` and keep
/// their complete child stream in the caller's paragraph. Nm retains its
/// separate head/body handling.
pub(super) fn participates_in_inline_flow(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Text | NodeKind::Element)
        || is_inline_equation(node)
        || is_enclosure_macro(node.macro_token.as_ref())
        || matches!(
            node.macro_token.as_ref(),
            Some(Mdoc(MdocMacro::Nd | MdocMacro::Fo | MdocMacro::Xo))
        )
}
