//! Shared definition content, source ownership, and head construction.
use super::super::{FormatterRowBoundary, RunInBody, ScopeFlow, lower_scope};
use super::{
    Block, DefinitionItem, Inline, InlineBuilder, LoweringContext, Node, NodeKind,
    first_part_children, is_inline_equation, is_inline_equation_quote_artifact, source_span,
    targets,
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
        _author_restarted,
        term_breaks,
    ) = lower_definition_head(
        head,
        &displaced_equations,
        context,
        flow,
        geometry.native_head_field_units,
        formatter,
    );
    let completed_head_rows = formatter.definition_head_completed_empty_rows();
    // TERMP_NONEWLINE survives the HEAD output drain. It is the execution
    // evidence that the first no-fill BODY row still belongs on that line.
    let head_source_continues = formatter.execution.source_row_continues();
    if man_node {
        formatter.font.man_text_boundary(); // HEAD post
        formatter.font.man_text_boundary(); // BODY pre
        formatter.enter_man_definition_body();
    }
    if definition_field_exited && !man_node {
        // man tag width fitting remains responsive IR geometry. The actual
        // HEAD post's device close controls formatter registers below, but
        // a width-only overrun does not author a hard layout boundary.
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    if definition_body_gap_consumed {
        geometry.gap = 0;
    }
    let (mut terms, closed_head_row) =
        normalize_executed_definition_head(term, &term_breaks, node, flow, &mut geometry);
    let empty_head_rows = transfer_completed_empty_head_rows(&mut terms, completed_head_rows);
    let body_origin = geometry.body_origin(context, node, indent_columns);
    // The BODY is executed once. Its active formatter records whether a real
    // boundary preceded the first visible word and whether the detached head
    // already accounts for an invisible first row.
    // A native HEAD may occupy a formatter cell without giving IR any term
    // that represents its row (for example, `.It \\&`). Only a rendered HEAD
    // can own the first invisible BODY row when term_newln() closes it.
    let rendered_head_row = terms
        .iter()
        .any(|term| mant_ir::has_printable_character(term));
    formatter.begin_definition_body(
        flow.shares_pending_term_row
            && if man_node {
                formatter.definition_head_row_occupied()
            } else {
                rendered_head_row
            }
            && !closed_head_row
            && !definition_field_exited,
    );
    let spacing_enabled = formatter.spacing_enabled();
    let mut description = lower_scope(
        body,
        context,
        paragraph_distance,
        formatter,
        definition_body_flow(node, body_origin, spacing_enabled, flow, run_in_execution),
    );
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
    apply_executed_definition_body_layout(
        &mut geometry,
        flow,
        closed_head_row,
        definition_field_exited,
        observed,
        head_source_continues,
        &description,
    );
    let layout = geometry.layout(indent_columns, body_origin, &terms);
    let mut item = DefinitionItem {
        source: source_span(node),
        entry: None,
        layout,
        terms: terms.into_iter().map(finalize_definition_term).collect(),
        description,
    };
    record_definition_item(&mut item, node, head, context);
    item
}

