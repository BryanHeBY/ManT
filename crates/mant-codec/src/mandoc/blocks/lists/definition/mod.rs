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

pub(super) struct LoweredDefinitionItem {
    pub(super) item: DefinitionItem,
    pub(super) ordinal_label_can_be_replaced: bool,
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
use head::{definition_head_nodes, lower_definition_head};
#[cfg(test)]
pub(super) use layout::split_definition_terms;
use layout::{
    apply_executed_definition_body_layout, apply_retired_head_gap, carry_completed_head_rows,
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
) -> LoweredDefinitionItem {
    close_pending_definition_prefix(formatter);
    let head = visible_definition_head(node);
    let head_nodes = definition_head_nodes(node, head, context.macro_set, indent_columns);
    let body = first_part_children(node, NodeKind::Body);
    let man_node = context.macro_set == libmandoc_rs::MacroSet::Man;
    let session = enter_definition_item(node, formatter, man_node, indent_columns);
    let (finished, run_in_execution) = lower_definition_head(
        head_nodes,
        context,
        flow,
        geometry.native_head_field_units,
        formatter,
    )
    .into_parts();
    let (completed_head_rows, carried_head_rows) = formatter.definition_head_completed_empty_rows();
    session.restore_head(formatter);
    // TERMP_NONEWLINE survives the HEAD output drain. It is the execution
    // evidence that the first no-fill BODY row still belongs on that line.
    let head_source_continues = formatter.execution.source_row_continues();
    if man_node {
        handoff_man_definition_head(formatter);
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
        .and_then(|run_in| run_in.state.deferred_head_projection())
        .inspect(|receiver| receiver.suspend_terms(&mut terms));
    let spacing_enabled = formatter.spacing_enabled();
    let mut description = lower_scope(
        body,
        context,
        paragraph_distance,
        formatter,
        definition_body_flow(node, body_origin, spacing_enabled, flow, run_in_execution),
    );
    if let Some(receiver) = &deferred_head_projection
        && let Some(captured) = receiver.resume_terms(&mut terms)
    {
        capture_deferred_operands(context, head, captured);
    }
    let retired_head_gap = retire_covered_head_ink(
        formatter,
        &mut terms,
        context,
        head,
        (indent_columns, body_origin, geometry.gap),
    );
    let observed = finish_definition_body_execution(formatter, man_node);
    finish_definition_body_layout(
        (node, &mut terms, &mut geometry, &mut description),
        (flow, closed_head_row, finished.definition_field_exited),
        (head_source_continues, deferred_head_projection.is_some()),
        (empty_head_rows, carried_head_rows, retired_head_gap),
        &observed,
    );
    let layout = geometry.layout(indent_columns, body_origin, &terms);
    let item = assemble_definition_item(
        terms,
        description,
        layout,
        node,
        head,
        context,
        closed_head_row,
    );
    session.finish(formatter);
    LoweredDefinitionItem {
        item,
        ordinal_label_can_be_replaced: !closed_head_row
            && empty_head_rows == 0
            && !finished.head_layout_must_survive,
    }
}

fn finish_definition_body_layout(
    (node, terms, geometry, description): (
        &Node,
        &mut Vec<Vec<Inline>>,
        &mut crate::mandoc::layout::DefinitionGeometry,
        &mut Vec<Block>,
    ),
    (flow, closed_head_row, field_exited): (DefinitionFlow, bool, bool),
    (head_source_continues, has_deferred_head): (bool, bool),
    (empty_head_rows, carried_head_rows, retired_head_gap): (u16, u16, Option<u16>),
    observed: &crate::mandoc::formatter::DefinitionBodyObservation,
) {
    carry_completed_head_rows(
        node,
        terms,
        closed_head_row || has_deferred_head && observed.term_carried_row_close(),
        empty_head_rows,
        carried_head_rows,
        description,
    );
    if node.macro_token.as_ref() == Some(&Man(ManMacro::Ip)) {
        finish_man_ip_head(terms, geometry, field_exited);
    }
    apply_executed_definition_body_layout(
        geometry,
        flow,
        closed_head_row,
        field_exited,
        observed,
        head_source_continues,
        description,
    );
    apply_retired_head_gap(
        geometry,
        closed_head_row,
        field_exited,
        observed,
        retired_head_gap,
    );
}

fn finish_man_ip_head(
    terms: &mut Vec<Vec<Inline>>,
    geometry: &mut crate::mandoc::layout::DefinitionGeometry,
    field_exited: bool,
) {
    use crate::mandoc::layout::TermPlacement;

    // post_IP() can complete an empty HEAD word. Its row now belongs to
    // the description, including in a headless continuation.
    terms.retain(|term| !term.is_empty());
    if matches!(geometry.placement, TermPlacement::Fit)
        && !field_exited
        && terms
            .last()
            .is_some_and(|term| matches!(mant_ir::last_visible_character(term), Some(' ' | '\t')))
    {
        // man_term.c::pre_IP() does not select BRTRSP. term_flushln()
        // ignores ordinary trailing cells for fit, including the spaces
        // kept in the quoted operand's IR. Follow the native field receipt
        // on this boundary instead of measuring the whole authored tail.
        geometry.placement = TermPlacement::RunIn;
    }
}

fn enter_definition_item(
    node: &Node,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    man_node: bool,
    indent_columns: crate::mandoc::layout::SourceIndent,
) -> DefinitionSession {
    if man_node {
        formatter.font.man_text_boundary(); // HEAD pre
        // pre_IP/pre_TP(HEAD) assign mt->offset after BLOCK print_bvspace.
        super::super::reset_man_macro_offset(formatter, indent_columns);
    }
    DefinitionSession::enter(node, formatter, man_node)
}

fn handoff_man_definition_head(formatter: &mut crate::mandoc::formatter::FormatterState) {
    formatter.font.man_text_boundary(); // HEAD post
    formatter.font.man_text_boundary(); // BODY pre
    // BODY uses its declared tag offset, without the HEAD's temporary
    // correction. The independent ti still closes at device endline.
    formatter.row_presentation.offset_shift = 0;
    formatter.enter_man_definition_body();
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

// The device receipt proves the final single-cell glyphs were overpainted
// by the BODY. Retire their spelling within the original styled owners.
// When `term.c:434` subtracted a zero-width preceding cell (e.g. NBRZW)
// during a HEAD backspace, the device `viscol` can exceed the surviving
// term width; return the exact native gap so layout preserves it.
fn retire_covered_head_ink(
    formatter: &mut crate::mandoc::formatter::FormatterState,
    terms: &mut [Vec<Inline>],
    context: &LoweringContext<'_>,
    head: &[Node],
    (indent_columns, body_origin, min_gap): (
        crate::mandoc::layout::SourceIndent,
        crate::mandoc::layout::SourceIndent,
        u16,
    ),
) -> Option<u16> {
    if let Some((covered, viscol, minbl)) = formatter.execution.take_covered_head_ink()
        && let Some(term) = terms.last_mut()
    {
        let mut candidate = term.clone();
        if retire_painted_suffix(&mut candidate, &covered) == Some("") {
            *term = candidate;
            context
                .native_heads
                .borrow_mut()
                .retire_painted_suffix(head.as_ptr() as usize, &covered);
            let body_units = body_origin
                .source_basic_units()
                .saturating_sub(indent_columns.source_basic_units());
            let native_gap = body_units
                .saturating_sub(viscol.saturating_mul(24))
                .max(minbl.saturating_mul(24))
                .saturating_add(11)
                / 24;
            let term_width =
                mant_ir::geometry::text_width(&crate::mandoc::inline::plain_text(term));
            let body_indent =
                usize::try_from(body_origin.offset_from(indent_columns).max(0)).unwrap_or(0);
            let standard_gap = body_indent
                .saturating_sub(term_width)
                .max(usize::from(min_gap));
            if native_gap != standard_gap {
                return Some(u16::try_from(native_gap).unwrap_or(u16::MAX));
            }
        }
    }
    None
}

fn retire_painted_suffix<'a>(nodes: &mut [Inline], mut suffix: &'a str) -> Option<&'a str> {
    for node in nodes.iter_mut().rev() {
        if suffix.is_empty() {
            break;
        }
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                let covered = value.len().min(suffix.len());
                let start = value.len() - covered;
                let rest = suffix.len() - covered;
                if value.get(start..)? != &suffix[rest..] {
                    return None;
                }
                value.truncate(start);
                suffix = &suffix[..rest];
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => suffix = retire_painted_suffix(children, suffix)?,
            Inline::Anchor { .. } => {}
            Inline::LineBreak {} | Inline::Equation { .. } => return None,
        }
    }
    Some(suffix)
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
    retired_break: bool,
) -> DefinitionItem {
    let mut item = DefinitionItem {
        head_body_relation,
        source: source_span(node),
        entry: None,
        layout,
        terms: terms.into_iter().map(finalize_definition_term).collect(),
        description,
    };
    record_definition_item(&mut item, node, head, context, retired_break);
    item
}

fn record_definition_item(
    item: &mut DefinitionItem,
    node: &Node,
    head: &[Node],
    context: &LoweringContext<'_>,
    retired_break: bool,
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
        .record_operands(item, head.as_ptr() as usize, retired_break);
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
