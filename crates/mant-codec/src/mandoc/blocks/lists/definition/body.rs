//! Enter and finish the original definition BODY in source execution order.

use super::super::super::{FormatterRowBoundary, RunInBody, ScopeFlow};
use super::{DefinitionFlow, DefinitionHeadFlow, Inline, Node, NodeKind, RunInExecution};

pub(super) fn finish_definition_body_execution(
    formatter: &mut crate::mandoc::formatter::FormatterState,
    man_node: bool,
) -> crate::mandoc::formatter::DefinitionBodyObservation {
    if man_node {
        // post_IP/post_TP executes term_newln() even when the BODY emitted
        // only a bare BACKAFTER and has no local cell. A fitting HEAD's
        // device row remains occupied until this actual BODY post; it must
        // close before a following PP or sibling starts another scope.
        formatter.settle_definition_head_rows();
    }
    let observed = formatter.finish_definition_body();
    if man_node {
        formatter.font.man_text_boundary(); // BODY post
    }
    observed
}

/// Enter the original BODY's source event before its pre changes offset.
/// The HEAD already owns its accepted glyphs; this entry may still close
/// its retained native HANG row and consume the independent ti register.
pub(super) fn enter_definition_body(
    node: &Node,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    flow: DefinitionFlow,
    macro_set: libmandoc_rs::MacroSet,
    terms: &[Vec<Inline>],
    head_open: bool,
) {
    let man_node = macro_set == libmandoc_rs::MacroSet::Man;
    if !man_node
        && matches!(flow.head, DefinitionHeadFlow::Detached { .. })
        && let Some(part) = node
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Body && part.scope_end.is_none())
    {
        // mdoc_term.c:314-329,764,838-862; no synthetic IR words are
        // executed here, only the original BODY's conditional term_newln.
        let mut entry_output = Vec::new();
        formatter.with_output_builder(&mut entry_output, |builder| {
            builder.observe_no_fill_source_lines(true);
            builder.begin_executed_node(part);
            builder.observe_no_fill_source_lines(false);
        });
    }
    // Native occupancy is distinct from the row already represented by
    // the HEAD. An invisible \& head cannot consume a BODY's new empty row.
    let represented_head = terms
        .iter()
        .any(|term| mant_ir::has_printable_character(term));
    formatter.begin_definition_body(
        flow.shares_pending_term_row
            && if man_node {
                formatter.definition_head_row_occupied()
            } else {
                represented_head
            }
            && head_open,
    );
}

pub(super) fn close_pending_definition_prefix(
    formatter: &mut crate::mandoc::formatter::FormatterState,
) {
    if formatter.definition_before_visible() {
        // CVS termp_it_pre() enters a new list item as a structural row.
        // That closes the outer definition's pending head/body prefix before
        // this item's HEAD is executed.
        formatter.note_definition_boundary();
        formatter.note_definition_visible();
    }
}

pub(super) fn definition_body_flow(
    node: &Node,
    body_origin: crate::mandoc::layout::SourceIndent,
    spacing_enabled: bool,
    flow: DefinitionFlow,
    run_in_execution: Option<RunInExecution>,
) -> ScopeFlow<'_> {
    if let Some(run_in) = run_in_execution {
        ScopeFlow {
            indent_columns: body_origin,
            spacing_enabled,
            paragraph_predecessor: flow.paragraph_predecessor,
            run_in: Some(RunInBody {
                execution: run_in.state,
                generated_cells: run_in.surviving_cells,
                native_generated_cells: usize::from(
                    flow.head.generated_cells().unwrap_or_default(),
                ),
                generated_word: run_in.generated_word,
                entry: node
                    .children
                    .iter()
                    .find(|part| part.kind == NodeKind::Body && part.scope_end.is_none()),
            }),
            row_boundary: FormatterRowBoundary::Settle,
            column_nodes: None,
        }
    } else {
        ScopeFlow::body_post_row_end(body_origin, spacing_enabled, flow.paragraph_predecessor)
    }
}