fn finalize_definition_term(mut content: Vec<Inline>) -> mant_ir::DefinitionTerm {
    let inline_layout = crate::mandoc::inline::take_inline_layout(&mut content);
    mant_ir::DefinitionTerm {
        content,
        inline_layout,
    }
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

fn definition_body_flow(
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

fn apply_executed_definition_body_layout(
    geometry: &mut crate::mandoc::layout::DefinitionGeometry,
    flow: DefinitionFlow,
    closed_head_row: bool,
    definition_field_exited: bool,
    observed: crate::mandoc::formatter::DefinitionBodyObservation,
    head_source_continues: bool,
    description: &[Block],
) {
    if flow.shares_pending_term_row && observed.placement_breaks() {
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    // NOSPACE describes the next word, not the relation to a previous
    // physical row. A closed HEAD stays separate even if pre_br left that
    // register armed (roff_term.c:69-78, mdoc_term.c::termp_it_post()).
    if !closed_head_row && !definition_field_exited && observed.first_word_flushed_at_body() {
        // The cleared field filled its capacity (term.c:250-253 with
        // 205-207): the body shares the head's row starting at the
        // description column, with no separator cell to count.
        geometry.relation_override = Some(mant_ir::HeadBodyRelation::joined(
            mant_ir::DefinitionBodyAlignment::Indented,
        ));
        geometry.gap = 0;
    } else if !closed_head_row && !definition_field_exited && observed.first_word_concatenated() {
        // TERMP_NOSPACE at the body's first word leaves no separator cell:
        // the body column starts at the head's end (roff_term.c:75-78),
        // so the layout carries no minimum gap.
        geometry.relation_override = Some(mant_ir::HeadBodyRelation::joined(
            mant_ir::DefinitionBodyAlignment::AfterTerm,
        ));
        geometry.gap = 0;
    }
    if matches!(description.first(), Some(Block::Preformatted { .. }))
        && !observed
            .source_continues_after_run_in()
            .unwrap_or(head_source_continues)
    {
        // A literal BODY can run in only when CVS kept the source row open
        // with \\c. Ordinary no-fill NODE_LINE starts a fresh physical row.
        // That executed close also supersedes a provisional relation from
        // the first word's NOSPACE/column state: a word register cannot join
        // a row that print_mdoc_node() already ended (mdoc_term.c:314-318).
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
        geometry.relation_override = None;
    }
}

fn normalize_executed_definition_head(
    term: Vec<Inline>,
    term_breaks: &[usize],
    node: &Node,
    flow: DefinitionFlow,
    geometry: &mut crate::mandoc::layout::DefinitionGeometry,
) -> (Vec<Vec<Inline>>, bool) {
    let mut terms = split_definition_terms(term, term_breaks);
    if matches!(flow.head, DefinitionHeadFlow::Detached { .. }) {
        for term in &mut terms {
            // CVS term.c::term_fill() drops a field made solely of ordinary
            // breakable spaces. Such a HEAD owns no printed row, so an early
            // BODY .sp must not make its IR term appear as another blank row.
            // Fixed blanks (\~ and \0) remain real cells in the UTF-8 device.
            if term.iter().all(only_breakable_head_padding) {
                clear_breakable_head_padding(term);
            }
        }
    }
    if let Some(id) = definition_head_anchor(node) {
        if terms.is_empty() {
            terms.push(Vec::new());
        }
        terms[0].insert(0, Inline::anchor_at(id, source_span(node)));
    }
    // Only an executed line boundary can close the HEAD row. An empty
    // operand without NODE_LINE still runs term_word(), not term_vspace()
    // (mdoc_term.c:354-378); later BODY words consume that same raw field.
    let closed_head_row = take_closed_head_row(&mut terms);
    if closed_head_row {
        // The last explicit HEAD break becomes the term/BODY separation.
        // Earlier breaks, including extra sp rows, remain inside the term.
        // Heads whose in-word `\p` or armed trailing marker closed the
        // native row follow the same term_flushln() tail rule
        // (term.c:250-252).
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    if flow.head.generated_cells().is_some() {
        // The native generated cells execute inside the shared stream below.
        // Their surviving projection is carried by the description itself;
        // adding the static geometry gap as well would count it twice.
        geometry.gap = 0;
    }
    (terms, closed_head_row)
}

fn only_breakable_head_padding(inline: &Inline) -> bool {
    match inline {
        Inline::Anchor { .. } | Inline::LineBreak { .. } => true,
        Inline::Text { value } | Inline::Code { value } => value.chars().all(|ch| ch == ' '),
        Inline::Emphasis { children }
        | Inline::Strong { children }
        | Inline::Link { children, .. } => children.iter().all(only_breakable_head_padding),
        // Equations and other semantic nodes are not ordinary term_fill()
        // padding cells.
        Inline::Equation { .. } => false,
    }
}

fn clear_breakable_head_padding(term: &mut Vec<Inline>) {
    term.retain_mut(|inline| match inline {
        Inline::Anchor { .. } | Inline::LineBreak { .. } => true,
        Inline::Link { children, .. } => {
            // Preserve the typed destination while removing its unprinted
            // whitespace label. Link resolution may later unwrap an unknown
            // section target; an empty child cannot revive the field row.
            clear_breakable_head_padding(children);
            true
        }
        Inline::Emphasis { children } | Inline::Strong { children } => {
            clear_breakable_head_padding(children);
            !children.is_empty()
        }
        Inline::Text { .. } | Inline::Code { .. } => false,
        Inline::Equation { .. } => unreachable!("padding predicate excludes equations"),
    });
}

fn take_closed_head_row(terms: &mut [Vec<Inline>]) -> bool {
    // Semantic wrappers cannot change term_newln()/endline() output. Move
    // exactly one final executed boundary into the structural HEAD/BODY
    // relation, preserving earlier blank rows and every authored target.
    // Pinned mdoc_term.c::termp_it_post(), term.c::term_flushln().
    fn take_boundary(nodes: &mut Vec<Inline>) -> Option<bool> {
        let mut index = nodes.len();
        while index > 0 {
            index -= 1;
            match &mut nodes[index] {
                Inline::Anchor { .. } => {}
                Inline::LineBreak { .. } => {
                    nodes.remove(index);
                    return Some(true);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    if let Some(closed) = take_boundary(children) {
                        return Some(closed);
                    }
                }
                // Even an empty Text can represent a newly occupied native
                // cell. It is not identity metadata to skip over.
                _ => return Some(false),
            }
        }
        None
    }

    let Some(term) = terms.last_mut() else {
        return false;
    };
    take_boundary(term) == Some(true)
}

fn invisible_closed_head_row(terms: &[Vec<Inline>]) -> bool {
    terms.last().is_some_and(|term| {
        !term.iter().any(|inline| {
            matches!(inline, Inline::LineBreak { .. })
                || mant_ir::has_printable_character(std::slice::from_ref(inline))
        })
    })
}

fn transfer_completed_empty_head_rows(terms: &mut [Vec<Inline>], rows: u16) -> u16 {
    if rows == 0
        || terms
            .iter()
            .any(|term| mant_ir::has_printable_character(term))
    {
        return 0;
    }
    // term_vspace() already executed each endline (term.c:489-497). An
    // otherwise empty term has no glyph row to render their last boundary.
    // Deliver the receipt once to BODY layout; neither an implicit term row
    // nor its retained LineBreak projection may account for the same rows.
    // Keep every authored identity even when its HEAD has no readable label.
    for term in terms {
        crate::mandoc::inline::retain_inline_identities(term);
    }
    rows
}

fn carry_completed_head_rows(
    node: &Node,
    terms: &[Vec<Inline>],
    closed_head_row: bool,
    completed_empty_rows: u16,
    description: &mut Vec<Block>,
) {
    let rows = if completed_empty_rows > 0 {
        completed_empty_rows
    } else {
        u16::from(closed_head_row && invisible_closed_head_row(terms))
    };
    if rows > 0 {
        // term_newln() flushed a HEAD cell, but an invisible term has no IR
        // renderer. Transfer its completed physical row to the BODY owner.
        description.insert(
            0,
            Block::VerticalSpace {
                lines: rows,
                source: source_span(node),
            },
        );
    }
}

fn lower_definition_head(
    head: &[Node],
    displaced_equations: &[&Node],
    context: &LoweringContext<'_>,
    flow: DefinitionFlow,
    native_head_field_units: Option<usize>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> (
    Vec<Inline>,
    Option<RunInExecution>,
    bool,
    bool,
    bool,
    Vec<usize>,
) {
    let groups = std::iter::once(head).chain(
        displaced_equations
            .iter()
            .map(|equation| std::slice::from_ref(*equation)),
    );
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
        let (term, mut execution, term_breaks, surviving_cells, generated_word) = context
            .lower_run_in_definition_head(
                groups,
                flow.spacing_enabled,
                formatter,
                flow.head.strong_scope(),
                generated_cells,
                run_in_field,
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
            false,
            term_breaks,
        );
    }

    let mut term_builder = InlineBuilder::with_spacing(flow.spacing_enabled);
    let mut definition_field_exited = false;
    let mut definition_body_gap_consumed = false;
    let mut definition_author_restarted = false;
    let mut term_breaks = Vec::new();
    for (index, group) in groups.enumerate() {
        let (lowered, field_exited, body_gap_consumed, author_restarted, breaks) = context
            .lower_inline_with_author_break_preserving_rows(
                group,
                flow.spacing_enabled,
                formatter,
                flow.head.author_break_effect(),
                native_head_field_units,
            );
        // Only the original HEAD can contain .Pp alternatives. Equations
        // displaced from it are later source operands, not term separators.
        if index == 0 {
            term_breaks = breaks;
        }
        term_builder.append(lowered);
        definition_field_exited |= field_exited;
        definition_body_gap_consumed |= body_gap_consumed;
        definition_author_restarted |= author_restarted;
    }
    (
        term_builder.finish_preserving_rows(),
        None,
        definition_field_exited,
        definition_body_gap_consumed,
        definition_author_restarted,
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
        if node == Inline::line_break() && alternatives.peek() == Some(&index) {
            alternatives.next();
            if mant_ir::has_printable_character(&current) {
                let origin = crate::mandoc::inline::split_row_origin(&mut current);
                terms.push(std::mem::take(&mut current));
                current.extend(origin);
            } else {
                // An executed but invisible author word (`\&`, or a
                // control-only `\p`) still owns a physical formatter row.
                // Keep that row attached to the next visible spelling rather
                // than manufacturing an empty semantic alternative that
                // renderers are required to ignore.
                current
                    .retain(|inline| !matches!(inline, Inline::Text { value } if value.is_empty()));
                current.push(Inline::line_break());
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
pub(super) fn visible_definition_head(node: &Node) -> &[Node] {
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
