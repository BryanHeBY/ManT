//! man(7)-specific macro handling, the seed of the analog of upstream
//! `man_html.c`'s `man_html_acts` dispatch family.

use libmandoc_rs::{MacroToken::Man, ManMacro, Node, NodeKind};

use super::{super::first_part_children, BlockLowerer, DEFAULT_MAN_TAG_WIDTH, NodeSourceContext};

impl BlockLowerer<'_, '_> {
    pub(super) fn push_man_paragraph(&mut self, node: &Node, source_predecessor: bool) {
        // man_term.c::pre_PP() closes the preceding row and enters BODY, but
        // PP/P/LP have no post handler. Keep BODY in this same output and
        // execution owner so a following UR/MT post can finish its last word.
        self.state.flush_preformatted();
        self.state.flush_paragraph();
        self.state
            .set_source_indent(self.indent_columns.macro_origin());
        self.man_list_state.reset();
        self.definition_hanging_width =
            crate::mandoc::layout::Distance::cells(DEFAULT_MAN_TAG_WIDTH);
        self.state.request_man_paragraph_spacing(
            *self.paragraph_distance,
            source_predecessor,
            crate::mandoc::source_span(node),
        );
        // The BLOCK pre ran at node entry. Its empty HEAD and its BODY still
        // pass through print_man_node(), whose generic pre/post font changes
        // are independent of pre_PP's absent macro-specific post handler.
        self.state.formatter.font.man_text_boundary(); // HEAD pre
        self.state.formatter.font.man_text_boundary(); // HEAD post
        self.state.formatter.font.man_text_boundary(); // BODY pre
        let inherited = std::mem::take(&mut self.man_source_predecessor);
        self.push_nodes(first_part_children(node, NodeKind::Body));
        self.man_source_predecessor = inherited;
        self.state.formatter.font.man_text_boundary(); // BODY post
        self.state.formatter.font.man_text_boundary(); // BLOCK post
        self.state.materialize_idle_spacing();
    }

    pub(super) fn prepare_man_synopsis_spacing(&mut self, node: &Node, source: NodeSourceContext) {
        if node.macro_token.as_ref() != Some(&Man(ManMacro::Sy)) {
            return;
        }
        self.state.flush_preformatted();
        self.state.flush_paragraph();
        if source.previous_is_sy {
            return;
        }
        // man_term.c::pre_SY() calls print_bvspace() unless the direct
        // previous native sibling is another SY. The first SY in an RS
        // chain uses the same source-predecessor rule.
        self.state.request_man_paragraph_spacing(
            *self.paragraph_distance,
            source.predecessor,
            crate::mandoc::source_span(node),
        );
    }
}
