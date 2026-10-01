//! The one source-order node walker, the analog of upstream
//! `mdoc_html.c::print_mdoc_node()` / `man_html.c::print_man_node()`.
//!
//! Routing order is observable: font requests precede containers and
//! controls; executed spacing precedes no-fill fallback; synopsis/filled
//! handling follows literal flush; structural output receives pending
//! targets only after it is emitted. Subdomains execute a node once and
//! return, never replay its macros.

use libmandoc_rs::Node;

use super::super::controls::{FormatterBoundary, formatter_control};
use super::{
    Block, BlockLowerer, DisplayFillMode, NodeSourceContext, StructuralLowerer, TableEmbedding,
    TableEmbeddingPlan, adjacent_ip_run, append_to_last_inline_block,
    follows_inline_equation_punctuation, is_inline_equation_quote_artifact,
    is_native_transparent_sibling, lower_inline_nodes,
    man_nofill::no_fill_boundary,
    mdoc_macros::{reference_author_conjunction, reference_field_post},
    participates_in_inline_flow, restores_macro_indent, source_span, synopsis, targets,
};

impl BlockLowerer<'_, '_> {
    /// Bibliography fields use the same source execution path as surrounding
    /// text. CVS `termp____post()` writes punctuation after each direct `Rs`
    /// field; the `Rs` wrapper itself has no post text or formatter row
    /// break.
    pub(super) fn push_nodes_with_reference_posts(&mut self, nodes: &[Node], reference_body: bool) {
        let table_plan = TableEmbeddingPlan::new(nodes, self.context);
        let mut synopsis_previous = None;
        let mut has_native_sibling = false;
        let mut previous_native_is_sy = false;
        for (index, node) in nodes.iter().enumerate() {
            let source_predecessor = has_native_sibling || self.man_source_predecessor;
            let previous_is_sy = previous_native_is_sy;
            if !is_native_transparent_sibling(node) {
                has_native_sibling = true;
                previous_native_is_sy = node.macro_name.as_deref() == Some("SY");
            }
            if is_inline_equation_quote_artifact(nodes, index) {
                continue;
            }
            if follows_inline_equation_punctuation(nodes, index) {
                self.state.tighten_next_boundary();
            }
            self.observe_source_fill_mode(node);
            // Both print_man_node() and print_mdoc_node() execute NODE_LINE
            // before visiting a text node or dispatching any request. This
            // source event is independent of the request's own line effect.
            let source_line_entered = self.enter_no_fill_source_line(node);
            self.prepare_node_execution(node);
            let author_pre = reference_body && reference_author_conjunction(nodes, index);
            if author_pre {
                self.push_generated_container_event(
                    node,
                    crate::mandoc::containers::Event::Glyph("and".to_owned()),
                );
            }
            self.push(
                node,
                nodes.get(index + 1),
                table_plan.embedding(index),
                synopsis_previous,
                adjacent_ip_run(nodes, index),
                NodeSourceContext {
                    line_entered: source_line_entered,
                    predecessor: source_predecessor,
                    previous_is_sy,
                },
            );
            if reference_body && let Some(punctuation) = reference_field_post(nodes, index) {
                self.push_generated_container_event(node, crate::mandoc::containers::Event::Tight);
                self.push_generated_container_event(
                    node,
                    crate::mandoc::containers::Event::Glyph(punctuation.to_owned()),
                );
                self.push_generated_container_event(
                    node,
                    crate::mandoc::containers::Event::Release,
                );
            }
            // Source execution, not visible output, owns the predecessor fact.
            if self.context.macro_set == libmandoc_rs::MacroSet::Mdoc
                && super::super::adjacency::is_logical_sibling(node)
            {
                self.paragraph_predecessor = true;
            }
            if !synopsis::transparent_synopsis_predecessor(node) {
                synopsis_previous = Some(node);
            }
        }
    }

