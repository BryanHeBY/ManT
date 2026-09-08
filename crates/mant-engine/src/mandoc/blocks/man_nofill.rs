//! Man EX/SY physical-line and font-reset policy.
use super::{
    FontState, Inline, Node, NodeKind, ends_with_line_continuation, first_part_children,
    lower_inline_nodes_with_font_state, participates_in_inline_flow, source_span,
};

pub(super) struct LoweredNoFillLine {
    pub(super) nodes: Vec<Inline>,
    pub(super) source: Option<mant_ir::SourceSpan>,
    pub(super) continues_line: bool,
}

pub(super) fn lower_no_fill_lines(
    node: &Node,
    default_name: Option<&str>,
    font: &mut FontState,
) -> Option<Vec<LoweredNoFillLine>> {
    if node.flags.no_fill && participates_in_inline_flow(node) {
        return Some(vec![LoweredNoFillLine {
            nodes: lower_inline_nodes_with_font_state(
                std::slice::from_ref(node),
                default_name,
                true,
                font,
            ),
            source: source_span(node),
            continues_line: ends_with_line_continuation(node),
        }]);
    }
    let body = first_part_children(node, NodeKind::Body);
    if node.macro_name.as_deref() != Some("SY") || !body.iter().any(|child| child.flags.no_fill) {
        return None;
    }

    let mut lines = Vec::new();
    let saved = font.push_scope(crate::mandoc::roff_escape::RoffFont::Strong);
    let head = lower_inline_nodes_with_font_state(
        first_part_children(node, NodeKind::Head),
        default_name,
        true,
        font,
    );
    font.pop_scope(saved);
    if !head.is_empty() {
        lines.push(LoweredNoFillLine {
            nodes: head,
            source: source_span(node),
            continues_line: first_part_children(node, NodeKind::Head)
                .last()
                .is_some_and(ends_with_line_continuation),
        });
    }
    for child in body {
        let line = lower_inline_nodes_with_font_state(
            std::slice::from_ref(child),
            default_name,
            true,
            font,
        );
        if !line.is_empty() {
            lines.push(LoweredNoFillLine {
                nodes: line,
                source: source_span(child),
                continues_line: ends_with_line_continuation(child),
            });
        }
    }
    // The man SY macro closes its font scope after its whole body, not after
    // each physical no-fill line. YS/EE and subsequent prose start regular.
    *font = FontState::new();
    Some(lines)
}
