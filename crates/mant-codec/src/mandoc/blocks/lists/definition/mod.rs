//! Shared definition content, source ownership, and head construction.
use super::super::lower_scope;
use super::{
    Block, DefinitionItem, Inline, InlineBuilder, LoweringContext, Node, NodeKind,
    first_part_children, source_span,
};
use libmandoc_rs::{MacroToken::Man, ManMacro};

#[derive(Clone, Copy)]
/// Source flow at a detached definition body, separate from its geometry.
/// mdoc It supplies a paragraph boundary; man TP/IP keep their existing
/// first-body policy instead of inferring a predecessor from the head text.
pub(super) struct DefinitionFlow {
    pub(super) spacing_enabled: bool,
    pub(super) paragraph_predecessor: bool,
    pub(super) shares_pending_term_row: bool,
    pub(super) head: DefinitionHeadFlow,
}

#[derive(Clone, Copy)]
pub(super) enum DefinitionHeadFlow {
    Detached {
        author_break_effect: crate::mandoc::inline::AuthorBreakEffect,
    },
    /// CVS inset and diagnostic lists execute HEAD, their generated separator
    /// cells, and BODY in one formatter stream instead of flushing the head.
    /// The flags are the upstream HEAD field flags (`termp_it_pre()`); inset
    /// sets none, diagnostic sets NOBREAK|BRIND without HANG.
    RunIn {
        cells: u8,
        style: RunInHeadStyle,
        flags: crate::mandoc::inline::FieldFlags,
    },
}

