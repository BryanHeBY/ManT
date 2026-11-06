//! Execute Nm parts in the current native field, independently of IR owners.

use super::{BlockLowerer, Node, NodeKind};
use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};

impl BlockLowerer<'_, '_> {
    pub(super) fn push_name_block(&mut self, node: &Node, previous: Option<&Node>) -> bool {
        if node.kind != NodeKind::Block || node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::Nm)) {
            return false;
        }
        let name = self.context.default_name;
        if super::first_part_children(node, NodeKind::Body).is_empty() {
            // termp_nm_pre/post(BODY) do nothing for a genuinely empty BODY;
            // its HEAD keeps the ordinary source/continuation driver.
            self.state.push_inline_with(
                super::source_span(node),
                super::inline_flow::starts_indented_filled_line(node),
                super::ends_with_line_continuation(node),
                |builder| {
                    builder.observe_no_fill_source_lines(true);
                    crate::mandoc::inline::append_inline_node_with_siblings(
                        builder, node, previous, None, name,
                    );
                    builder.observe_no_fill_source_lines(false);
                },
            );
            return true;
        }
        let width = crate::mandoc::inline::names::body_width(node, name);
        self.state.note_inline_source(super::source_span(node));
        let previous = previous.map(super::synopsis::SynopsisToken::from_node);
        // mdoc_term.c::termp_nm_pre/post() operates on the surrounding tcol:
        // even inset's unflushed key + fixed separator precede the Nm HEAD
        // in that same buffer. No output drain or separate HEAD interpreter
        // is a native boundary here.
        self.state.with_active_output_builder(|builder| {
            builder.observe_no_fill_source_lines(true);
            builder.execution.enter_native_field_scope();
            builder.begin_definition_head_consumption();
            builder.enter_keep_words();
        });
        for part in &node.children {
            match part.kind {
                NodeKind::Head => self.state.with_active_output_builder(|builder| {
                    crate::mandoc::inline::names::append_head(builder, node, part, previous, name);
                }),
                NodeKind::Body => {
                    let origin = self.state.source_indent();
                    self.state
                        .set_inline_source_origin(super::synopsis::body_origin(
                            node,
                            self.context,
                            origin,
                        ));
                    let saved = self.state.enter_name_body_node(part, width);
                    self.push_nodes(&part.children);
                    self.state.finish_name_body_node(part, saved);
                    self.state.set_inline_source_origin(origin);
                }
                _ => self.push_nodes(std::slice::from_ref(part)),
            }
        }
        self.state.with_active_output_builder(|builder| {
            builder.exit_keep_words();
            builder.execution.exit_native_field_scope();
            builder.observe_no_fill_source_lines(false);
        });
        true
    }
}
