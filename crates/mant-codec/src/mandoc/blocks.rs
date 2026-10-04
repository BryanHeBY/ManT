//! Block-scope ownership: the lowering state, its single entry, and the
//! macro-family subdomains, mirroring the upstream split between the html
//! formatter core (`html.c`) and per-dialect dispatch (`mdoc_html.c`,
//! `man_html.c`, `roff_html.c`). The one source-order node walk and its
//! observable routing order live in [`walker`].

use libmandoc_rs::{
    DisplayKind,
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, Node, NodeKind, RoffMacro,
};
use mant_ir::{Block, Inline, Section};

use super::{
    LoweringContext, first_part_children,
    inline::{
        FilledBoundary, InlineBuilder, append_inline_node_with_next, is_enclosure_macro,
        lower_inline_nodes, lower_inline_nodes_with_font_state, plain_text,
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
    is_inline_equation_quote_artifact, participates_in_inline_flow,
};

mod sections;
use sections::is_section;
pub(super) use sections::lower_document_structure;
mod structural;
use structural::StructuralLowerer;
mod synopsis;
use synopsis::lower_synopsis_head;
mod flow;
mod man_links;
mod man_nofill;
use flow::BlockState;
mod lists;
mod man_macros;
mod mdoc_macros;
mod preformatted;
mod tables;
mod walker;

use lists::man::{
    adjacent_ip_run,
    ordered::{ManListState, append_relative_continuation},
};
use lists::{
    ManDefinitionState, lower_man_definition as lower_man_definition_block, lower_mdoc_list,
};
use preformatted::preformatted_blocks;
use tables::{TableEmbedding, TableEmbeddingPlan, append_table_row};

/// Everything one block-scope entry varies. Fields replace the positional
/// parameter permutations of the former `lower_blocks_*` wrapper family,
/// mirroring upstream's single walker signature (nodes, immutable context,
/// mutable formatter): flow facts belong to one entry, not to arity.
pub(super) struct ScopeFlow<'node> {
    /// Base `.in` indent for this scope.
    pub(super) indent_columns: crate::mandoc::layout::SourceIndent,
    pub(super) spacing_enabled: bool,
    /// A source sibling already produced output before this scope.
    pub(super) paragraph_predecessor: bool,
    /// Carried inline execution from a finished HEAD row into this body.
    pub(super) run_in: Option<RunInBody<'node>>,
    /// Whether returning from this scope settles the active formatter row.
    pub(super) row_boundary: FormatterRowBoundary,
    /// Actual column BODY entries surround pre/post, independently of
    /// their detached child slices and output-owner returns.
    pub(super) column_nodes: Option<ColumnBodyNodes<'node>>,
}

pub(super) struct ColumnBodyNodes<'node> {
    entry: Option<&'node Node>,
    next: Option<&'node Node>,
}

pub(super) struct RunInBody<'node> {
    pub(super) execution: crate::mandoc::inline::PreservedInlineState,
    pub(super) generated_cells: usize,
    /// Native pre-handler writes, before compact HEAD/BODY ownership splits.
    pub(super) native_generated_cells: usize,
    pub(super) generated_word: bool,
    pub(super) entry: Option<&'node Node>,
}

impl<'node> ScopeFlow<'node> {
    /// Ordinary filled scope; the caller keeps its active formatter row.
    pub(super) const fn filled(
        indent_columns: crate::mandoc::layout::SourceIndent,
        spacing_enabled: bool,
    ) -> Self {
        Self {
            indent_columns,
            spacing_enabled,
            paragraph_predecessor: false,
            run_in: None,
            row_boundary: FormatterRowBoundary::Preserve,
            column_nodes: None,
        }
    }

    /// A document or section body has a real terminal row boundary at its
    /// end: nested output owners return their active formatter row to the
    /// caller.
    pub(super) const fn settled(
        indent_columns: crate::mandoc::layout::SourceIndent,
        spacing_enabled: bool,
    ) -> Self {
        Self {
            indent_columns,
            spacing_enabled,
            paragraph_predecessor: false,
            run_in: None,
            row_boundary: FormatterRowBoundary::Settle,
            column_nodes: None,
        }
    }

    /// Column BODY post calls `term_flushln()`, while its children use the
    /// ordinary source-order block driver and persistent word state.
    pub(super) const fn column_post(
        indent_columns: crate::mandoc::layout::SourceIndent,
        spacing_enabled: bool,
        width: u16,
        origin: usize,
        last: bool,
    ) -> Self {
        Self {
            indent_columns,
            spacing_enabled,
            // print_bvspace() stops at any non-LIST_item It ancestor.
            // A detached column BODY therefore keeps that source boundary,
            // even when it has not emitted a cell block yet.
            paragraph_predecessor: true,
            run_in: None,
            row_boundary: FormatterRowBoundary::Column {
                width,
                origin,
                last,
            },
            column_nodes: None,
        }
    }

    pub(super) const fn with_column_nodes(
        mut self,
        entry: Option<&'node Node>,
        next: Option<&'node Node>,
    ) -> Self {
        self.column_nodes = Some(ColumnBodyNodes { entry, next });
        self
    }

