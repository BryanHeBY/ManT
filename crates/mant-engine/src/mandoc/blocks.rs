//! One source-order block driver over the copied mandoc tree.
//!
//! Routing order is observable: font requests precede containers and controls;
//! executed spacing precedes no-fill fallback; synopsis/filled handling follows
//! literal flush; structural output receives pending targets only after it is
//! emitted. Subdomains execute a node once and return, never replay its macros.

use libmandoc_rs::{AuthorMode, DisplayKind, Node, NodeKind};
use mant_ir::{Block, Inline, Section};

use super::{
    LoweringContext, first_part_children,
    inline::{
        FilledBoundary, FontState, InlineBuilder, append_inline_node_with_next, is_enclosure_macro,
        lower_inline_nodes, lower_inline_nodes_with_font_state, lower_inline_nodes_with_spacing,
        lower_man_link, plain_text, updated_spacing,
    },
    layout::{
        add_leading_spacing, layout, layout_with_spacing, section_spacing, set_block_spacing,
        update_paragraph_distance, vertical_distance_lines,
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
pub(super) use sections::{lower_root_blocks, lower_sections};
mod structural;
use structural::StructuralLowerer;
mod synopsis;
use synopsis::lower_synopsis_head;
mod flow;
mod man_nofill;
use flow::BlockState;
mod lists;
mod preformatted;
mod tables;

use lists::man::ordered::{ManListState, append_relative_continuation};
use lists::{
    ManDefinitionState, lower_man_definition as lower_man_definition_block, lower_mdoc_list,
};
use preformatted::preformatted_blocks;
use tables::{TableEmbedding, TableEmbeddingPlan, append_table_row};

fn lower_blocks(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
) -> Vec<Block> {
    let mut formatter = crate::mandoc::formatter::FormatterState::default();
    lower_blocks_with_spacing(
        nodes,
        context,
        indent_columns,
        paragraph_distance,
        true,
        &mut formatter,
    )
}

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
    let mut lowerer = BlockLowerer::new(
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        Vec::new(),
        *formatter,
    );
    lowerer.paragraph_predecessor = paragraph_predecessor;
    lowerer.push_nodes(nodes);
    lowerer.formatter.spacing = lowerer.state.spacing_enabled();
    *formatter = lowerer.formatter;
    lowerer.finish()
}

const DEFAULT_MAN_TAG_WIDTH: i32 = 7;

struct BlockLowerer<'a, 'source> {
    context: &'a LoweringContext<'source>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &'a mut u16,
    state: BlockState,
    formatter: crate::mandoc::formatter::FormatterState,
    // man(7) starts each section or relative-indent scope with a seven-column
    // hanging margin. Explicit `.TP`/`.IP` widths update it for following
    // tagged paragraphs, exactly as mandoc's terminal renderer does.
    definition_hanging_width: crate::mandoc::layout::Distance,
    split_authors: bool,
    synopsis_return_type_open: bool,
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
        Self {
            context,
            indent_columns,
            paragraph_distance,
            state: BlockState::with_output(indent_columns, spacing_enabled, output),
            formatter,
            definition_hanging_width: crate::mandoc::layout::Distance::cells(DEFAULT_MAN_TAG_WIDTH),
            split_authors: false,
            synopsis_return_type_open: false,
            man_list_state: ManListState::None,
            paragraph_predecessor: false,
        }
    }

    fn push_nodes(&mut self, nodes: &[Node]) {
        let table_plan = TableEmbeddingPlan::new(nodes, self.context);
        for (index, node) in nodes.iter().enumerate() {
            if table_plan.consumes(index) || is_inline_equation_quote_artifact(nodes, index) {
                continue;
            }
            if follows_inline_equation_punctuation(nodes, index) {
                self.state.tighten_next_boundary();
            }
            self.push(node, nodes.get(index + 1), table_plan.embedding(index));
            // Source execution, not visible output, owns the predecessor fact.
            if self.context.macro_set == libmandoc_rs::MacroSet::Mdoc
                && super::adjacency::is_logical_sibling(node)
            {
                self.paragraph_predecessor = true;
            }
        }
    }

    fn push(
        &mut self,
        node: &Node,
        next: Option<&Node>,
        table_embedding: Option<&TableEmbedding<'_>>,
    ) {
        if matches!(
            node.macro_name.as_deref(),
            Some("PP" | "HP" | "IP" | "TP" | "TQ" | "RS" | "SY")
        ) {
            self.formatter.font = FontState::new();
        }
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
        if self.push_no_fill_lines(node) {
            self.state
                .queue_targets(structural_targets, source_span(node));
            return;
        }
        self.state.flush_preformatted();
        if self.push_mdoc_synopsis_declaration(node) {
            return;
        }
        if node.flags.delimiter_close
            && participates_in_inline_flow(node)
            && self.state.paragraph.is_empty()
        {
            let tail = lower_inline_nodes(std::slice::from_ref(node), self.context.default_name);
            if append_to_last_inline_block(&mut self.state.output, &tail) {
                return;
            }
        }
        if node.macro_name.as_deref() == Some("Pp") {
            self.state.flush_paragraph();
            self.state
                .queue_targets(structural_targets, source_span(node));
            if !self.state.output.is_empty() {
                self.state.output.push(Block::VerticalSpace {
                    lines: 1,
                    source: source_span(node),
                });
            }
        } else if node.macro_name.as_deref() == Some("br") {
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
                spacing_enabled,
                formatter: &mut self.formatter,
            }
            .push(node, table_embedding);
            if restores_macro_indent(node) {
                self.state
                    .set_source_indent(self.indent_columns.macro_origin());
            }
            self.state.inherit_spacing(self.formatter.spacing);
            self.state
                .queue_targets(structural_targets, source_span(node));
            self.state.attach_pending_to_structural_output(output_start);
        }
    }

    fn finish(self) -> Vec<Block> {
        let blocks = self.state.finish();
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
