//! Display output changes its IR destination while native node execution stays
//! in the surrounding block driver.

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{Block, Inline};

use super::{BlockLowerer, DisplayFillMode, FormatterRowBoundary, LoweringContext};

pub(super) fn preformatted_blocks(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    spacing_enabled: bool,
    paragraph_predecessor: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    // CVS mdoc_html.c::print_mdoc_node() selects html_fillmode() at each
    // source node. The terminal's termp_bd_post()/termp_bl_post() settle the
    // current row at the original BODY post, not at an IR owner return.
    let inbound_no_fill = formatter.no_fill;
    let mut paragraph_distance = 1;
    let mut lowerer = BlockLowerer::new(
        context,
        indent_columns,
        &mut paragraph_distance,
        spacing_enabled,
        Vec::new(),
        std::mem::take(formatter),
    );
    lowerer.paragraph_predecessor = paragraph_predecessor;
    lowerer.display_fill = Some(if node.macro_name.as_deref() == Some("Bd") {
        DisplayFillMode::NodeFlags
    } else {
        DisplayFillMode::SingleLine
    });
    lowerer.state.formatter.no_fill = true;

    let body_index = node
        .children
        .iter()
        .position(|child| child.kind == NodeKind::Body);
    if let Some(index) = body_index {
        let body = &node.children[index];
        context
            .scope_posts
            .enter_body(body.id, lowerer.state.formatter.font.checkpoint());
        if node.macro_name.as_deref() == Some("Bd") {
            context
                .scope_posts
                .enter_display_fill(body.id, inbound_no_fill);
        }
        lowerer.push_nodes(&body.children);
        // Native validation can retain delimiter-close children after BODY.
        // They are source nodes and must use the same driver as BODY content.
        for child in node.children[index + 1..]
            .iter()
            .take_while(|child| child.line == node.line && child.flags.delimiter_close)
        {
            lowerer.push_nodes(std::slice::from_ref(child));
        }
        if let Some(saved) = context.scope_posts.exit_body(body.id) {
            lowerer.state.formatter.font.pop_scope(saved);
        }
    } else {
        lowerer.push_nodes(&node.children);
    }

    // A crossed .Ed restores fill at its source marker. If that marker has
    // already consumed the checkpoint, a later .nf remains in force here.
    let close_at_source_marker = body_index.is_some_and(|index| {
        node.macro_name.as_deref() == Some("Bd")
            && match context
                .scope_posts
                .exit_display_fill(node.children[index].id)
            {
                Some(fill) => {
                    lowerer.state.formatter.no_fill = fill;
                    false
                }
                None => true,
            }
    });
    let output = lowerer.finish_into(
        formatter,
        if close_at_source_marker {
            FormatterRowBoundary::Preserve
        } else {
            FormatterRowBoundary::Settle
        },
    );
    if close_at_source_marker
        && formatter.no_fill
        && output.last().is_some_and(|block| {
            matches!(block, Block::Preformatted { children, .. } if !matches!(children.last(), Some(Inline::LineBreak { .. })))
        })
    {
        formatter.mark_trailing_literal_row();
    }
    output
}

#[cfg(test)]
mod tests {
    use mant_ir::Inline;

    #[test]
    fn font_styling_preserves_line_boundaries() {
        let mut builder = crate::mandoc::inline::InlineBuilder::new();
        builder.with_font_scope(crate::mandoc::roff_escape::RoffFont::Strong, |builder| {
            builder.append_text("first");
            builder.hard_break();
            builder.append_text("second");
        });
        let styled = builder.finish();

        assert!(matches!(
            styled.as_slice(),
            [
                Inline::Strong { children: first },
                Inline::LineBreak { .. },
                Inline::Strong { children: second },
            ] if mant_ir::inline_plain_text(first) == "first" && mant_ir::inline_plain_text(second) == "second"
        ));
    }
}
