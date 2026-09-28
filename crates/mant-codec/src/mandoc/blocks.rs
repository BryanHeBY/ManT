//! One source-order block driver over the copied mandoc tree.
//!
//! Routing order is observable: font requests precede containers and controls;
//! executed spacing precedes no-fill fallback; synopsis/filled handling follows
//! literal flush; structural output receives pending targets only after it is
//! emitted. Subdomains execute a node once and return, never replay its macros.

use libmandoc_rs::{DisplayKind, Node, NodeKind};
use mant_ir::{Block, Inline, Section};

use super::{
    LoweringContext,
    controls::FormatterBoundary,
    first_part_children,
    inline::{
        FilledBoundary, FontState, InlineBuilder, append_inline_node_with_next, is_enclosure_macro,
        lower_inline_nodes, lower_inline_nodes_with_font_state, lower_inline_nodes_with_spacing,
        lower_man_link, plain_text,
    },
    layout::{
        add_leading_spacing, layout, section_spacing, set_block_spacing, update_paragraph_distance,
        vertical_space_delta,
    },
    part_child_groups,
    roff_escape::visible_text,
    source_span, targets,
};

mod container_flow;
mod controls;
mod inline_flow;
pub(super) use inline_flow::ends_with_line_continuation;
use inline_flow::{
    append_to_last_inline_block, follows_inline_equation_punctuation, is_inline_equation,
    is_inline_equation_quote_artifact, participates_in_inline_flow, push_man_link,
};

mod sections;
use sections::is_section;
pub(super) use sections::lower_document_structure;
mod structural;
use structural::StructuralLowerer;
mod synopsis;
use synopsis::lower_synopsis_head;
mod flow;
mod man_nofill;
use flow::BlockState;
use man_nofill::no_fill_boundary;
mod lists;
mod preformatted;
mod tables;

use lists::man::{
    adjacent_ip_run,
    ordered::{ManListState, append_relative_continuation},
};
use lists::{
    ManDefinitionState, lower_man_definition as lower_man_definition_block, lower_mdoc_list,
};
use preformatted::preformatted_blocks;
use tables::{TableEmbedding, TableEmbeddingPlan, append_table_row};

fn lower_blocks_with_spacing(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    lower_blocks_with_predecessor(
        nodes,
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        false,
        formatter,
    )
}

/// A document or section body has a real terminal row boundary at its end.
/// Nested output owners return their active formatter row to the caller.
fn lower_blocks_through_row_end(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    lower_blocks_with_predecessor_and_run_in(
        nodes,
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        false,
        formatter,
        None,
        FormatterRowBoundary::Settle,
    )
}

/// Lower a detached structural body without discarding its predecessor.
/// Native display spacing walks through first-child containers to find an
/// earlier source sibling. An empty child output buffer is not evidence that
/// the body immediately follows a section heading.
fn lower_blocks_with_predecessor(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    paragraph_predecessor: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    lower_blocks_with_predecessor_and_run_in(
        nodes,
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        paragraph_predecessor,
        formatter,
        None,
        FormatterRowBoundary::Preserve,
    )
}

