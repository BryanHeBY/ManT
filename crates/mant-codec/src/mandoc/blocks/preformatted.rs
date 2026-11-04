//! Display output changes its IR destination while native node execution stays
//! in the surrounding block driver.

use libmandoc_rs::{MacroToken::Mdoc, MdocMacro, Node, NodeKind};
use mant_ir::{Block, Inline};

use super::{BlockLowerer, DisplayFillMode, FormatterRowBoundary, LoweringContext};

impl BlockLowerer<'_, '_> {
    /// D1/Dl pre/post belong to BLOCK; each part keeps its own source/font
    /// checkpoint. Unlike Bd HEAD, their HEAD pre permits its children.
    fn push_single_line_display_children(&mut self, node: &Node) {
        for child in &node.children {
            if matches!(child.kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail) {
                let geometry = self
                    .state
                    .formatter
                    .execution
                    .definition_geometry_checkpoint(child);
                self.state.enter_column_node(child);
                let saved_font = self.state.formatter.font.checkpoint();
                self.push_nodes(&child.children);
                self.state.formatter.font.pop_scope(saved_font);
                self.state.formatter.execution.finish_executed_node(child);
                self.state
                    .formatter
                    .execution
                    .restore_definition_geometry(geometry);
            } else {
                self.push_nodes(std::slice::from_ref(child));
            }
        }
    }

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
        let inbound_presentation = self.state.single_line_display_owner;
        self.state.single_line_display_owner = !is_bd;
        crate::mandoc::inline::display_tabs::enter_pre(&mut self.state.formatter.execution, node);
        let body_index = node
            .children
            .iter()
            .position(|child| child.kind == NodeKind::Body);
        let mut close_at_source_marker = false;
        if !is_bd {
            self.push_single_line_display_children(node);
        } else if let Some(index) = body_index {
            let body = &node.children[index];
            crate::mandoc::inline::display_tabs::enter_pre(
                &mut self.state.formatter.execution,
                body,
            );
            self.context.scope_posts.enter_display_body(
                body.id,
                &self.state.formatter.font,
                is_bd.then_some(inbound_fill),
            );
            self.push_nodes(&body.children);
            for child in node.children[index + 1..]
                .iter()
                .take_while(|child| child.line == node.line && child.flags.delimiter_close)
            {
                self.push_nodes(std::slice::from_ref(child));
            }
            self.context
                .scope_posts
                .restore_body_font(body.id, &mut self.state.formatter.font);
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
        self.state.single_line_display_owner = inbound_presentation;
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
    lowerer.state.single_line_display_owner =
        node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::Bd));
    // The enclosing block driver already settled the former output row.
    // Display pre handlers use the same execution state as inline HEADs.
    crate::mandoc::inline::display_tabs::enter_pre(&mut lowerer.state.formatter.execution, node);

    let body_index = node
        .children
        .iter()
        .position(|child| child.kind == NodeKind::Body);
    if node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::Bd)) {
        // termp_d1_pre(BLOCK) already selected the display origin/stops.
        // The generic native traversal includes leading and trailing direct
        // children, not merely BODY plus delimiter-close TEXT.
        lowerer.push_single_line_display_children(node);
    } else if let Some(index) = body_index {
        let body = &node.children[index];
        crate::mandoc::inline::display_tabs::enter_pre(
            &mut lowerer.state.formatter.execution,
            body,
        );
        context.scope_posts.enter_display_body(
            body.id,
            &lowerer.state.formatter.font,
            (node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd))).then_some(inbound_no_fill),
        );
        lowerer.push_nodes(&body.children);
        // Native validation can retain delimiter-close children after BODY.
        // They are source nodes and must use the same driver as BODY content.
        for child in node.children[index + 1..]
            .iter()
            .take_while(|child| child.line == node.line && child.flags.delimiter_close)
        {
            lowerer.push_nodes(std::slice::from_ref(child));
        }
        context
            .scope_posts
            .restore_body_font(body.id, &mut lowerer.state.formatter.font);
    } else {
        lowerer.push_nodes(&node.children);
    }

    // A crossed .Ed restores fill at its source marker. If that marker has
    // already consumed the checkpoint, a later .nf remains in force here.
    let returned_fill = body_index.and_then(|index| {
        (node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd))).then(|| {
            context
                .scope_posts
                .exit_display_fill(node.children[index].id)
        })
    });
    let close_at_source_marker = returned_fill == Some(None);
    if !close_at_source_marker {
        // D1/Dl BLOCK post is termp_bl_post(): consume the live field before
        // geometry returns (mdoc_term.c:1142-1148). Its literal presentation
        // does not set NODE_NOFILL or choose a different execution lifetime.
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd)) {
            lowerer.state.finish_column_display_body(node.display_kind);
        } else {
            lowerer.state.finish_native_body_line();
        }
    }
    if let Some(Some(fill)) = returned_fill {
        // The BODY post consumes the last child's live owner first. Only
        // then does normal return restore Bd's fill checkpoint. A crossed
        // Ed already consumed it at its own source position.
        lowerer.state.formatter.no_fill = fill;
    }
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