impl Default for DefinitionHeadFlow {
    fn default() -> Self {
        Self::Detached {
            author_break_effect: crate::mandoc::inline::AuthorBreakEffect::Line,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum RunInHeadStyle {
    Plain,
    /// Diagnostic heads execute in a scoped bold font whose previous-font
    /// side effects survive into the body.
    Strong,
}

struct RunInExecution {
    state: crate::mandoc::inline::PreservedInlineState,
    surviving_cells: usize,
    generated_word: bool,
}

struct DefinitionHeadOutcome {
    line: crate::mandoc::formatter::FinishedInlineLine,
    run_in_execution: Option<RunInExecution>,
}

impl DefinitionHeadOutcome {
    fn into_parts(
        self,
    ) -> (
        crate::mandoc::formatter::FinishedInlineLine,
        Option<RunInExecution>,
    ) {
        (self.line, self.run_in_execution)
    }
}

impl DefinitionHeadFlow {
    fn generated_cells(self) -> Option<u8> {
        match self {
            Self::Detached { .. } => None,
            Self::RunIn { cells, .. } => Some(cells),
        }
    }

    fn author_break_effect(self) -> crate::mandoc::inline::AuthorBreakEffect {
        match self {
            Self::Detached {
                author_break_effect,
            } => author_break_effect,
            Self::RunIn { cells, flags, .. } => crate::mandoc::inline::AuthorBreakEffect::Field {
                gap_cells: cells,
                // Neither inset nor diagnostic shortens the right margin
                // (termp_it_pre default arm), so no width-based overrun can
                // fire; the field decisions come from the flags alone.
                body_width_columns: u16::MAX,
                field_width_columns: u16::MAX,
                flags,
            },
        }
    }

    fn strong_scope(self) -> bool {
        matches!(
            self,
            Self::RunIn {
                style: RunInHeadStyle::Strong,
                ..
            }
        )
    }
}

mod body;
mod head;
mod layout;

use body::{
    DefinitionSession, close_pending_definition_prefix, definition_body_flow,
    enter_definition_body, finish_definition_body_execution,
};
pub(super) use head::visible_definition_head;
use head::{definition_head_nodes, displaced_definition_equations, lower_definition_head};
#[cfg(test)]
pub(super) use layout::split_definition_terms;
use layout::{
    apply_executed_definition_body_layout, carry_completed_head_rows,
    normalize_executed_definition_head, transfer_completed_empty_head_rows,
};

// Keep the native HEAD/BODY checkpoint decisions in source execution order.
pub(super) fn definition_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    mut geometry: crate::mandoc::layout::DefinitionGeometry,
    flow: DefinitionFlow,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> DefinitionItem {
    close_pending_definition_prefix(formatter);
    let head = visible_definition_head(node);
    let head_nodes = definition_head_nodes(node, head, context.macro_set, indent_columns);
    let body = first_part_children(node, NodeKind::Body);
    let (displaced_equations, body) = displaced_definition_equations(head, body);
    let man_node = context.macro_set == libmandoc_rs::MacroSet::Man;
    if man_node {
        formatter.font.man_text_boundary(); // HEAD pre
        // pre_IP/pre_TP(HEAD) assign mt->offset after BLOCK print_bvspace.
        super::super::reset_man_macro_offset(formatter, indent_columns);
    }
    let session = DefinitionSession::enter(node, formatter, man_node);
    let (finished, run_in_execution) = lower_definition_head(
        head_nodes,
        &displaced_equations,
        context,
        flow,
        geometry.native_head_field_units,
        formatter,
    )
    .into_parts();
    let completed_head_rows = formatter.definition_head_completed_empty_rows();
    session.restore_head(formatter);
    // TERMP_NONEWLINE survives the HEAD output drain. It is the execution
    // evidence that the first no-fill BODY row still belongs on that line.
    let head_source_continues = formatter.execution.source_row_continues();
    if man_node {
        formatter.font.man_text_boundary(); // HEAD post
        formatter.font.man_text_boundary(); // BODY pre
        // BODY uses its declared tag offset, without the HEAD's temporary
        // correction. The independent ti still closes at device endline.
        formatter.row_presentation.offset_shift = 0;
        formatter.enter_man_definition_body();
    }
    if finished.definition_field_exited && !man_node {
        // man tag width fitting remains responsive IR geometry. The actual
        // HEAD post's device close controls formatter registers below, but
        // a width-only overrun does not author a hard layout boundary.
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    if finished.definition_body_gap_consumed {
        geometry.gap = 0;
    }
    let (mut terms, closed_head_row) = normalize_executed_definition_head(
        finished.output,
        &finished.definition_term_breaks,
        node,
        flow,
        &mut geometry,
    );
    let empty_head_rows = transfer_completed_empty_head_rows(&mut terms, completed_head_rows);
    let body_origin = geometry.body_origin(context, node, indent_columns);
    enter_definition_body(
        node,
        formatter,
        flow,
        body_origin,
        context.macro_set,
        &terms,
        !closed_head_row && !finished.definition_field_exited,
    );
    let deferred_head_projection = run_in_execution
        .as_ref()
        .and_then(|run_in| run_in.state.deferred_head_projection());
    let spacing_enabled = formatter.spacing_enabled();
    let mut description = lower_scope(
        body,
        context,
        paragraph_distance,
        formatter,
        definition_body_flow(node, body_origin, spacing_enabled, flow, run_in_execution),
    );
    if let Some(receiver) = deferred_head_projection
        && let Some(captured) = receiver.apply(&mut terms)
    {
        capture_deferred_operands(context, head, captured);
    }
    carry_completed_head_rows(
        node,
        &terms,
        closed_head_row,
        empty_head_rows,
        &mut description,
    );
    if node.macro_token.as_ref() == Some(&Man(ManMacro::Ip)) {
        // man_term.c::post_IP() can complete an empty HEAD word even though
        // it supplies no tag. Its row now belongs to the description; an
        // empty term shell must not turn a headless .IP continuation into a
        // new semantic definition.
        terms.retain(|term| !term.is_empty());
    }
    let observed = finish_definition_body_execution(formatter, man_node);
    apply_executed_definition_body_layout(
        &mut geometry,
        flow,
        closed_head_row,
        finished.definition_field_exited,
        &observed,
        head_source_continues,
        &description,
    );
    let layout = geometry.layout(indent_columns, body_origin, &terms);
    let item = assemble_definition_item(terms, description, layout, node, head, context);
    session.finish(formatter);
    item
}

fn capture_deferred_operands(
    context: &LoweringContext<'_>,
    head: &[Node],
    captured: crate::definitions::CapturedHeadOperands,
) {
    context
        .native_heads
        .borrow_mut()
        .capture_operands(head.as_ptr() as usize, captured);
}

fn finalize_definition_term(mut content: Vec<Inline>) -> mant_ir::DefinitionTerm {
    crate::mandoc::inline::prepare_inline_output(&mut content);
    crate::mandoc::inline::strip_native_projection_markers(&mut content);
    let inline_layout = crate::mandoc::inline::take_inline_layout(&mut content);
    mant_ir::DefinitionTerm {
        content,
        inline_layout,
    }
}

fn assemble_definition_item(
    terms: Vec<Vec<Inline>>,
    description: Vec<Block>,
    (head_body_relation, layout): (mant_ir::HeadBodyRelation, mant_ir::DefinitionLayout),
    node: &Node,
    head: &[Node],
    context: &LoweringContext<'_>,
) -> DefinitionItem {
    let mut item = DefinitionItem {
        head_body_relation,
        source: source_span(node),
        entry: None,
        layout,
        terms: terms.into_iter().map(finalize_definition_term).collect(),
        description,
    };
    record_definition_item(&mut item, node, head, context);
    item
}

fn record_definition_item(
    item: &mut DefinitionItem,
    node: &Node,
    head: &[Node],
    context: &LoweringContext<'_>,
) {
    // A source coordinate identifies authored text, not one executed macro
    // invocation: expansion can produce the same coordinate and head several
    // times. Carry the native node identity through IR-only normalization and
    // strip it once semantic declaration grouping has consumed the witness.
    crate::definitions::mark_native_definition_owner(item, std::ptr::from_ref(node) as usize);
    context
        .native_heads
        .borrow_mut()
        .groups
        .record(item, std::ptr::from_ref(node) as usize);
    context
        .native_heads
        .borrow_mut()
        .record_operands(item, head.as_ptr() as usize);
    if context.macro_set == libmandoc_rs::MacroSet::Mdoc
        && let Some(role) = super::evidence::leading_role(head)
    {
        context.native_heads.borrow_mut().record(item, role);
    }
}

/// Combining heads changes the owner start as well as its displayed terms.
/// Retain the first head's actual source (including unknown), without inventing
/// an end position from a later head or borrowing the body/container location.
pub(super) fn prepend_definition_heads(
    item: &mut DefinitionItem,
    mut heads: impl Iterator<Item = DefinitionItem>,
) {
    if let Some(first) = heads.next() {
        item.source = first.source;
        item.terms.splice(
            0..0,
            std::iter::once(first)
                .chain(heads)
                .flat_map(|head| head.terms),
        );
    }
}