/// The owning macro's BODY post calls `term_newln()` (or `term_flushln()`).
/// Keep this distinct from an IR output-owner return without such a post.
fn lower_blocks_with_body_post_row_end(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    paragraph_predecessor: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    lower_blocks_with_predecessor_and_run_in(
        nodes,
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        paragraph_predecessor,
        formatter,
        None,
        FormatterRowBoundary::Settle,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_blocks_with_predecessor_and_run_in(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    paragraph_predecessor: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    run_in: Option<(crate::mandoc::inline::PreservedInlineState, usize)>,
    row_boundary: FormatterRowBoundary,
) -> Vec<Block> {
    let mut lowerer = BlockLowerer::new(
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        Vec::new(),
        std::mem::take(formatter),
    );
    if let Some((execution, generated_cells)) = run_in {
        lowerer
            .state
            .inherit_run_in_execution(execution, generated_cells);
    }
    lowerer.paragraph_predecessor = paragraph_predecessor;
    lowerer.push_nodes(nodes);
    lowerer.finish_into(formatter, row_boundary)
}

const DEFAULT_MAN_TAG_WIDTH: i32 = 7;

/// A Rust output-owner return does not by itself end a CVS formatter row.
#[derive(Clone, Copy, Eq, PartialEq)]
enum FormatterRowBoundary {
    Preserve,
    Settle,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DisplayFillMode {
    /// Bd uses each source node's parsed `NODE_NOFILL` bit.
    NodeFlags,
    /// D1/Dl enter one literal row even though their operands lack the bit.
    SingleLine,
}

struct BlockLowerer<'a, 'source> {
    context: &'a LoweringContext<'source>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &'a mut u16,
    state: BlockState,
    // man(7) starts each section or relative-indent scope with a seven-column
    // hanging margin. Explicit `.TP`/`.IP` widths update it for following
    // tagged paragraphs, exactly as mandoc's terminal renderer does.
    definition_hanging_width: crate::mandoc::layout::Distance,
    // Source-proven `.IP`/`.TP` ordinals form lists immediately; this state
    // joins only adjacent, consecutively numbered items of the same style.
    man_list_state: ManListState,
    // Transparent `.RS` scopes retain their native predecessor even when
    // their IR is collected separately for attachment to an ordered item.
    paragraph_predecessor: bool,
    display_fill: Option<DisplayFillMode>,
}

impl<'a, 'source> BlockLowerer<'a, 'source> {
    fn resume_no_fill_row(&mut self) {
        if self.state.formatter.take_trailing_literal_row() {
            self.state.adopt_trailing_preformatted();
        }
    }

    fn new(
        context: &'a LoweringContext<'source>,
        indent_columns: crate::mandoc::layout::SourceIndent,
        paragraph_distance: &'a mut u16,
        spacing_enabled: bool,
        output: Vec<Block>,
        formatter: crate::mandoc::formatter::FormatterState,
    ) -> Self {
        let mut state = BlockState::with_output(indent_columns, spacing_enabled, output, formatter);
        state.inherit_scope_posts(context.scope_posts.clone());
        state.inherit_author_execution(
            state.formatter.author_flow(),
            context.active_mdoc_section()
                == crate::mandoc::source_context::MdocSectionContext::Authors,
        );
        Self {
            context,
            indent_columns,
            paragraph_distance,
            state,
            definition_hanging_width: crate::mandoc::layout::Distance::cells(DEFAULT_MAN_TAG_WIDTH),
            man_list_state: ManListState::new(),
            paragraph_predecessor: false,
            display_fill: None,
        }
    }

    fn push_nodes(&mut self, nodes: &[Node]) {
        self.push_nodes_with_reference_posts(nodes, false);
    }

    /// Bibliography fields use the same source execution path as surrounding
    /// text. CVS `termp____post()` writes punctuation after each direct Rs
    /// field; the Rs wrapper itself has no post text or formatter row break.
    fn push_nodes_with_reference_posts(&mut self, nodes: &[Node], reference_body: bool) {
        let table_plan = TableEmbeddingPlan::new(nodes, self.context);
        let mut synopsis_previous = None;
        for (index, node) in nodes.iter().enumerate() {
            if is_inline_equation_quote_artifact(nodes, index) {
                continue;
            }
            if follows_inline_equation_punctuation(nodes, index) {
                self.state.tighten_next_boundary();
            }
            // CVS print_mdoc_node() enters NODE_LINE before a macro's pre
            // handler writes generated words such as the final author's and.
            self.prepare_node_execution(node);
            self.observe_source_fill_mode(node);
            let author_pre = reference_body && reference_author_conjunction(nodes, index);
            if author_pre {
                self.enter_no_fill_source_line(node);
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
                author_pre && self.state.formatter.no_fill,
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
                && super::adjacency::is_logical_sibling(node)
            {
                self.paragraph_predecessor = true;
            }
            if !synopsis::transparent_synopsis_predecessor(node) {
                synopsis_previous = Some(node);
            }
        }
    }

    fn push_bibliography(&mut self, node: &Node) {
        // CVS termp_rs_pre() calls term_vspace only for a non-first Rs in
        // SEE ALSO. This is an executed pre boundary, before BODY fields.
        if node.section == libmandoc_rs::NormalizedSection::SeeAlso && self.paragraph_predecessor {
            self.settle_no_fill_inline();
            self.state.flush_preformatted();
            let lines = self.state.resolve_vertical_space(1);
            self.state.flush_paragraph_for_vertical_request();
            self.state.output.push(Block::VerticalSpace {
                lines,
                source: source_span(node),
            });
        }
        let Some(body) = node
            .children
            .iter()
            .find(|child| child.kind == NodeKind::Body)
        else {
            return;
        };
        let posts = self.context.scope_posts.clone();
        posts.enter_body(body.id, self.state.formatter.font.checkpoint());
        self.push_nodes_with_reference_posts(&body.children, true);
        if let Some(saved) = posts.exit_body(body.id) {
            self.state.formatter.font.pop_scope(saved);
        }
    }

    fn consume_font_request(&mut self, node: &Node) -> bool {
        if node.macro_name.as_deref() != Some("ft") {
            return false;
        }
        lower_inline_nodes_with_font_state(
            std::slice::from_ref(node),
            self.context.default_name,
            self.state.spacing_enabled(),
            &mut self.state.formatter.font,
        );
        true
    }

    fn observe_source_fill_mode(&mut self, node: &Node) {
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

    fn push(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        table_embedding: Option<&TableEmbedding>,
        synopsis_previous: Option<&Node>,
        ip_run: Option<lists::man::IpRun>,
        source_line_entered: bool,
    ) {
        if self.consume_font_request(node) {
            return;
        }
        // The container callback returns child execution to this same driver.
        if self.push_container(node) || self.consume_control_or_empty_block(node) {
            return;
        }
        if self.push_no_fill_synopsis(node) {
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
            self.state.flush_paragraph_for_vertical_request();
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
        if self.push_no_fill_lines(node, next, source_line_entered, single_line_literal) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        if node.macro_name.as_deref() == Some("Rs") {
            self.push_bibliography(node);
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
        } else if matches!(node.macro_name.as_deref(), Some("UR" | "MT")) {
            let spacing_enabled = self.state.spacing_enabled();
            push_man_link(
                &mut self.state,
                node,
                self.context.default_name,
                spacing_enabled,
            );
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

    fn prepare_node_execution(&mut self, node: &Node) {
        let formatter_control = super::controls::formatter_control(node.macro_name.as_deref());
        let single_line_literal =
            self.display_fill == Some(DisplayFillMode::SingleLine) && self.state.formatter.no_fill;
        match no_fill_boundary(node, single_line_literal) {
            FormatterBoundary::None => {}
            FormatterBoundary::Line => {
                self.state.formatter.clear_trailing_literal_row();
                self.settle_no_fill_inline();
                if formatter_control.is_some_and(|control| !control.specialized) {
                    self.state.hard_break();
                }
            }
            FormatterBoundary::NoBreak => {
                if self.state.has_formatter_cell()
                    || self
                        .state
                        .formatter
                        .no_fill_inline
                        .has_pending_formatter_cell(&self.state.formatter.execution)
                {
                    let nodes = self
                        .state
                        .formatter
                        .no_fill_inline
                        .take_no_break_cell(&mut self.state.formatter.execution);
                    self.state.no_break_formatter_flush(nodes);
                }
            }
        }
        if matches!(
            node.macro_name.as_deref(),
            Some("PP" | "P" | "LP" | "HP" | "IP" | "TP" | "TQ" | "RS" | "SY")
        ) {
            self.state.formatter.font = FontState::new();
        }
    }

    fn enter_no_fill_source_line(&mut self, node: &Node) {
        if !self.state.formatter.no_fill {
            return;
        }
        self.resume_no_fill_row();
        if node.flags.line_start && !self.state.formatter.no_fill_inline.continues_source_line() {
            self.settle_no_fill_inline();
            self.state.begin_no_fill_source_line();
        }
    }

    fn settle_no_fill_inline(&mut self) {
        let nodes = self
            .state
            .formatter
            .no_fill_inline
            .take_settled_row(&mut self.state.formatter.execution);
        if !nodes.is_empty() {
            self.state
                .push_preformatted(nodes, None, false, false, true);
        }
    }

    fn finish_into(
        mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        row_boundary: FormatterRowBoundary,
    ) -> Vec<Block> {
        if row_boundary == FormatterRowBoundary::Settle {
            self.settle_no_fill_inline();
        }
        let blocks = self.state.finish_with_formatter(formatter);
        self.context.check_gap_bounds(&blocks);
        blocks
    }
}

fn is_reference_field(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some(
            "%A" | "%B"
                | "%C"
                | "%D"
                | "%I"
                | "%J"
                | "%N"
                | "%O"
                | "%P"
                | "%Q"
                | "%R"
                | "%T"
                | "%U"
                | "%V"
        )
    )
}

/// `roff.c::roff_node_prev()/next()` skip comments, NOPRT nodes and the
/// formatter's transparent requests before bibliography pre/post handlers
/// inspect neighboring fields.
fn next_reference_sibling(nodes: &[Node], index: usize) -> Option<usize> {
    ((index + 1)..nodes.len()).find(|&next| super::adjacency::is_logical_sibling(&nodes[next]))
}

fn previous_reference_sibling(nodes: &[Node], index: usize) -> Option<usize> {
    (0..index)
        .rev()
        .find(|&previous| super::adjacency::is_logical_sibling(&nodes[previous]))
}

/// CVS `mdoc_term.c::termp__a_pre()` adds `and` before the final author.
fn reference_author_conjunction(nodes: &[Node], index: usize) -> bool {
    nodes[index].macro_name.as_deref() == Some("%A")
        && previous_reference_sibling(nodes, index)
            .is_some_and(|previous| nodes[previous].macro_name.as_deref() == Some("%A"))
        && next_reference_sibling(nodes, index)
            .is_none_or(|next| nodes[next].macro_name.as_deref() != Some("%A"))
}

/// CVS `mdoc_term.c::termp____post()` omits the first comma for exactly two
/// adjacent authors, then uses a period only after the final Rs field.
fn reference_field_post(nodes: &[Node], index: usize) -> Option<&'static str> {
    let node = &nodes[index];
    if !is_reference_field(node) {
        return None;
    }
    let next = next_reference_sibling(nodes, index);
    if node.macro_name.as_deref() == Some("%A")
        && next.is_some_and(|next| nodes[next].macro_name.as_deref() == Some("%A"))
        && next
            .and_then(|next| next_reference_sibling(nodes, next))
            .is_none_or(|after| nodes[after].macro_name.as_deref() != Some("%A"))
        && previous_reference_sibling(nodes, index)
            .is_none_or(|previous| nodes[previous].macro_name.as_deref() != Some("%A"))
    {
        return None;
    }
    Some(if next.is_none() { "." } else { "," })
}

/// These man macros explicitly assign the formatter's macro base. Passive
/// structures such as tables and equations inherit the current `.in` position.
fn restores_macro_indent(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some("PP" | "P" | "LP" | "HP" | "TP" | "TQ" | "IP" | "RS" | "SY")
    )
}
