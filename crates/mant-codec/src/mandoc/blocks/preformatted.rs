//! Display output changes its IR destination while native node execution stays
//! in the surrounding block driver.

use libmandoc_rs::{MacroToken::Mdoc, MdocMacro, Node, NodeKind};
use mant_ir::{Block, Inline};

use super::{BlockLowerer, DisplayFillMode, FormatterRowBoundary, LoweringContext};

impl BlockLowerer<'_, '_> {
    /// Column displays execute in the same active stream as their siblings.
    /// The pre/post handlers really flush native cells, but an IR destination
    /// return does not close a device row retained by NOBREAK (term.c:250).
    pub(super) fn push_column_display(&mut self, node: &Node) -> bool {
        if !self.column_field
            || node.kind != NodeKind::Block
            || !matches!(
                node.macro_token.as_ref(),
                Some(Mdoc(MdocMacro::Bd | MdocMacro::D1 | MdocMacro::Dl))
            )
        {
            return false;
        }
        self.state.enter_column_display_output();
        let inbound_fill = self.state.formatter.no_fill;
        let inbound_display = self.display_fill;
        let geometry = self
            .state
            .formatter
            .execution
            .definition_geometry_checkpoint(node);
        let is_bd = node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd));
        self.display_fill = Some(if is_bd {
            DisplayFillMode::NodeFlags
        } else {
            DisplayFillMode::SingleLine
        });
        self.state.formatter.no_fill = true;
        crate::mandoc::inline::display_tabs::enter_pre(&mut self.state.formatter.execution, node);
        let body_index = node
            .children
            .iter()
            .position(|child| child.kind == NodeKind::Body);
        let mut close_at_source_marker = false;
        if let Some(index) = body_index {
            let body = &node.children[index];
            crate::mandoc::inline::display_tabs::enter_pre(
                &mut self.state.formatter.execution,
                body,
            );
            self.context
                .scope_posts
                .enter_body(body.id, self.state.formatter.font.checkpoint());
            if is_bd {
                self.context
                    .scope_posts
                    .enter_display_fill(body.id, inbound_fill);
            }
            self.push_nodes(&body.children);
            for child in node.children[index + 1..]
                .iter()
                .take_while(|child| child.line == node.line && child.flags.delimiter_close)
            {
                self.push_nodes(std::slice::from_ref(child));
            }
            if let Some(saved) = self.context.scope_posts.exit_body(body.id) {
                self.state.formatter.font.pop_scope(saved);
            }
            if is_bd {
                if let Some(fill) = self.context.scope_posts.exit_display_fill(body.id) {
                    self.state.formatter.no_fill = fill;
                } else {
                    close_at_source_marker = true;
                }
            }
        } else {
            self.push_nodes(&node.children);
        }
        if !close_at_source_marker {
            // Bd BODY post and D1/Dl BLOCK post call term_newln before
            // print_mdoc_node restores geometry (mdoc_term.c:1482,1148,437).
            if is_bd {
                self.state.finish_column_display_body(node.display_kind);
            } else {
                self.state.finish_column_nested_row();
            }
        }
        self.state
            .formatter
            .execution
            .restore_definition_geometry(geometry);
        if !is_bd {
            self.state.formatter.no_fill = inbound_fill;
        }
        self.display_fill = inbound_display;
        self.state.queue_targets(
            super::targets::structural_targets(node),
            super::source_span(node),
        );
        true
    }
}

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
    let geometry = formatter.execution.definition_geometry_checkpoint(node);
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
    lowerer.display_fill = Some(if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd)) {
        DisplayFillMode::NodeFlags
    } else {
        DisplayFillMode::SingleLine
    });
    lowerer.state.formatter.no_fill = true;
    // The enclosing block driver already settled the former output row.
    // Display pre handlers use the same execution state as inline HEADs.
    crate::mandoc::inline::display_tabs::enter_pre(&mut lowerer.state.formatter.execution, node);

    let body_index = node
        .children
        .iter()
        .position(|child| child.kind == NodeKind::Body);
    if let Some(index) = body_index {
        let body = &node.children[index];
        crate::mandoc::inline::display_tabs::enter_pre(
            &mut lowerer.state.formatter.execution,
            body,
        );
        context
            .scope_posts
            .enter_body(body.id, lowerer.state.formatter.font.checkpoint());
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd)) {
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
        node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd))
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
    // print_mdoc_node() restores offset/rmargin only after the native post
    // (mdoc_term.c:437-439). In particular D1/Dl's +6 offset must affect
    // its last field's overrun decision, then stop affecting later text.
    formatter.execution.restore_definition_geometry(geometry);
    // termp_bd_post()/termp_bl_post() can flush a column field under
    // NOBREAK without ending its device row (term.c:220-253). Return that
    // execution fact to the enclosing output owner, independently of the
    // current fill mode or the last projected glyph.
    if formatter.execution.has_column_output_scope()
        && formatter.execution.has_open_native_device_row()
    {
        formatter.mark_trailing_literal_row();
    }
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
        let mut styled = builder.finish();
        let inline_layout = crate::mandoc::inline::take_inline_layout(&mut styled);
        assert!(inline_layout.is_empty());

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
