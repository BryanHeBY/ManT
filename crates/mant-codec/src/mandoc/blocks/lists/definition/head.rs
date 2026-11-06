//! Execute definition HEAD sources before handing the formatter to BODY.

use super::super::{
    first_part_children, is_inline_equation, is_inline_equation_quote_artifact, targets,
};
use super::{
    DefinitionFlow, DefinitionHeadFlow, DefinitionHeadOutcome, InlineBuilder, LoweringContext,
    Node, NodeKind, RunInExecution,
};
use crate::mandoc::source_context::HeadNodes;
use libmandoc_rs::{MacroToken::Man, ManMacro};

pub(super) fn definition_head_nodes<'a>(
    node: &'a Node,
    content: &'a [Node],
    macro_set: libmandoc_rs::MacroSet,
    source_indent: crate::mandoc::layout::SourceIndent,
) -> HeadNodes<'a> {
    let before = if macro_set == libmandoc_rs::MacroSet::Mdoc {
        node.children
            .iter()
            .position(|child| child.kind == NodeKind::Head && child.scope_end.is_none())
            .map_or(&[][..], |index| &node.children[..index])
    } else {
        &[]
    };
    HeadNodes {
        before,
        content,
        source_origin_units: (macro_set == libmandoc_rs::MacroSet::Mdoc)
            .then(|| source_indent.physical_basic_units()),
    }
}

pub(super) fn lower_definition_head(
    head_nodes: HeadNodes<'_>,
    displaced_equations: &[&Node],
    context: &LoweringContext<'_>,
    flow: DefinitionFlow,
    native_head_field_units: Option<usize>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> DefinitionHeadOutcome {
    let head = head_nodes.content;
    let groups =
        std::iter::once(head_nodes).chain(displaced_equations.iter().map(|equation| HeadNodes {
            before: &[],
            content: std::slice::from_ref(*equation),
            source_origin_units: None,
        }));
    if flow.head.generated_cells().is_some() {
        let generated_cells = usize::from(flow.head.generated_cells().unwrap_or_default());
        // mdoc_term.c::termp_it_pre() configures the NOBREAK field before
        // the HEAD prints; a `-diag` head (the only NOBREAK run-in kind)
        // must therefore decode its own words inside that field.
        let run_in_field = match flow.head {
            DefinitionHeadFlow::RunIn { flags, .. }
                if flags.contains(crate::mandoc::inline::FieldFlag::NoBreak) =>
            {
                crate::mandoc::inline::AuthorBreakEffect::Field {
                    // The generated cells execute as a real run-in word
                    // below (geometry.gap is 0 for the same reason), so the
                    // field adds no separator of its own.
                    gap_cells: 0,
                    // mdoc_term.c::termp_it_pre() shortens rmargin to
                    // offset+width only for hang/tag-style lists; LIST_diag
                    // keeps the unshortened margin (mdoc_term.c:843-855), so
                    // this field's right bound is the page margin, which the
                    // width-agnostic IR lowering cannot know.
                    body_width_columns: u16::MAX,
                    field_width_columns: u16::MAX,
                    flags,
                }
            }
            _ => crate::mandoc::inline::AuthorBreakEffect::Line,
        };
        let mut finished = context.lower_run_in_definition_head(
            groups,
            flow.spacing_enabled,
            formatter,
            flow.head.strong_scope(),
            generated_cells,
            run_in_field,
        );
        finished.execution.last_executed_source_line = head
            .iter()
            .chain(head_nodes.before)
            .chain(displaced_equations.iter().copied())
            .filter_map(latest_source_line)
            .max();
        return DefinitionHeadOutcome {
            line: crate::mandoc::formatter::FinishedInlineLine {
                output: finished.output,
                definition_term_breaks: finished.term_breaks,
                definition_field_exited: false,
                definition_body_gap_consumed: false,
                definition_author_restarted: false,
            },
            run_in_execution: Some(RunInExecution {
                state: finished.execution,
                surviving_cells: finished.surviving_cells,
                generated_word: finished.generated_word,
            }),
        };
    }

    let mut term_builder = InlineBuilder::with_spacing(flow.spacing_enabled);
    let mut definition_field_exited = false;
    let mut definition_body_gap_consumed = false;
    let mut definition_author_restarted = false;
    let mut term_breaks = Vec::new();
    for (index, group) in groups.enumerate() {
        let finished = context.lower_inline_with_author_break_preserving_rows(
            group,
            flow.spacing_enabled,
            formatter,
            flow.head.author_break_effect(),
            native_head_field_units,
        );
        // Only the original HEAD can contain .Pp alternatives. Equations
        // displaced from it are later source operands, not term separators.
        if index == 0 {
            term_breaks = finished.definition_term_breaks;
        }
        term_builder.append(finished.output);
        definition_field_exited |= finished.definition_field_exited;
        definition_body_gap_consumed |= finished.definition_body_gap_consumed;
        definition_author_restarted |= finished.definition_author_restarted;
    }
    DefinitionHeadOutcome {
        line: crate::mandoc::formatter::FinishedInlineLine {
            output: term_builder.finish_preserving_rows(),
            definition_field_exited,
            definition_body_gap_consumed,
            definition_author_restarted,
            definition_term_breaks: term_breaks,
        },
        run_in_execution: None,
    }
}

fn latest_source_line(node: &Node) -> Option<u32> {
    std::iter::once(node.line)
        .chain(node.children.iter().filter_map(latest_source_line))
        .filter(|line| *line != 0)
        .max()
}

/// Recover inline eqn arguments that libmandoc moved from a man macro head to
/// the beginning of its owning definition body.
pub(super) fn displaced_definition_equations<'a>(
    head: &[Node],
    body: &'a [Node],
) -> (Vec<&'a Node>, &'a [Node]) {
    let Some(head_line) = head.iter().map(maximum_node_line).max() else {
        return (Vec::new(), body);
    };
    let mut equations = Vec::new();
    let mut consumed = 0;
    while let Some(candidate) = body
        .get(consumed)
        .filter(|candidate| candidate.line == head_line)
    {
        if is_inline_equation(candidate) {
            equations.push(candidate);
            consumed += 1;
            continue;
        }
        if consumed > 0 && is_inline_equation_quote_artifact(body, consumed) {
            consumed += 1;
            continue;
        }
        break;
    }
    if equations.is_empty() {
        (equations, body)
    } else {
        (equations, &body[consumed..])
    }
}

fn maximum_node_line(node: &Node) -> u32 {
    node.children
        .iter()
        .map(maximum_node_line)
        .fold(node.line, u32::max)
}

/// Preserve libmandoc's tag on a man(7) `.TP`/`.IP` head. Unlike mdoc `Fl`
/// tags, this identity lives on the structural head rather than a visible
/// inline child, so it has to be copied before lowering discards that wrapper.
pub(super) fn definition_head_anchor(node: &Node) -> Option<String> {
    targets::part_target(node, NodeKind::Head)
}

/// Return only document content from a definition macro's mixed-purpose head.
pub(in crate::mandoc::blocks::lists) fn visible_definition_head(node: &Node) -> &[Node] {
    let head = first_part_children(node, NodeKind::Head);
    match node.macro_token.as_ref() {
        Some(Man(ManMacro::Ip)) => head.first().map_or(&[], std::slice::from_ref),
        Some(Man(ManMacro::Tp | ManMacro::Tq)) => head
            .iter()
            .position(|child| child.flags.line_start)
            .map_or(&[], |visible_start| &head[visible_start..]),
        _ => head,
    }
}
