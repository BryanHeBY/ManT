//! The one source-order node walker, the analog of upstream
//! `mdoc_html.c::print_mdoc_node()` / `man_html.c::print_man_node()`.
//!
//! Routing order is observable: font requests precede containers and
//! controls; executed spacing precedes no-fill fallback; synopsis/filled
//! handling follows literal flush; structural output receives pending
//! targets only after it is emitted. Subdomains execute a node once and
//! return, never replay its macros.

use crate::mandoc::adjacency::{self, Siblings};
use libmandoc_rs::Node;
use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, RoffMacro,
};

use super::super::controls::{FormatterBoundary, formatter_control};
use super::{
    BlockLowerer, DisplayFillMode, NodeSourceContext, StructuralLowerer, TableEmbedding,
    TableEmbeddingPlan, adjacent_ip_run, is_native_transparent_sibling,
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
        self.push_source_sequence(nodes, reference_body, false);
    }

    pub(super) fn push_function_children(&mut self, nodes: &[Node]) {
        self.push_source_sequence(nodes, false, true);
    }

    fn push_source_sequence(&mut self, nodes: &[Node], reference_body: bool, function_body: bool) {
        let table_plan = TableEmbeddingPlan::new(nodes, self.context);
        let mut synopsis_previous = None;
        let mut has_native_sibling = false;
        let mut previous_native_is_sy = false;
        for (index, (node, siblings)) in adjacency::with_siblings(nodes).enumerate() {
            let source_predecessor = has_native_sibling || self.man_source_predecessor;
            let previous_is_sy = previous_native_is_sy;
            if !is_native_transparent_sibling(node) {
                has_native_sibling = true;
                previous_native_is_sy = node.macro_token.as_ref() == Some(&Man(ManMacro::Sy));
            }
            self.enter_annotation_root(node);
            self.observe_source_fill_mode(node);
            // Both print_man_node() and print_mdoc_node() execute NODE_LINE
            // before visiting a text node or dispatching any request. This
            // source event is independent of the request's own line effect.
            let source_line_entered = self.enter_no_fill_source_line(node);
            self.prepare_node_execution(node);
            self.state.enter_synopsis_node(node, synopsis_previous);
            let author_pre = reference_body && reference_author_conjunction(nodes, index);
            if author_pre {
                self.push_generated_container_event(
                    node,
                    crate::mandoc::containers::Event::Glyph("and".to_owned()),
                );
            }
            if function_body && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fa)) {
                let comma_after = !node.children.is_empty()
                    && siblings.next.is_some_and(|next| {
                        next.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fa))
                    });
                self.push_function_argument(node, comma_after);
            } else {
                self.push(
                    node,
                    siblings,
                    table_plan.embedding(index),
                    synopsis_previous,
                    adjacent_ip_run(nodes, index),
                    NodeSourceContext {
                        line_entered: source_line_entered,
                        predecessor: source_predecessor,
                        previous_is_sy,
                    },
                );
            }
            self.finish_node_execution(node);
            self.state
                .formatter
                .execution
                .finish_no_fill_source_line_entry(node.id);
            if reference_body && let Some(punctuation) = reference_field_post(nodes, index) {
                self.push_generated_container_event(node, crate::mandoc::containers::Event::Tight);
                self.push_generated_container_event(
                    node,
                    crate::mandoc::containers::Event::Glyph(punctuation.glyph().to_owned()),
                );
                // termp____post() emits a real punctuation word; unlike
                // Eo/En it never clears NOSPACE after that word. Its outgoing
                // register is whatever the current Sm mode establishes.

                if punctuation.ends_sentence() {
                    self.state.formatter.execution.arm_sentence_end();
                }
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

    fn finish_node_execution(&mut self, node: &Node) {
        if node.flags.synopsis_pretty
            && matches!(
                node.macro_token.as_ref(),
                Some(Mdoc(MdocMacro::Fn | MdocMacro::Fo))
            )
            && !self.state.formatter.no_fill
            && !self.column_field
            && self.display_fill.is_none()
            && !self.state.formatter.execution.has_formatter_cell()
            && !self.state.formatter.execution.has_open_native_device_row()
        {
            // The real Fn/Fo suffix already flushed. Release its completed
            // declaration owner without making a second native post.
            self.state.flush_paragraph();
        }
        let Some((first_id, origin)) = self.first_body_post else {
            return;
        };
        if node.id != first_id {
            return;
        }
        self.first_body_post = None;
        if node.flags.no_fill && reaches_man_node_out(node) {
            // man_term.c::print_man_node(out): the first HP BODY child
            // closes its real row, then assigns offset=rmargin. Roff,
            // empty TEXT, comments, equations and tables returned earlier.
            self.state.finish_native_structural_row();
            self.state.set_source_indent(origin);
        }
    }

    fn push(
        &mut self,
        node: &Node,
        siblings: Siblings<'_>,
        table_embedding: Option<&TableEmbedding>,
        synopsis_previous: Option<&Node>,
        ip_run: Option<super::lists::man::IpRun>,
        source: NodeSourceContext,
    ) {
        let next = siblings.raw_next;
        if self.push_node_pre_handlers(node, siblings, source) {
            return;
        }
        let structural_targets = targets::structural_targets(node);
        if self.push_executed_spacing(node) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        if self.push_mdoc_paragraph_spacing(node, &structural_targets) {
            return;
        }
        // Nm's HEAD and BODY consume the current native field. Changing its
        // IR owner before HEAD post would lose an enclosing It's live row.
        if self.push_name_block(node, synopsis_previous) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        let single_line_literal = self.display_fill == Some(DisplayFillMode::SingleLine);
        if self.push_no_fill_lines(node, siblings, source.line_entered, single_line_literal) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        if self.push_configuration_equation(node, siblings) {
            return;
        }
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Rs)) {
            self.push_bibliography(node);
            return;
        }
        if matches!(
            node.macro_token.as_ref(),
            Some(Man(ManMacro::Ur | ManMacro::Mt))
        ) {
            // man_term.c::print_man_node() observes this source line before
            // pre_UR(), but pre_UR() itself does not close a formatter row.
            // Keep the active literal sink and pending \c/\z for BODY text.
            self.push_man_link(node);
            return;
        }
        if matches!(
            node.macro_token.as_ref(),
            Some(Man(ManMacro::Pp | ManMacro::P | ManMacro::Lp))
        ) {
            self.push_man_paragraph(node, source.predecessor);
            return;
        }
        if node.kind == libmandoc_rs::NodeKind::Table && node.flags.table_start {
            // tbl_html.c::html_tblopen closes the current semantic root.
            self.state
                .close_annotation_root(self.state.formatter.no_fill);
            // Both native node dispatchers call term_newln only before a
            // table's first span (man_term.c:966-970; mdoc_term.c:388-392).
            // Settle the live parent input before draining its output owner;
            // pre_br would also change flags and is not this native event.
            self.state.finish_native_body_line();
        }
        self.state.flush_preformatted();
        if self.push_mdoc_synopsis_declaration(node) {
            return;
        }
        if node.macro_token.as_ref() == Some(&Roff(RoffMacro::Br)) {
            // roff_term_pre_br consumes the live native field before its
            // BRIND/NOBREAK update; a projection-only hard break loses the
            // late accepted HEAD presentation receipt.
            self.state.pre_break_request();
        } else if participates_in_inline_flow(node) {
            self.push_inline_node_with_source(node, siblings);
        } else {
            self.state.flush_paragraph();
            self.end_literal_for_structure(node);
            let output_start = self.state.output.len();
            let accepted_before = self.state.formatter.execution.visible_content_checkpoint();
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
                literal_assembly: &mut self.state.literal_assembly,
                paragraph_predecessor: self.paragraph_predecessor,
                man_source_predecessor: source.predecessor,
                definition_hanging_width: &mut self.definition_hanging_width,
                man_list_state: &mut self.man_list_state,
                ip_run,
                spacing_enabled,
                formatter: &mut self.state.formatter,
            }
            .push(node, next, table_embedding);
            if restores_macro_indent(node) {
                self.state
                    .set_source_indent(self.indent_columns.macro_origin());
            }
            self.state
                .queue_targets(structural_targets, source_span(node));
            self.state
                .finish_structural_output(output_start, accepted_before);
            self.state.attach_pending_to_structural_output(output_start);
        }
    }

    fn push_node_pre_handlers(
        &mut self,
        node: &Node,
        siblings: Siblings<'_>,
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
        if self.column_field && self.push_column_payload(node, siblings) {
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

    fn end_literal_for_structure(&mut self, node: &Node) {
        // RS changes only the source offset; its output and ownership are
        // transferred together below. Semantic structures have their own
        // content owners, even if the surrounding fill mode remains nf.
        if node.macro_token.as_ref() != Some(&Man(ManMacro::Rs)) || self.man_list_state.is_active()
        {
            self.state.literal_assembly.end(self.state.output.len());
        }
    }

    /// The output destination changes field flags, never macro interpretation.
    /// Requests and words borrow the same live paragraph/native buffer; real
    /// structural nodes continue through the ordinary block driver below.
    fn push_column_payload(&mut self, node: &Node, siblings: Siblings<'_>) -> bool {
        if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Pp)) {
            self.state.column_vertical_space(1);
            return true;
        }
        if participates_in_inline_flow(node)
            || formatter_control(node.macro_token.as_ref())
                .is_some_and(|control| !control.specialized)
            || matches!(
                node.macro_token.as_ref(),
                Some(Roff(
                    RoffMacro::Br | RoffMacro::Sp | RoffMacro::Nf | RoffMacro::Fi | RoffMacro::Ta
                ))
            )
        {
            self.push_inline_node_with_source(node, siblings);
            return true;
        }
        false
    }

    fn prepare_node_execution(&mut self, node: &Node) {
        self.state.formatter.enter_tab_source_node(node);
        self.state.formatter.execute_tab_configuration(node);
        if !self.column_field
            && node.kind == libmandoc_rs::NodeKind::Block
            && matches!(
                node.macro_token.as_ref(),
                Some(
                    Mdoc(MdocMacro::Bl | MdocMacro::Bd | MdocMacro::D1 | MdocMacro::Dl)
                        | Man(ManMacro::Rs)
                )
            )
        {
            // These BLOCK pres call term_newln() before installing their
            // own geometry, including an occupied detached HEAD with no
            // BODY buffer (mdoc_term.c:1128,1329,1442; man_term.c:772).
            // An IR drain cannot substitute for that source execution or
            // its NOSPACE flag.
            self.state.finish_native_structural_row();
        }
        if self.column_field {
            // NODE_LINE was already observed at node entry. A filled word
            // or transparent Xo has no independent native line effect:
            // mdoc_term.c maps Xo/Xc to NULL pre/post, and ordinary styled
            // words only call term_word(). Structural BLOCK pre handlers
            // really do consume the column's live field before changing IR
            // owner: Bl/D1/Dl call term_newln(), and Bd's print_bvspace()
            // starts with the same call. Requests retain their own dispatch.
            if node.kind == libmandoc_rs::NodeKind::Block
                && matches!(
                    node.macro_token.as_ref(),
                    Some(Mdoc(
                        MdocMacro::Bl | MdocMacro::Bd | MdocMacro::D1 | MdocMacro::Dl
                    ))
                )
            {
                self.state.finish_column_nested_row();
                if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd))
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
        let formatter_control = formatter_control(node.macro_token.as_ref());
        if formatter_control.is_some() && crate::mandoc::containers::is_container(node) {
            // The common container driver owns its pre_br and grouped flush
            // events in both output destinations. Preparing another break
            // here would reconsume the already retired rejected field.
            return;
        }
        if formatter_control.is_some_and(|control| control.boundary == FormatterBoundary::Line) {
            // This is the actual request dispatch, after HEAD execution and
            // before BODY output. The definition checkpoint records it once.
            self.state.formatter.note_definition_boundary();
        }
        let single_line_literal = self.display_fill == Some(DisplayFillMode::SingleLine);
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
        if node.macro_token.as_ref() == Some(&Man(ManMacro::Sy)) {
            // man_term.c::print_man_node() replaces the active slot when SY
            // BLOCK enters; it does not clear the font stack or fontlast.
            self.state.formatter.font.man_text_boundary();
        } else if matches!(
            node.macro_token.as_ref(),
            Some(Man(ManMacro::Pp
                | ManMacro::P
                | ManMacro::Lp
                | ManMacro::Hp
                | ManMacro::Ip
                | ManMacro::Tp
                | ManMacro::Tq
                | ManMacro::Rs))
        ) {
            // This is the BLOCK pre transition, not a fresh font stack.
            // print_man_node() preserves the independent previous register.
            self.state.formatter.font.man_text_boundary();
        }
    }

    fn enter_annotation_root(&mut self, node: &Node) {
        // man_html.c closes the paragraph in these BLOCK pre handlers;
        // a Rust output drain and roff .br are not equivalent events.
        if node.kind == libmandoc_rs::NodeKind::Block
            && matches!(
                node.macro_token.as_ref(),
                Some(Man(ManMacro::P
                    | ManMacro::Pp
                    | ManMacro::Lp
                    | ManMacro::Hp
                    | ManMacro::Ip
                    | ManMacro::Tp
                    | ManMacro::Tq
                    | ManMacro::Rs
                    | ManMacro::Sy))
            )
        {
            self.state
                .close_annotation_root(self.state.formatter.no_fill);
        }
        // roff_html_pre_sp closes a filled paragraph even for sp 0 or
        // negative spacing; in nf it only ends a physical device row.
        if node.macro_token.as_ref() == Some(&Roff(RoffMacro::Sp)) && !self.state.formatter.no_fill
        {
            self.state
                .close_annotation_root(self.state.formatter.no_fill);
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
        if !self.state.formatter.execution.source_row_continues() {
            if self.state.settle_no_fill_row(true) {
                self.state
                    .formatter
                    .execution
                    .note_no_fill_source_line_entered(node.id);
            }
            self.state.begin_no_fill_source_line();
        }
        true
    }

    pub(super) fn observe_source_fill_mode(&mut self, node: &Node) {
        let was_no_fill = self.state.formatter.no_fill;
        match node.macro_token.as_ref() {
            Some(Roff(RoffMacro::Nf)) => self.state.formatter.no_fill = true,
            Some(Roff(RoffMacro::Fi)) => self.state.formatter.no_fill = false,
            _ if self.display_fill == Some(DisplayFillMode::NodeFlags) => {
                self.state.formatter.no_fill = node.flags.no_fill;
            }
            _ if self.display_fill == Some(DisplayFillMode::SingleLine) => {
                self.state.formatter.no_fill = node.flags.no_fill;
            }
            _ if node.flags.no_fill => self.state.formatter.no_fill = true,
            _ if node.scope_end.is_none() && participates_in_inline_flow(node) => {
                self.state.formatter.no_fill = false;
            }
            _ => {}
        }
        if was_no_fill != self.state.formatter.no_fill {
            self.state.close_annotation_root(was_no_fill);
        }
        if was_no_fill && !self.state.formatter.no_fill {
            self.state.formatter.clear_trailing_literal_row();
            self.state.literal_assembly.mark_close();
        }
        if self.state.formatter.no_fill {
            // A preceding fi/EE already executed its row close. Drain that
            // old owner before starting another nf/EX, without replaying it.
            if self.state.literal_assembly.close_pending() {
                self.state.flush_preformatted();
            }
            self.state.literal_assembly.begin(self.state.output.len());
        }
    }
}

/// Native dispatch result, independent of whether Rust emitted any Inline.
fn reaches_man_node_out(node: &Node) -> bool {
    match node.kind {
        libmandoc_rs::NodeKind::Text => node.text.as_ref().is_some_and(|text| !text.is_empty()),
        libmandoc_rs::NodeKind::Comment
        | libmandoc_rs::NodeKind::Equation
        | libmandoc_rs::NodeKind::Table => false,
        _ => matches!(node.macro_token.as_ref(), Some(Man(_))),
    }
}
