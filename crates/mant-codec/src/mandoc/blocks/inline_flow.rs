//! Filled inline routing and source-boundary recognition, not structural ownership.
use super::{
    Block, BlockState, Inline, Node, NodeKind, append_inline_node_with_next, is_enclosure_macro,
    lower_man_link, source_span, visible_text,
};

impl super::BlockLowerer<'_, '_> {
    pub(super) fn push_inline_node(&mut self, node: &Node, next: Option<&Node>) {
        let source = source_span(node);
        self.state.push_source_inline_with(
            source,
            starts_indented_filled_line(node),
            ends_with_line_continuation(node),
            node.kind == NodeKind::Text && node.flags.line_start,
            |builder| {
                builder.font = self.formatter.font;
                append_inline_node_with_next(builder, node, next, self.context.default_name);
                self.formatter.font = builder.font;
                self.formatter.spacing = builder.spacing_enabled();
            },
        );
        self.state.inherit_spacing(self.formatter.spacing);
    }
}

/// Keep man-ext links inside the surrounding filled flow.
pub(super) fn push_man_link(
    state: &mut BlockState,
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) {
    state.push_inline(
        lower_man_link(node, default_name, spacing_enabled),
        source_span(node),
        starts_indented_filled_line(node),
        ends_with_line_continuation(node),
    );
}

pub(super) fn is_inline_equation(node: &Node) -> bool {
    node.kind == NodeKind::Equation
        && !node.flags.line_start
        && node
            .equation
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
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

/// Attach a native closing delimiter to the preceding visible flow, walking
/// the last actual item/cell rather than generating a new paragraph for it.
pub(super) fn append_to_last_inline_block(blocks: &mut [Block], tail: &[Inline]) -> bool {
    for block in blocks.iter_mut().rev() {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                children.extend_from_slice(tail);
                return true;
            }
            Block::List { items, .. } => {
                if items
                    .last_mut()
                    .is_some_and(|item| append_to_last_inline_block(&mut item.blocks, tail))
                {
                    return true;
                }
            }
            Block::DefinitionList { items, .. } => {
                if items.last_mut().is_some_and(|item| {
                    append_to_last_inline_block(&mut item.description, tail)
                        || item.terms.last_mut().is_some_and(|term| {
                            term.extend_from_slice(tail);
                            true
                        })
                }) {
                    return true;
                }
            }
            Block::Table { rows, .. } => {
                if rows
                    .last_mut()
                    .and_then(|row| row.cells.last_mut())
                    .is_some_and(|cell| append_to_last_inline_block(&mut cell.blocks, tail))
                {
                    return true;
                }
            }
            Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
    false
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
    if node.macro_name.as_deref() == Some("Lk") {
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
            return children
                .first()
                .is_some_and(|address| ends_with_line_continuation(address));
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
/// structural breaks. Nm retains its separate head/body handling.
pub(super) fn participates_in_inline_flow(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Text | NodeKind::Element)
        || is_inline_equation(node)
        || is_enclosure_macro(node.macro_name.as_deref())
        || matches!(node.macro_name.as_deref(), Some("Nd" | "Fo"))
}