    fn push(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        table_embedding: Option<&TableEmbedding>,
        synopsis_previous: Option<&Node>,
        ip_run: Option<super::lists::man::IpRun>,
        source: NodeSourceContext,
    ) {
        if self.push_node_pre_handlers(node, next, source) {
            return;
        }
        let structural_targets = targets::structural_targets(node);
        if self.push_executed_spacing(node) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        // A retained Pp executes term_vspace even when native no-fill flags
        // would otherwise route it through inline-only word lowering.
        if node.macro_name.as_deref() == Some("Pp") {
            self.state.flush_preformatted();
            let lines = self.state.resolve_vertical_space(1);
            self.state.flush_paragraph_for_line_request();
            self.state
                .queue_targets(structural_targets, source_span(node));
            // CVS termp_pp_pre() executes term_vspace() even for the first
            // child of a compact Bd BODY. A detached display starts with an
            // empty IR sink, but that does not cancel the authored request.
            if self.paragraph_predecessor
                || !self.state.output.is_empty()
                || self.display_fill.is_some()
            {
                self.state.output.push(Block::VerticalSpace {
                    lines,
                    source: source_span(node),
                });
            }
            return;
        }
        let single_line_literal =
            self.display_fill == Some(DisplayFillMode::SingleLine) && self.state.formatter.no_fill;
        if self.push_no_fill_lines(node, next, source.line_entered, single_line_literal) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        if node.macro_name.as_deref() == Some("Rs") {
            self.push_bibliography(node);
            return;
        }
        if matches!(node.macro_name.as_deref(), Some("UR" | "MT")) {
            // man_term.c::print_man_node() observes this source line before
            // pre_UR(), but pre_UR() itself does not close a formatter row.
            // Keep the active literal sink and pending \c/\z for BODY text.
            self.push_man_link(node);
            return;
        }
        if matches!(node.macro_name.as_deref(), Some("PP" | "P" | "LP")) {
            self.push_man_paragraph(node, source.predecessor);
            return;
        }
        self.state.flush_preformatted();
        if self.push_mdoc_synopsis_declaration(node, synopsis_previous) {
            return;
        }
        if node.flags.delimiter_close
            && participates_in_inline_flow(node)
            && self.state.paragraph_is_empty()
        {
            let tail = lower_inline_nodes(std::slice::from_ref(node), self.context.default_name);
            if append_to_last_inline_block(&mut self.state.output, &tail) {
                return;
            }
        }
        if node.macro_name.as_deref() == Some("br") {
            self.state.hard_break();
        } else if participates_in_inline_flow(node) {
            self.push_inline_node(node, next);
        } else {
            self.state.flush_paragraph();
            let output_start = self.state.output.len();
            let spacing_enabled = self.state.spacing_enabled();
            StructuralLowerer {
                context: self.context,
                indent_columns: if restores_macro_indent(node) {
                    self.indent_columns.macro_origin()
                } else {
                    self.state.source_indent()
                },
                paragraph_distance: self.paragraph_distance,
                output: &mut self.state.output,
                paragraph_predecessor: self.paragraph_predecessor,
                man_source_predecessor: source.predecessor,
                definition_hanging_width: &mut self.definition_hanging_width,
                man_list_state: &mut self.man_list_state,
                ip_run,
                spacing_enabled,
                formatter: &mut self.state.formatter,
            }
            .push(node, table_embedding);
            if restores_macro_indent(node) {
                self.state
                    .set_source_indent(self.indent_columns.macro_origin());
            }
            self.state
                .queue_targets(structural_targets, source_span(node));
            self.state.attach_pending_to_structural_output(output_start);
        }
    }

    fn push_node_pre_handlers(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        source: NodeSourceContext,
    ) -> bool {
        if self.consume_font_request(node) {
            return true;
        }
        self.prepare_man_synopsis_spacing(node, source);
        // The container callback returns child execution to this same driver.
        if self.push_container(node) {
            return true;
        }
        if self.push_column_display(node) {
            return true;
        }
        if self.column_field && self.push_column_payload(node, next) {
            return true;
        }
        if self.consume_control_or_empty_block(node) {
            return true;
        }
        if self.push_no_fill_synopsis(node) {
            return true;
        }
        false
    }

    /// The output destination changes field flags, never macro interpretation.
    /// Requests and words borrow the same live paragraph/native buffer; real
    /// structural nodes continue through the ordinary block driver below.
    fn push_column_payload(&mut self, node: &Node, next: Option<&Node>) -> bool {
        if node.macro_name.as_deref() == Some("Pp") {
            self.state.column_vertical_space(1);
            return true;
        }
        if participates_in_inline_flow(node)
            || formatter_control(node.macro_name.as_deref())
                .is_some_and(|control| !control.specialized)
            || matches!(
                node.macro_name.as_deref(),
                Some("br" | "sp" | "nf" | "fi" | "ta")
            )
        {
            self.push_inline_node(node, next);
            return true;
        }
        false
    }

