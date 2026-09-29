//! Shared definition content, source ownership, and head construction.
use super::super::{FormatterRowBoundary, ScopeFlow, lower_scope};
use super::{
    Block, DefinitionItem, Inline, InlineBuilder, LoweringContext, Node, NodeKind,
    first_part_children, is_inline_equation, is_inline_equation_quote_artifact, source_span,
    targets,
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
#[allow(clippy::too_many_lines)]
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
    // TERMP_NONEWLINE survives the HEAD output drain. It is the execution
    // evidence that the first no-fill BODY row still belongs on that line.
    let head_source_continues = formatter.execution.source_row_continues();
    // mdoc_macro.c::blk_exp_close() marks the original block BROKEN when a
    // later explicit end closes its formatting scope. The terminal renderer
    // never reads that flag (no NODE_BROKEN reference in mdoc_term.c or
    // term.c); it only witnesses the row through executed requests. The
    // body's `.br` closes the head row once (roff_term_pre_br term_newln);
    // a SECOND row appears only in no-fill mode, where print_mdoc_node()
    // runs another term_newln() at the next NODE_LINE
    // (mdoc_term.c:314-317 with 361-369). Fill mode joins the open row
    // instead (mdoc.c:238-250 cancels the continuation there).
    let closed_head_scope = (formatter.no_fill || body.iter().any(node_entered_no_fill))
        && head.iter().any(native_broken_head_scope)
        && !head_source_continues;
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
    let empty_text_closes_run_in = flow.head.generated_cells().is_some()
        && head.iter().any(has_empty_text_child)
        && terms
            .last()
            .is_some_and(|term| mant_ir::has_printable_character(term))
        && matches!(
            flow.head,
            DefinitionHeadFlow::RunIn { flags, .. }
                if !flags.contains(crate::mandoc::inline::FieldFlag::Hang)
                    && !flags.contains(crate::mandoc::inline::FieldFlag::NoBreak)
        );
    // term.c:250-252 with 347-350: the generated body separator is a
    // non-breaking space, so an armed trailing `\p` alone keeps the field
    // (and the joining BODY) on one row. A separate empty TEXT node runs
    // term_newln() mid-HEAD (NODE_LINE); that flush prints its prefix, and
    // a field without NOBREAK or HANG (inset) closes the row before BODY.
    let closed_head_row = take_closed_head_row(&mut terms)
        || marker_split_field_exited(flow.head, &terms)
        || empty_text_closes_run_in;
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
            && rendered_head_row
            && !closed_head_row
            && !definition_field_exited
            && !closed_head_scope,
    );
    let spacing_enabled = formatter.spacing_enabled();
    // Compute before the run-in state is moved into the body scope below.
    let discarded_run_in_suffix = run_in_execution
        .as_ref()
        .is_some_and(|run_in| run_in.state.definition_suffix_discarded);
    let mut description = if let Some(run_in) = run_in_execution {
        lower_scope(
            body,
            context,
            paragraph_distance,
            formatter,
            ScopeFlow {
                indent_columns: body_origin,
                spacing_enabled,
                paragraph_predecessor: flow.paragraph_predecessor,
                run_in: Some((run_in.state, run_in.surviving_cells, run_in.generated_word)),
                row_boundary: FormatterRowBoundary::Settle,
            },
        )
    } else {
        lower_scope(
            body,
            context,
            paragraph_distance,
            formatter,
            ScopeFlow::body_post_row_end(body_origin, spacing_enabled, flow.paragraph_predecessor),
        )
    };
    // term_flushln() clears the whole unflushed buffer when a pass rejects
    // (term.c:144-146 with 235): for a run-in HEAD whose in-word `\p`
    // discarded the suffix, the late flush still holds the generated
    // separator and the first BODY text, so that content never prints.
    if discarded_run_in_suffix && matches!(description.first(), Some(Block::Paragraph { .. })) {
        description.remove(0);
    }
    carry_invisible_head_row(node, &terms, closed_head_row, &mut description);
    if node.macro_name.as_deref() == Some("IP") {
        // man_term.c::post_IP() can complete an empty HEAD word even though
        // it supplies no tag. Its row now belongs to the description; an
        // empty term shell must not turn a headless .IP continuation into a
        // new semantic definition.
        terms.retain(|term| !term.is_empty());
    }
    let observed = formatter.finish_definition_body();
    if man_node {
        formatter.font.man_text_boundary(); // BODY post
    }
    if flow.shares_pending_term_row && observed.placement_breaks() {
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    if observed.first_word_flushed_at_body() {
        // The cleared field filled its capacity (term.c:250-253 with
        // 205-207): the body shares the head's row starting at the
        // description column, with no separator cell to count.
        geometry.relation_override = Some(mant_ir::HeadBodyRelation::FlushAtBody);
        geometry.gap = 0;
    } else if observed.first_word_concatenated() {
        // TERMP_NOSPACE at the body's first word leaves no separator cell:
        // the body column starts at the head's end (roff_term.c:75-78),
        // so the layout carries no minimum gap.
        geometry.relation_override = Some(mant_ir::HeadBodyRelation::JoinedNoSpace);
        geometry.gap = 0;
    }
    if matches!(description.first(), Some(Block::Preformatted { .. }))
        && !observed
            .source_continues_after_run_in()
            .unwrap_or(head_source_continues)
    {
        // A literal BODY can run in only when CVS kept the source row open
        // with \\c. Ordinary no-fill NODE_LINE starts a fresh physical row.
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

fn native_broken_head_scope(node: &Node) -> bool {
    (node.kind == NodeKind::Block && node.flags.broken)
        || node.children.iter().any(native_broken_head_scope)
}

/// The extra no-fill row decision needs the fill mode the BODY's first
/// executed word runs in; an `.nf` inside the HEAD does not survive the
/// head session's state snapshot, so read it from the parsed nodes.
fn node_entered_no_fill(node: &Node) -> bool {
    node.flags.no_fill || node.children.iter().any(node_entered_no_fill)
}

fn only_breakable_head_padding(inline: &Inline) -> bool {
    match inline {
        Inline::Anchor { .. } | Inline::LineBreak => true,
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
        Inline::Anchor { .. } | Inline::LineBreak => true,
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
    let Some(term) = terms.last_mut() else {
        return false;
    };
    let Some(last_content) = term
        .iter()
        .rposition(|inline| !matches!(inline, Inline::Anchor { .. }))
    else {
        return false;
    };
    if !matches!(term[last_content], Inline::LineBreak) {
        return false;
    }
    term.remove(last_content);
    true
}

/// A HEAD whose authored `\\p` markers closed rows inside the term has no
/// HANG protection left at the final pass: `term_flushln()`'s tail rule
/// (term.c:250-252) closes the row, so BODY starts its own row. A real
/// `term_fill()` pass requires printable content on both sides of the
/// break; author-split rows break before their first printed word.
fn marker_split_field_exited(head: DefinitionHeadFlow, terms: &[Vec<Inline>]) -> bool {
    let Some(term) = terms.last() else {
        return false;
    };
    let split_between_words = term.iter().enumerate().any(|(index, node)| {
        node == &Inline::LineBreak
            && term[..index]
                .iter()
                .any(|before| mant_ir::has_printable_character(std::slice::from_ref(before)))
            && term[index + 1..]
                .iter()
                .any(|after| mant_ir::has_printable_character(std::slice::from_ref(after)))
    });
    split_between_words
        && matches!(
            head,
            DefinitionHeadFlow::Detached {
                author_break_effect: crate::mandoc::inline::AuthorBreakEffect::Field { flags, .. },
            } if !flags.contains(crate::mandoc::inline::FieldFlag::Hang)
        )
}

fn invisible_closed_head_row(terms: &[Vec<Inline>]) -> bool {
    terms.last().is_some_and(|term| {
        !term.iter().any(|inline| {
            matches!(inline, Inline::LineBreak)
                || mant_ir::has_printable_character(std::slice::from_ref(inline))
        })
    })
}

fn carry_invisible_head_row(
    node: &Node,
    terms: &[Vec<Inline>],
    closed_head_row: bool,
    description: &mut Vec<Block>,
) {
    if closed_head_row && invisible_closed_head_row(terms) {
        // term_newln() flushed a HEAD cell, but an invisible term has no IR
        // renderer. Transfer its completed physical row to the BODY owner.
        description.insert(
            0,
            Block::VerticalSpace {
                lines: 1,
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
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> (Vec<Inline>, Option<RunInExecution>, bool, bool, Vec<usize>) {
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
            term_breaks,
        );
    }

    let mut term_builder = InlineBuilder::with_spacing(flow.spacing_enabled);
    let mut definition_field_exited = false;
    let mut definition_body_gap_consumed = false;
    let mut term_breaks = Vec::new();
    for (index, group) in groups.enumerate() {
        let (lowered, field_exited, body_gap_consumed, breaks) = context
            .lower_inline_with_author_break_preserving_rows(
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
        term_builder.finish_preserving_rows(),
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

/// Whether any nested node is an empty TEXT: its `NODE_LINE` runs
/// `term_newln()` mid-HEAD even though it prints nothing.
fn has_empty_text_child(node: &Node) -> bool {
    (node.kind == NodeKind::Text && node.text.as_deref() == Some(""))
        || node.children.iter().any(has_empty_text_child)
}

/// Return only document content from a definition macro's mixed-purpose head.
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