    /// The owning macro's BODY post calls `term_newln()` (or
    /// `term_flushln()`); keep that distinct from an IR output-owner return
    /// without such a post. A detached structural body keeps its source
    /// predecessor: an empty child output buffer is not evidence that the
    /// body immediately follows a section heading.
    pub(super) const fn body_post_row_end(
        indent_columns: crate::mandoc::layout::SourceIndent,
        spacing_enabled: bool,
        paragraph_predecessor: bool,
    ) -> Self {
        Self {
            indent_columns,
            spacing_enabled,
            paragraph_predecessor,
            run_in: None,
            row_boundary: FormatterRowBoundary::Settle,
            column_nodes: None,
        }
    }
}

/// Lower one node list through a fresh output owner and return its blocks.
pub(super) fn lower_scope(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    flow: ScopeFlow<'_>,
) -> Vec<Block> {
    let mut lowerer = BlockLowerer::new(
        context,
        flow.indent_columns,
        paragraph_distance,
        flow.spacing_enabled,
        Vec::new(),
        std::mem::take(formatter),
    );
    if let Some(run_in) = flow.run_in {
        lowerer.state.inherit_run_in_execution(
            run_in.execution,
            run_in.generated_cells,
            run_in.native_generated_cells,
            run_in.generated_word,
            run_in.entry,
        );
    }
    lowerer.paragraph_predecessor = flow.paragraph_predecessor;
    if let Some(entry) = flow.column_nodes.as_ref().and_then(|nodes| nodes.entry) {
        lowerer.state.enter_column_body_node(entry);
    }
    if let FormatterRowBoundary::Column {
        width,
        origin,
        last,
    } = flow.row_boundary
    {
        lowerer.column_field = true;
        lowerer.state.begin_column_body(width, origin, last);
    }
    lowerer.push_nodes(nodes);
    lowerer.finish_into_before_column_entry(
        formatter,
        flow.row_boundary,
        flow.column_nodes.and_then(|nodes| nodes.next),
    )
}

const DEFAULT_MAN_TAG_WIDTH: i32 = 7;

/// A Rust output-owner return does not by itself end a CVS formatter row.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FormatterRowBoundary {
    Preserve,
    Settle,
    Column {
        width: u16,
        origin: usize,
        last: bool,
    },
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DisplayFillMode {
    /// Bd uses each source node's parsed `NODE_NOFILL` bit.
    NodeFlags,
    /// D1/Dl enter one literal row even though their operands lack the bit.
    SingleLine,
}

#[derive(Clone, Copy)]
struct NodeSourceContext {
    line_entered: bool,
    predecessor: bool,
    previous_is_sy: bool,
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
    // man_term.c::print_bvspace() examines source siblings, climbing only
    // through first-child RS wrappers. IR output from an outer scope does
    // not make the first child of an unrelated BODY a source successor.
    man_source_predecessor: bool,
    display_fill: Option<DisplayFillMode>,
    column_field: bool,
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
        state.formatter.execution.macro_set = context.macro_set;
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
            man_source_predecessor: false,
            display_fill: None,
            column_field: false,
        }
    }

    fn push_nodes(&mut self, nodes: &[Node]) {
        self.push_nodes_with_reference_posts(nodes, false);
    }

    fn settle_no_fill_inline(&mut self) {
        self.state.settle_no_fill_row();
    }

    fn finish_into(
        self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        row_boundary: FormatterRowBoundary,
    ) -> Vec<Block> {
        self.finish_into_before_column_entry(formatter, row_boundary, None)
    }

    fn finish_into_before_column_entry(
        mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        row_boundary: FormatterRowBoundary,
        next_column_entry: Option<&Node>,
    ) -> Vec<Block> {
        if row_boundary == FormatterRowBoundary::Settle {
            // Native BODY posts settle the field before its IR owner drains.
            // In a column, NOBREAK may leave the device row occupied afterwards.
            self.state.finish_column_nested_row();
            self.settle_no_fill_inline();
        }
        let blocks = self
            .state
            .finish_with_formatter(formatter, row_boundary, next_column_entry);
        self.context.check_gap_bounds(&blocks);
        blocks
    }
}

/// `roff_node_prev()` skips comments, `NODE_NOPRT`, and roff tokens without
/// rendered structure. Keep paragraph predecessor facts tied to that source
/// traversal rather than to IR blocks or raw child indices.
pub(super) fn is_native_transparent_sibling(node: &Node) -> bool {
    node.kind == NodeKind::Comment
        || node.flags.no_print
        || matches!(
            node.macro_token.as_ref(),
            Some(
                Roff(RoffMacro::Ft | RoffMacro::Ll | RoffMacro::Mc | RoffMacro::Po | RoffMacro::Ta)
                    | Mdoc(MdocMacro::Db | MdocMacro::Es | MdocMacro::Sm | MdocMacro::Tg)
                    | Man(ManMacro::Dt | ManMacro::Uc | ManMacro::Pd | ManMacro::At)
            )
        )
}

/// These man macros explicitly assign the formatter's macro base. Passive
/// structures such as tables and equations inherit the current `.in` position.
pub(super) fn restores_macro_indent(node: &Node) -> bool {
    matches!(
        node.macro_token.as_ref(),
        Some(Man(ManMacro::Pp
            | ManMacro::P
            | ManMacro::Lp
            | ManMacro::Hp
            | ManMacro::Tp
            | ManMacro::Tq
            | ManMacro::Ip
            | ManMacro::Rs
            | ManMacro::Sy))
    )
}
