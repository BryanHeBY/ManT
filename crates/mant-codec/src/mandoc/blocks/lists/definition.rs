//! Shared definition content, source ownership, and head construction.
use super::super::lower_blocks_with_predecessor_and_run_in;
use super::{
    DefinitionItem, Inline, InlineBuilder, LoweringContext, Node, NodeKind, first_part_children,
    is_inline_equation, is_inline_equation_quote_artifact, source_span, targets,
};

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
    RunIn { cells: u8, style: RunInHeadStyle },
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
            Self::RunIn { .. } => crate::mandoc::inline::AuthorBreakEffect::Line,
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

pub(super) fn definition_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    mut geometry: crate::mandoc::layout::DefinitionGeometry,
    flow: DefinitionFlow,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> DefinitionItem {
    if formatter.definition_before_visible() {
        // CVS termp_it_pre() enters a new list item as a structural row.
        // That closes the outer definition's pending head/body prefix before
        // this item's HEAD is executed.
        formatter.note_definition_boundary();
        formatter.note_definition_visible();
    }
    let head = visible_definition_head(node);
    let body = first_part_children(node, NodeKind::Body);
    let (displaced_equations, body) = displaced_definition_equations(head, body);
    let man_node = context.macro_set == libmandoc_rs::MacroSet::Man;
    if man_node {
        formatter.font.man_text_boundary(); // HEAD pre
    }
    let (
        term,
        run_in_execution,
        definition_field_exited,
        definition_body_gap_consumed,
        term_breaks,
    ) = lower_definition_head(head, &displaced_equations, context, flow, formatter);
    if man_node {
        formatter.font.man_text_boundary(); // HEAD post
        formatter.font.man_text_boundary(); // BODY pre
    }
    if definition_field_exited {
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    if definition_body_gap_consumed {
        geometry.gap = 0;
    }
    let mut terms = split_definition_terms(term, &term_breaks);
    if let Some(id) = definition_head_anchor(node) {
        if terms.is_empty() {
            terms.push(Vec::new());
        }
        terms[0].insert(0, Inline::anchor_at(id, source_span(node)));
    }
    if flow.head.generated_cells().is_some() {
        // The native generated cells execute inside the shared stream below.
        // Their surviving projection is carried by the description itself;
        // adding the static geometry gap as well would count it twice.
        geometry.gap = 0;
    }
    let body_origin = geometry.body_origin(context, node, indent_columns);
    // The BODY is executed once. Its active formatter records whether a real
    // boundary preceded the first visible word and whether the detached head
    // already accounts for an invisible first row.
    // A native HEAD may occupy a formatter cell without giving IR any term
    // that represents its row (for example, `.It \\&`). Only a rendered HEAD
    // can own the first invisible BODY row when term_newln() closes it.
    let rendered_head_row = terms.iter().any(|term| {
        mant_ir::inline_plain_text(term)
            .chars()
            .any(|character| !character.is_whitespace())
    });
    formatter.begin_definition_body(flow.shares_pending_term_row && rendered_head_row);
    let description = if let Some(run_in) = run_in_execution {
        lower_blocks_with_predecessor_and_run_in(
            body,
            context,
            body_origin,
            paragraph_distance,
            formatter.spacing_enabled(),
            flow.paragraph_predecessor,
            formatter,
            Some((run_in.state, run_in.surviving_cells, run_in.generated_word)),
            super::super::FormatterRowBoundary::Settle,
        )
    } else {
        super::super::lower_blocks_with_body_post_row_end(
            body,
            context,
            body_origin,
            paragraph_distance,
            formatter.spacing_enabled(),
            flow.paragraph_predecessor,
            formatter,
        )
    };
    let observed = formatter.finish_definition_body();
    if man_node {
        formatter.font.man_text_boundary(); // BODY post
    }
    if flow.shares_pending_term_row && observed.placement_breaks() {
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    let layout = geometry.layout(indent_columns, body_origin, &terms);
    let mut item = DefinitionItem {
        source: source_span(node),
        entry: None,
        layout,
        terms,
        description,
    };
    // A source coordinate identifies authored text, not one executed macro
    // invocation: expansion can produce the same coordinate and head several
    // times. Carry the native node identity through IR-only normalization and
    // strip it once semantic declaration grouping has consumed the witness.
    crate::definitions::mark_native_definition_owner(&mut item, std::ptr::from_ref(node) as usize);
    context
        .native_heads
        .borrow_mut()
        .groups
        .record(&item, std::ptr::from_ref(node) as usize);
    if context.macro_set == libmandoc_rs::MacroSet::Mdoc
        && let Some(role) = super::evidence::leading_role(head)
    {
        context.native_heads.borrow_mut().record(&item, role);
    }
    item
}

fn lower_definition_head(
    head: &[Node],
    displaced_equations: &[&Node],
    context: &LoweringContext<'_>,
    flow: DefinitionFlow,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> (Vec<Inline>, Option<RunInExecution>, bool, bool, Vec<usize>) {
    let groups = std::iter::once(head).chain(
        displaced_equations
            .iter()
            .map(|equation| std::slice::from_ref(*equation)),
    );
    if flow.head.generated_cells().is_some() {
        let generated_cells = usize::from(flow.head.generated_cells().unwrap_or_default());
        let (term, mut execution, term_breaks, surviving_cells, generated_word) = context
            .lower_run_in_definition_head(
                groups,
                flow.spacing_enabled,
                formatter,
                flow.head.strong_scope(),
                generated_cells,
            );
        execution.last_executed_source_line = head
            .iter()
            .chain(displaced_equations.iter().copied())
            .filter_map(latest_source_line)
            .max();
        return (
            term,
            Some(RunInExecution {
                state: execution,
                surviving_cells,
                generated_word,
            }),
            false,
            false,
            term_breaks,
        );
    }

    let mut term_builder = InlineBuilder::with_spacing(flow.spacing_enabled);
    let mut definition_field_exited = false;
    let mut definition_body_gap_consumed = false;
    let mut term_breaks = Vec::new();
    for (index, group) in groups.enumerate() {
        let (lowered, field_exited, body_gap_consumed, breaks) = context
            .lower_inline_with_author_break(
                group,
                flow.spacing_enabled,
                formatter,
                flow.head.author_break_effect(),
            );
        // Only the original HEAD can contain .Pp alternatives. Equations
        // displaced from it are later source operands, not term separators.
        if index == 0 {
            term_breaks = breaks;
        }
        term_builder.append(lowered);
        definition_field_exited |= field_exited;
        definition_body_gap_consumed |= body_gap_consumed;
    }
    (
        term_builder.finish(),
        None,
        definition_field_exited,
        definition_body_gap_consumed,
        term_breaks,
    )
}

fn latest_source_line(node: &Node) -> Option<u32> {
    std::iter::once(node.line)
        .chain(node.children.iter().filter_map(latest_source_line))
        .filter(|line| *line != 0)
        .max()
}

/// Recover inline eqn arguments that libmandoc moved from a man macro head to
/// the beginning of its owning definition body.
fn displaced_definition_equations<'a>(
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

/// Split alternatives embedded in one extended mdoc definition head.
///
/// libmandoc retains `.Pp` inside `It Xo ... Xc` as an inline child. In that
/// position it separates equivalent term spellings rather than starting a
/// new description paragraph. The IR already models such aliases as several
/// terms on one definition item, so preserve that structure explicitly.
pub(super) fn split_definition_terms(
    term: Vec<Inline>,
    alternative_breaks: &[usize],
) -> Vec<Vec<Inline>> {
    let mut terms = Vec::new();
    let mut current = Vec::new();
    let mut alternatives = alternative_breaks.iter().copied().peekable();
    for (index, node) in term.into_iter().enumerate() {
        if node == Inline::LineBreak && alternatives.peek() == Some(&index) {
            alternatives.next();
            if mant_ir::has_printable_character(&current) {
                terms.push(std::mem::take(&mut current));
            } else {
                // An executed but invisible author word (`\&`, or a
                // control-only `\p`) still owns a physical formatter row.
                // Keep that row attached to the next visible spelling rather
                // than manufacturing an empty semantic alternative that
                // renderers are required to ignore.
                current
                    .retain(|inline| !matches!(inline, Inline::Text { value } if value.is_empty()));
                current.push(Inline::LineBreak);
            }
        } else {
            current.push(node);
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

/// Preserve libmandoc's tag on a man(7) `.TP`/`.IP` head. Unlike mdoc `Fl`
/// tags, this identity lives on the structural head rather than a visible
/// inline child, so it has to be copied before lowering discards that wrapper.
fn definition_head_anchor(node: &Node) -> Option<String> {
    targets::part_target(node, NodeKind::Head)
}

/// Return only document content from a definition macro's mixed-purpose head.
///
/// This follows mandoc's own HTML and terminal renderers: `.IP` prints its
/// first head node and treats later arguments as layout, while `.TP`/`.TQ`
/// print only nodes beginning on the following input line. The distinction is
/// structural; inspecting strings such as `96u` would incorrectly remove a
/// numeric term while still leaking non-numeric width expressions.
pub(super) fn visible_definition_head(node: &Node) -> &[Node] {
    let head = first_part_children(node, NodeKind::Head);
    match node.macro_name.as_deref() {
        Some("IP") => head.first().map_or(&[], std::slice::from_ref),
        Some("TP" | "TQ") => head
            .iter()
            .position(|child| child.flags.line_start)
            .map_or(&[], |visible_start| &head[visible_start..]),
        _ => head,
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
