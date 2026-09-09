//! Man no-fill words retain executed empty rows, not formatter operands.
use super::{
    FontState, Inline, Node, NodeKind, ends_with_line_continuation,
    lower_inline_nodes_with_font_state, participates_in_inline_flow, source_span,
};

pub(super) struct LoweredNoFillLine {
    pub(super) nodes: Vec<Inline>,
    pub(super) source: Option<mant_ir::SourceSpan>,
    pub(super) continues_line: bool,
    pub(super) starts_line: bool,
    pub(super) occupies_row: bool,
}

pub(super) fn lower_no_fill_lines(
    node: &Node,
    default_name: Option<&str>,
    font: &mut FontState,
) -> Option<Vec<LoweredNoFillLine>> {
    if node.flags.no_fill && participates_in_inline_flow(node) {
        let mut nodes = lower_inline_nodes_with_font_state(
            std::slice::from_ref(node),
            default_name,
            true,
            font,
        );
        let mut occupies_row = !nodes.is_empty();
        if nodes.is_empty() {
            let blank_rows = empty_word_rows(node);
            occupies_row = blank_rows > 0;
            for index in 0..blank_rows {
                if index > 0 {
                    nodes.push(Inline::LineBreak);
                }
                nodes.push(Inline::Text {
                    value: String::new(),
                });
            }
        }
        return Some(vec![LoweredNoFillLine {
            nodes,
            source: source_span(node),
            continues_line: ends_with_line_continuation(node),
            starts_line: node.flags.line_start,
            occupies_row,
        }]);
    }
    None
}

/// `man_term` renders every empty TEXT as vertical space, including a B/I
/// operand; `ESCAPE_IGNORE` instead buffers an invisible glyph on the current
/// row. Pure font escapes do neither. Inspect typed decoded events only when
/// ordinary lowering returned no visible payload.
fn empty_word_rows(node: &Node) -> usize {
    // Alternating font macros call term_word on operands themselves instead
    // of visiting man TEXT nodes, so their empty parameters are not vspace.
    let empty_text_is_row =
        crate::mandoc::inline::alternating_font_pair(node.macro_name.as_deref()).is_none();
    let mut stack = vec![node];
    let mut blanks = 0usize;
    let mut zero_width_glyph = false;
    while let Some(node) = stack.pop() {
        if node.flags.no_print || node.kind == NodeKind::Comment {
            continue;
        }
        if node.kind == NodeKind::Text {
            let text = node.text.as_deref().unwrap_or_default();
            if text.is_empty() && empty_text_is_row {
                blanks = blanks.saturating_add(1);
            } else {
                zero_width_glyph |= crate::mandoc::roff_escape::decode(text)
                    .iter()
                    .any(|event| {
                        matches!(
                            event,
                            crate::mandoc::roff_escape::RoffInlineEvent::ZeroWidthGlyph
                        )
                    });
            }
        } else {
            stack.extend(node.children.iter().rev());
        }
    }
    blanks.max(usize::from(zero_width_glyph))
}