    fn prepare_node_execution(&mut self, node: &Node) {
        self.state.formatter.enter_tab_source_node(node);
        self.state.formatter.execute_tab_configuration(node);
        if self.column_field {
            // NODE_LINE was already observed at node entry. A filled word
            // or transparent Xo has no independent native line effect:
            // mdoc_term.c maps Xo/Xc to NULL pre/post, and ordinary styled
            // words only call term_word(). Structural BLOCK pre handlers
            // really do consume the column's live field before changing IR
            // owner: Bl/D1/Dl call term_newln(), and Bd's print_bvspace()
            // starts with the same call. Requests retain their own dispatch.
            if node.kind == libmandoc_rs::NodeKind::Block
                && matches!(node.macro_name.as_deref(), Some("Bl" | "Bd" | "D1" | "Dl"))
            {
                self.state.finish_column_nested_row();
                if node.macro_name.as_deref() == Some("Bd")
                    && !node.compact
                    && self.paragraph_predecessor
                {
                    // print_bvspace first calls term_newln above, then
                    // term_vspace calls it again before backend endline
                    // (mdoc_term.c:589,619; term.c:491). The latter can
                    // close a row that
                    // NOBREAK left open rather than assert an empty row.
                    // Execute that native effect once in its live owner;
                    // structural output must not request a second gap.
                    self.state.column_vertical_space(1);
                }
            }
            return;
        }
        let formatter_control = formatter_control(node.macro_name.as_deref());
        if formatter_control.is_some_and(|control| control.boundary == FormatterBoundary::Line) {
            // This is the actual request dispatch, after HEAD execution and
            // before BODY output. The definition checkpoint records it once.
            self.state.formatter.note_definition_boundary();
        }
        let single_line_literal =
            self.display_fill == Some(DisplayFillMode::SingleLine) && self.state.formatter.no_fill;
        match no_fill_boundary(node, single_line_literal) {
            FormatterBoundary::None => {}
            FormatterBoundary::Line => {
                self.state.formatter.clear_trailing_literal_row();
                self.settle_no_fill_inline();
                if formatter_control.is_some() && self.state.formatter.no_fill {
                    // roff_term_pre_br()/term_newln() end the literal owner's
                    // row even after \c; the request's handler may still
                    // apply geometry or spacing afterward.
                    self.state.end_literal_execution_line();
                }
                if formatter_control.is_some_and(|control| {
                    !control.specialized
                        || control.settle_before_handler && !self.state.formatter.no_fill
                }) {
                    // The request's native term_newln() precedes its other
                    // effects. Close the shared execution row here even when
                    // a later handler owns payload or geometry (notably ce,
                    // rj, and in); otherwise a pending \z reaches BODY text.
                    self.state.hard_break();
                }
            }
            FormatterBoundary::NoBreak => {
                if self.state.has_formatter_cell()
                    // A rejected unit still occupies the native buffer until
                    // term_flushln() retires it (term.c:233-237). It need not
                    // have a visible or pending glyph in the output sink.
                    || self.state.formatter.wipe_remainder
                    || self
                        .state
                        .formatter
                        .no_fill_inline
                        .has_pending_formatter_cell(&self.state.formatter.execution)
                {
                    self.state.no_break_formatter_flush();
                }
            }
        }
        if node.macro_name.as_deref() == Some("SY") {
            // man_term.c::print_man_node() replaces the active slot when SY
            // BLOCK enters; it does not clear the font stack or fontlast.
            self.state.formatter.font.man_text_boundary();
        } else if matches!(
            node.macro_name.as_deref(),
            Some("PP" | "P" | "LP" | "HP" | "IP" | "TP" | "TQ" | "RS")
        ) {
            // This is the BLOCK pre transition, not a fresh font stack.
            // print_man_node() preserves the independent previous register.
            self.state.formatter.font.man_text_boundary();
        }
    }

    fn enter_no_fill_source_line(&mut self, node: &Node) -> bool {
        if self.column_field {
            self.state.enter_column_node(node);
            return node.flags.no_fill && node.flags.line_start;
        }
        if !self.state.formatter.no_fill || !node.flags.no_fill || !node.flags.line_start {
            return false;
        }
        self.resume_no_fill_row();
        if !self.state.formatter.no_fill_inline.continues_source_line() {
            self.settle_no_fill_inline();
            self.state.begin_no_fill_source_line();
        }
        true
    }

    pub(super) fn observe_source_fill_mode(&mut self, node: &Node) {
        let was_no_fill = self.state.formatter.no_fill;
        match node.macro_name.as_deref() {
            Some("nf") => self.state.formatter.no_fill = true,
            Some("fi") => self.state.formatter.no_fill = false,
            _ if self.display_fill == Some(DisplayFillMode::NodeFlags) => {
                self.state.formatter.no_fill = node.flags.no_fill;
            }
            _ if self.display_fill == Some(DisplayFillMode::SingleLine) => {}
            _ if node.flags.no_fill => self.state.formatter.no_fill = true,
            _ if node.scope_end.is_none() && participates_in_inline_flow(node) => {
                self.state.formatter.no_fill = false;
            }
            _ => {}
        }
        if was_no_fill && !self.state.formatter.no_fill {
            self.state.formatter.clear_trailing_literal_row();
        }
    }
}
