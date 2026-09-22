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
        FilledBoundary, FontState, InlineBuilder, NoFillInlineState, append_inline_node_with_next,
        is_enclosure_macro, lower_inline_nodes, lower_inline_nodes_with_font_state,
        lower_inline_nodes_with_spacing, lower_man_link, lower_no_fill_line_with_font_state,
        plain_text, updated_spacing,
    },
    layout::{
        add_leading_spacing, layout, layout_with_spacing, section_spacing, set_block_spacing,
        update_paragraph_distance, vertical_space_delta,
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
    starts_indented_filled_line,
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

use lists::man::ordered::{ManListState, append_relative_continuation};
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
) -> Vec<Block> {
    let mut lowerer = BlockLowerer::new(
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        Vec::new(),
        *formatter,
    );
    if let Some((execution, generated_cells)) = run_in {
        lowerer
            .state
            .inherit_run_in_execution(execution, generated_cells);
    }
    lowerer.paragraph_predecessor = paragraph_predecessor;
    lowerer.push_nodes(nodes);
    lowerer.finish_into(formatter)
}

const DEFAULT_MAN_TAG_WIDTH: i32 = 7;

struct BlockLowerer<'a, 'source> {
    context: &'a LoweringContext<'source>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &'a mut u16,
    state: BlockState,
    formatter: crate::mandoc::formatter::FormatterState,
    no_fill_inline: NoFillInlineState,
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
}

impl<'a, 'source> BlockLowerer<'a, 'source> {
    fn new(
        context: &'a LoweringContext<'source>,
        indent_columns: crate::mandoc::layout::SourceIndent,
        paragraph_distance: &'a mut u16,
        spacing_enabled: bool,
        output: Vec<Block>,
        formatter: crate::mandoc::formatter::FormatterState,
    ) -> Self {
        let mut formatter = formatter;
        let mut state = BlockState::with_output(
            indent_columns,
            spacing_enabled,
            output,
            context.content.clone(),
        );
        state.inherit_vertical_space_debt(formatter.vertical_space_debt);
        state.inherit_zero_advance_armed(std::mem::take(&mut formatter.zero_advance_armed));
        state.inherit_author_execution(
            formatter.author_flow(),
            context.active_mdoc_section()
                == crate::mandoc::source_context::MdocSectionContext::Authors,
        );
        Self {
            context,
            indent_columns,
            paragraph_distance,
            state,
            formatter,
            no_fill_inline: NoFillInlineState::new(),
            definition_hanging_width: crate::mandoc::layout::Distance::cells(DEFAULT_MAN_TAG_WIDTH),
            man_list_state: ManListState::new(),
            paragraph_predecessor: false,
        }
    }

    fn push_nodes(&mut self, nodes: &[Node]) {
        let table_plan = TableEmbeddingPlan::new(nodes, self.context);
        let mut synopsis_previous = None;
        for (index, node) in nodes.iter().enumerate() {
            if is_inline_equation_quote_artifact(nodes, index) {
                continue;
            }
            if follows_inline_equation_punctuation(nodes, index) {
                self.state.tighten_next_boundary();
            }
            self.push(
                node,
                nodes.get(index + 1),
                table_plan.embedding(index),
                synopsis_previous,
            );
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

    fn push(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        table_embedding: Option<&TableEmbedding>,
        synopsis_previous: Option<&Node>,
    ) {
        self.prepare_node_execution(node);
        if node.macro_name.as_deref() == Some("ft") {
            lower_inline_nodes_with_font_state(
                std::slice::from_ref(node),
                self.context.default_name,
                self.state.spacing_enabled(),
                &mut self.formatter.font,
            );
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
            if self.paragraph_predecessor || !self.state.output.is_empty() {
                self.state.output.push(Block::VerticalSpace {
                    lines,
                    source: source_span(node),
                });
            }
            return;
        }
        if self.push_no_fill_lines(node) {
            self.state
                .queue_targets(structural_targets, source_span(node));
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
            let tail = self.context.content.lower(
                mant_ir::ContentRootKind::Body,
                source_span(node),
                lower_inline_nodes(std::slice::from_ref(node), self.context.default_name),
            );
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
            self.state.sync_formatter_state(&mut self.formatter);
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
                spacing_enabled,
                formatter: &mut self.formatter,
            }
            .push(node, table_embedding);
            if restores_macro_indent(node) {
                self.state
                    .set_source_indent(self.indent_columns.macro_origin());
            }
            self.sync_paragraph_formatter_state();
            self.state
                .queue_targets(structural_targets, source_span(node));
            self.state.attach_pending_to_structural_output(output_start);
        }
    }

    fn sync_paragraph_formatter_state(&mut self) {
        self.state.inherit_spacing(self.formatter.spacing);
        self.state.inherit_author_execution(
            self.formatter.author_flow(),
            self.context.active_mdoc_section()
                == crate::mandoc::source_context::MdocSectionContext::Authors,
        );
        self.state
            .inherit_vertical_space_debt(self.formatter.vertical_space_debt);
        self.state
            .inherit_zero_advance_armed(std::mem::take(&mut self.formatter.zero_advance_armed));
    }

    fn prepare_node_execution(&mut self, node: &Node) {
        let formatter_control = super::controls::formatter_control(node.macro_name.as_deref());
        match no_fill_boundary(node) {
            FormatterBoundary::None => {}
            FormatterBoundary::Line => {
                self.settle_no_fill_inline();
                if formatter_control.is_some_and(|control| !control.specialized) {
                    self.state.hard_break();
                }
            }
            FormatterBoundary::NoBreak => {
                if self.state.has_formatter_cell()
                    || self.no_fill_inline.has_pending_formatter_cell()
                {
                    let nodes = self.no_fill_inline.take_no_break_cell();
                    self.state.no_break_formatter_flush(nodes);
                }
            }
        }
        if matches!(
            node.macro_name.as_deref(),
            Some("PP" | "P" | "LP" | "HP" | "IP" | "TP" | "TQ" | "RS" | "SY")
        ) {
            self.formatter.font = FontState::new();
        }
    }

    fn settle_no_fill_inline(&mut self) {
        let nodes = self.no_fill_inline.take_settled_row();
        if !nodes.is_empty() {
            self.state
                .push_preformatted(nodes, None, false, false, true);
        }
        self.state
            .inherit_zero_advance_armed(self.no_fill_inline.take_bare_zero_advance_armed());
    }

    fn finish_into(
        mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
    ) -> Vec<Block> {
        self.settle_no_fill_inline();
        let blocks = self.state.finish_with_formatter(&mut self.formatter);
        *formatter = self.formatter;
        self.context.check_gap_bounds(&blocks);
        blocks
    }
}

/// These man macros explicitly assign the formatter's macro base. Passive
/// structures such as tables and equations inherit the current `.in` position.
fn restores_macro_indent(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some("PP" | "P" | "LP" | "HP" | "TP" | "TQ" | "IP" | "RS" | "SY")
    )
}
