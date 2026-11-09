//! Execute Nm parts in the current native field, independently of IR owners.

use super::{BlockLowerer, Node, NodeKind, flow::InlineOutputRole};
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
        let output_role = self.state.inline_output_role();
        self.state.with_active_output_builder(|builder| {
            builder.observe_no_fill_source_lines(true);
            builder.execution.enter_native_field_scope();
            builder.begin_definition_head_consumption();
            builder.enter_keep_words();
        });
        for part in &node.children {
            match part.kind {
                NodeKind::Head => self.push_name_head(node, part, previous),
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
        self.state.set_inline_output_role(output_role);
        true
    }

    fn push_name_head(
        &mut self,
        node: &Node,
        part: &Node,
        previous: Option<super::synopsis::SynopsisToken>,
    ) {
        let completed_prefix = self.state.has_completed_inline_owner();
        let (saved, closed) = self.state.with_active_output_builder(|builder| {
            let occupied = builder.has_formatter_cell() || builder.has_open_native_device_row();
            let saved = crate::mandoc::inline::names::begin_head(builder, node, part, previous);
            let closed = (occupied || builder.asserted_vertical_row())
                && !builder.has_formatter_cell()
                && !builder.has_open_native_device_row();
            (saved, closed)
        });
        if closed || completed_prefix {
            // The source gate or synopsis_pre already consumed the prefix. Keep its
            // separate semantic owner without a second native newline.
            self.state.drain_closed_inline_owner();
        }
        self.state
            .set_inline_output_role(InlineOutputRole::Declaration);
        self.state.note_inline_source(super::source_span(node));
        let name = self.context.default_name;
        self.state.with_active_output_builder(|builder| {
            crate::mandoc::inline::names::append_started_head(builder, node, part, name, saved);
        });
    }
}
