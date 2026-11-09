//! Execute definition HEAD sources before handing the formatter to BODY.

use super::super::{first_part_children, targets};
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
    context: &LoweringContext<'_>,
    flow: DefinitionFlow,
    native_head_field_units: Option<usize>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> DefinitionHeadOutcome {
    let head = head_nodes.content;
    let groups = std::iter::once(head_nodes);
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
            .filter_map(latest_source_line)
            .max();
        return DefinitionHeadOutcome {
            line: crate::mandoc::formatter::FinishedInlineLine {
                output: finished.output,
                definition_term_breaks: finished.term_breaks,
                definition_field_exited: false,
                definition_body_gap_consumed: false,
            },
            run_in_execution: Some(RunInExecution {
                state: finished.execution,
                surviving_cells: finished.surviving_cells,
                generated_word: finished.generated_word,
            }),
        };
    }

    let mut finished = context.lower_inline_with_author_break_preserving_rows(
        head_nodes,
        flow.spacing_enabled,
        formatter,
        flow.head.author_break_effect(),
        native_head_field_units,
    );
    let mut term_builder = InlineBuilder::with_spacing(flow.spacing_enabled);
    term_builder.append(finished.output);
    finished.output = term_builder.finish_preserving_rows();
    DefinitionHeadOutcome {
        line: finished,
        run_in_execution: None,
    }
}

fn latest_source_line(node: &Node) -> Option<u32> {
    std::iter::once(node.line)
        .chain(node.children.iter().filter_map(latest_source_line))
        .filter(|line| *line != 0)
        .max()
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
