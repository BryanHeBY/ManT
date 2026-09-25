//! Shared definition content, source ownership, and head construction.
use super::super::lower_blocks_with_predecessor_and_run_in;
use super::super::synopsis::{
    SynopsisBoundary, SynopsisToken, synopsis_boundary, transparent_synopsis_predecessor,
};
use super::{
    Block, DefinitionItem, Inline, InlineBuilder, LoweringContext, Node, NodeKind,
    first_part_children, is_inline_equation, is_inline_equation_quote_artifact,
    lower_blocks_with_predecessor, source_span, targets,
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

#[derive(Clone, Copy)]
pub(super) enum PendingHeadConsumption {
    OccupiedUntil(u32),
}

#[derive(Clone, Copy, Default)]
pub(super) struct PendingHeadExecution {
    pub(super) placement_breaks: bool,
    pub(super) consumption: Option<PendingHeadConsumption>,
    pub(super) leading_rows: Option<PendingHeadRows>,
}

#[derive(Clone, Copy)]
pub(super) struct PendingHeadRows {
    rows: u16,
    boundary_line: u32,
}

/// Inspect the executed prefix of a detached definition body while the
/// native formatter still owns the term row. Presentation-only events and
/// row markers do not end that prefix; a real request does. `\&\p` is one
/// occupied cell whose deferred break closes the term row exactly once.
pub(super) fn pending_head_execution(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    formatter: crate::mandoc::formatter::FormatterState,
    initial_formatter_cell: bool,
) -> PendingHeadExecution {
    let mut execution = PendingHeadExecution::default();
    let mut has_body_cell_artifact = initial_formatter_cell;
    let mut formatter = formatter;
    inspect_pending_prefix(
        nodes,
        context,
        &mut formatter,
        &mut execution,
        &mut has_body_cell_artifact,
    );
    execution
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PrefixDecision {
    Continue,
    Break,
    Stop,
}

fn inspect_pending_prefix(
    nodes: &[Node],
    context: &LoweringContext<'_>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    execution: &mut PendingHeadExecution,
    has_body_cell_artifact: &mut bool,
) -> PrefixDecision {
    let mut previous_sibling = None;
    for node in nodes {
        if node.kind == NodeKind::Comment {
            continue;
        }
        let name = node.macro_name.as_deref();
        if let Some(control) = crate::mandoc::controls::formatter_control(name) {
            match control.boundary {
                crate::mandoc::controls::FormatterBoundary::Line => {
                    execution.placement_breaks = true;
                    execution.consumption = has_body_cell_artifact
                        .then_some(PendingHeadConsumption::OccupiedUntil(node.line));
                    return PrefixDecision::Break;
                }
                crate::mandoc::controls::FormatterBoundary::NoBreak => {
                    // CVS `.mc` commits an occupied formatter cell under
                    // TERMP_NOBREAK.  A deferred `\p` is therefore settled
                    // without ending the definition-head row, and later body
                    // text resumes at an ordinary boundary on that row.
                    if *has_body_cell_artifact {
                        execution.placement_breaks = false;
                        execution.consumption =
                            Some(PendingHeadConsumption::OccupiedUntil(node.line));
                        *has_body_cell_artifact = false;
                    }
                    continue;
                }
                crate::mandoc::controls::FormatterBoundary::None => {}
            }
        }
        if name == Some("Tg") || crate::mandoc::controls::operand_control(name).is_some() {
            continue;
        }
        if node.kind == NodeKind::Text {
            let decision = inspect_pending_text(node, execution, has_body_cell_artifact);
            if decision == PrefixDecision::Continue {
                continue;
            }
            return decision;
        }
        // Bf/Bk and font/semantic styling wrappers execute their children
        // even when the wrapper node itself is NODE_NOPRT. Recurse before
        // treating a no-print leaf as transparent.
        if let Some(wrapper) = pending_wrapper_effect(node, context, formatter, previous_sibling) {
            let visible = pending_wrapper_emits_visible_output(node, context);
            if let PendingWrapperEffect::Boundary(boundary) = wrapper {
                execution.placement_breaks = true;
                if *has_body_cell_artifact {
                    execution.consumption = Some(PendingHeadConsumption::OccupiedUntil(node.line));
                    *has_body_cell_artifact = false;
                }
                // `term_newln()` closes an occupied invisible row but does not
                // create vertical space; `term_vspace()` adds one empty row.
                // If the current wrapper is also invisible, its formatter row
                // must remain represented for the next request/body boundary.
                let rows = match (boundary, visible) {
                    (SynopsisBoundary::Newline, true) => 0,
                    (SynopsisBoundary::VerticalSpace, true)
                    | (SynopsisBoundary::Newline, false) => 1,
                    (SynopsisBoundary::VerticalSpace, false) => 2,
                };
                // `Some(0)` is observable: a native `term_newln()` replaces
                // the detached body's ordinary paragraph gap with a tight
                // boundary.  Explicit `.sp` blocks encountered later remain
                // independent and are added when the plan is applied.
                execution.leading_rows = Some(PendingHeadRows {
                    rows: execution
                        .leading_rows
                        .map_or(rows, |pending| pending.rows.saturating_add(rows)),
                    boundary_line: node.line,
                });
            }
            // The boundary preceding the first visible wrapper still belongs
            // to the pending term row, but the wrapper output and everything
            // after it must remain in source order in the detached body.
            if visible {
                return PrefixDecision::Stop;
            }
            let decision = inspect_pending_prefix(
                crate::mandoc::inline::inline_children(node),
                context,
                formatter,
                execution,
                has_body_cell_artifact,
            );
            if decision != PrefixDecision::Continue {
                return decision;
            }
            if !transparent_synopsis_predecessor(node) {
                previous_sibling = Some(SynopsisToken::from_node(node));
            }
            continue;
        }
        if node.flags.no_print {
            continue;
        }
        return PrefixDecision::Stop;
    }
    PrefixDecision::Continue
}

fn inspect_pending_text(
    node: &Node,
    execution: &mut PendingHeadExecution,
    has_body_cell_artifact: &mut bool,
) -> PrefixDecision {
    let events = node
        .text
        .as_deref()
        .map(crate::mandoc::roff_escape::decode)
        .unwrap_or_default();
    if events.is_empty()
        || events.iter().any(|event| {
            !matches!(
                crate::mandoc::roff_escape::inline_event_effect(event),
                crate::mandoc::roff_escape::InlineEventEffect::StateOnly
                    | crate::mandoc::roff_escape::InlineEventEffect::RowMarker
                    | crate::mandoc::roff_escape::InlineEventEffect::LineBoundary
            )
        })
    {
        return PrefixDecision::Stop;
    }
    let has_row_marker = events.iter().any(|event| {
        crate::mandoc::roff_escape::inline_event_effect(event)
            == crate::mandoc::roff_escape::InlineEventEffect::RowMarker
    });
    let has_line_boundary = events.iter().any(|event| {
        crate::mandoc::roff_escape::inline_event_effect(event)
            == crate::mandoc::roff_escape::InlineEventEffect::LineBoundary
    });
    *has_body_cell_artifact |= has_row_marker || has_line_boundary;
    if has_row_marker && has_line_boundary {
        execution.placement_breaks = true;
        execution.consumption = Some(PendingHeadConsumption::OccupiedUntil(node.line));
    }
    PrefixDecision::Continue
}

fn pending_wrapper_emits_visible_output(node: &Node, context: &LoweringContext<'_>) -> bool {
    super::super::synopsis::node_emits_visible_output(node, context.default_name)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PendingWrapperEffect {
    ChildrenOnly,
    Boundary(SynopsisBoundary),
}

/// Classify the formatter action of a macro in the pending definition-body
/// prefix. This is context and sequence aware: CVS mdoc emits synopsis and
/// author line boundaries from the wrapper itself, while ordinary semantic
/// wrappers contribute only their children.
fn pending_wrapper_effect(
    node: &Node,
    context: &LoweringContext<'_>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    previous: Option<SynopsisToken>,
) -> Option<PendingWrapperEffect> {
    use crate::mandoc::source_context::MdocSectionContext;
    let name = node.macro_name.as_deref()?;
    let current = SynopsisToken::from_node(node);
    if context.active_mdoc_section() == MdocSectionContext::Synopsis
        && node.flags.synopsis_pretty
        && current.invokes_pre()
        && let Some(previous) = previous
    {
        return Some(PendingWrapperEffect::Boundary(synopsis_boundary(
            previous, current,
        )));
    }
    if name == "An" {
        let authors_section = context.active_mdoc_section() == MdocSectionContext::Authors;
        if formatter.execute_author(node.author_mode, authors_section) {
            return Some(PendingWrapperEffect::Boundary(SynopsisBoundary::Newline));
        }
    }
    matches!(
        name,
        "No" | "Em"
            | "Sy"
            | "Li"
            | "Cm"
            | "Ar"
            | "Pa"
            | "Va"
            | "Vt"
            | "Ft"
            | "Fa"
            | "Fr"
            | "Ad"
            | "Ms"
            | "Sx"
            | "Tn"
            | "Mt"
            | "Dv"
            | "Er"
            | "Ev"
            | "Ic"
            | "Nm"
            | "Fd"
            | "Fn"
            | "In"
            | "Fo"
            | "Cd"
            | "An"
            | "Pf"
            | "B"
            | "I"
            | "SB"
            | "SM"
            | "R"
            | "BI"
            | "BR"
            | "IB"
            | "IR"
            | "RB"
            | "RI"
            | "Bf"
            | "Bk"
            | "Xo"
    )
    .then_some(PendingWrapperEffect::ChildrenOnly)
}

pub(super) fn definition_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    mut geometry: crate::mandoc::layout::DefinitionGeometry,
    flow: DefinitionFlow,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> PendingDefinitionItem {
    let head = visible_definition_head(node);
    let body = first_part_children(node, NodeKind::Body);
    let (displaced_equations, body) = displaced_definition_equations(head, body);
    let (mut term, run_in_execution, definition_field_exited, definition_body_gap_consumed) =
        lower_definition_head(head, &displaced_equations, context, flow, formatter);
    // CVS executes the `.It`/`.TP` head before its detached body.  Derive the
    // pending-row plan from the resulting formatter state so head-side `.An`,
    // font, spacing, and zero-width controls are visible to the body prefix.
    let pending_head = pending_head_execution(
        body,
        context,
        *formatter,
        flow.head.generated_cells().is_some_and(|cells| cells > 0),
    );
    if flow.shares_pending_term_row && pending_head.placement_breaks {
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    if definition_field_exited {
        geometry.placement = crate::mandoc::layout::TermPlacement::Stacked;
    }
    if definition_body_gap_consumed {
        geometry.gap = 0;
    }
    if let Some(id) = definition_head_anchor(node) {
        term.insert(
            0,
            crate::mandoc::inline::DraftInline::anchor_at(id, source_span(node)),
        );
    }
    let mut terms = split_definition_terms(term);
    let native_ranges = super::evidence::take_head_ranges(&mut terms);
    if flow.head.generated_cells().is_some() {
        // The native generated cells execute inside the shared stream below.
        // Their surviving projection is carried by the description itself;
        // adding the static geometry gap as well would count it twice.
        geometry.gap = 0;
    }
    let (layout, body_origin) = geometry.resolve(context, node, indent_columns, &terms);
    let mut description = if let Some(execution) = run_in_execution {
        lower_blocks_with_predecessor_and_run_in(
            body,
            context,
            body_origin,
            paragraph_distance,
            formatter.spacing,
            flow.paragraph_predecessor,
            formatter,
            Some((
                execution,
                usize::from(flow.head.generated_cells().unwrap_or_default()),
            )),
        )
    } else {
        lower_blocks_with_predecessor(
            body,
            context,
            body_origin,
            paragraph_distance,
            formatter.spacing,
            flow.paragraph_predecessor,
            formatter,
        )
    };
    if flow.shares_pending_term_row
        && let Some(consumption) = pending_head.consumption
    {
        context.content.with_context(|content| {
            consume_initial_formatter_row(content, &mut description, consumption);
        });
    }
    if flow.shares_pending_term_row
        && let Some(rows) = pending_head.leading_rows
    {
        retain_pending_head_rows(&mut description, rows);
    }
    PendingDefinitionItem {
        source: source_span(node),
        layout,
        terms,
        description,
        native_key: std::ptr::from_ref(node) as usize,
        role: (context.macro_set == libmandoc_rs::MacroSet::Mdoc)
            .then(|| super::evidence::leading_role(head))
            .flatten(),
        native_option_ranges: native_ranges.option_ranges,
        native_operand_ranges: native_ranges.operand_ranges,
    }
}

pub(in crate::mandoc::blocks::lists) struct PendingDefinitionItem {
    pub(super) source: Option<mant_ir::SourceSpan>,
    pub(super) layout: mant_ir::DefinitionLayout,
    pub(super) terms: Vec<Vec<crate::mandoc::inline::DraftInline>>,
    pub(super) description: Vec<Block>,
    pub(super) native_key: usize,
    pub(super) role: Option<crate::definitions::NativeHeadRole>,
    pub(super) native_option_ranges: Vec<Vec<std::ops::Range<usize>>>,
    pub(super) native_operand_ranges: Vec<Vec<std::ops::Range<usize>>>,
}

impl PendingDefinitionItem {
    pub(super) fn attach_targets(
        &mut self,
        targets: impl IntoIterator<Item = String>,
        source: Option<mant_ir::SourceSpan>,
    ) {
        // mdoc_validate.c post_tg moves a single NODE_ID onto the chosen
        // head/body owner. The pending head can already contain that same
        // target, so conserve one anchor before committing either stream.
        let mut seen = std::collections::HashSet::new();
        let anchors = targets
            .into_iter()
            .filter(|id| {
                seen.insert(id.clone())
                    && !self
                        .terms
                        .iter()
                        .any(|term| draft_contains_anchor(term, id))
                    && !targets::contains_anchor(&self.description, id)
            })
            .map(|id| crate::mandoc::inline::DraftInline::anchor_at(id, source))
            .collect::<Vec<_>>();
        if anchors.is_empty() {
            return;
        }
        if let Some(term) = self.terms.first_mut() {
            term.splice(0..0, anchors);
        } else {
            self.terms.push(anchors);
            self.layout.inline_term = true;
        }
    }

    pub(super) fn commit(self, context: &LoweringContext<'_>) -> DefinitionItem {
        let mut item = DefinitionItem {
            source: self.source,
            entry: None,
            layout: self.layout,
            terms: self
                .terms
                .into_iter()
                .map(|term| {
                    context
                        .content
                        .lower(mant_ir::ContentRootKind::Term, self.source, term)
                })
                .collect(),
            description: self.description,
        };
        if !item.terms.is_empty() {
            // Source coordinates can repeat under expansion. Carry a
            // parse-local owner through semantic preparation and remove it
            // before exporting public IR.
            let owner_point = context
                .content
                .point(mant_ir::ContentRootKind::Term, self.source);
            crate::definitions::mark_native_definition_owner(
                &mut item,
                self.native_key,
                owner_point,
            );
        }
        let mut evidence = context.native_heads.borrow_mut();
        evidence.groups.record(&item, self.native_key);
        if self.role.is_some()
            || self
                .native_operand_ranges
                .iter()
                .any(|term| !term.is_empty())
        {
            evidence.record_with_head_ranges(
                &item,
                self.role,
                self.native_option_ranges,
                self.native_operand_ranges,
            );
        }
        item
    }
}

fn draft_contains_anchor(nodes: &[crate::mandoc::inline::DraftInline], target: &str) -> bool {
    use crate::mandoc::inline::DraftInline;
    nodes.iter().any(|node| match node {
        DraftInline::Anchor { id, .. } => id == target,
        DraftInline::Strong { children }
        | DraftInline::Emphasis { children }
        | DraftInline::Link { children, .. } => draft_contains_anchor(children, target),
        DraftInline::Text { .. } | DraftInline::Code { .. } | DraftInline::LineBreak => false,
    })
}

fn lower_definition_head(
    head: &[Node],
    displaced_equations: &[&Node],
    context: &LoweringContext<'_>,
    flow: DefinitionFlow,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> (
    Vec<crate::mandoc::inline::DraftInline>,
    Option<crate::mandoc::inline::PreservedInlineState>,
    bool,
    bool,
) {
    let groups = std::iter::once(head).chain(
        displaced_equations
            .iter()
            .map(|equation| std::slice::from_ref(*equation)),
    );
    if flow.head.generated_cells().is_some() {
        let (term, mut execution) = context.lower_run_in_definition_head(
            groups,
            flow.spacing_enabled,
            formatter,
            flow.head.strong_scope(),
        );
        execution.last_executed_source_line = head
            .iter()
            .chain(displaced_equations.iter().copied())
            .filter_map(latest_source_line)
            .max();
        return (term, Some(execution), false, false);
    }

    let mut term_builder = InlineBuilder::with_spacing(flow.spacing_enabled);
    let mut definition_field_exited = false;
    let mut definition_body_gap_consumed = false;
    for group in groups {
        let (lowered, field_exited, body_gap_consumed) = context
            .lower_inline_draft_with_option_evidence(
                group,
                flow.spacing_enabled,
                formatter,
                flow.head.author_break_effect(),
                true,
            );
        term_builder.append(lowered);
        definition_field_exited |= field_exited;
        definition_body_gap_consumed |= body_gap_consumed;
    }
    (
        term_builder.finish(),
        None,
        definition_field_exited,
        definition_body_gap_consumed,
    )
}

fn latest_source_line(node: &Node) -> Option<u32> {
    std::iter::once(node.line)
        .chain(node.children.iter().filter_map(latest_source_line))
        .filter(|line| *line != 0)
        .max()
}

/// Consume the one physical row already represented by a pending man tag.
/// Anchor-only paragraphs are position metadata and do not end the search;
/// ordinary content does.  Explicit vertical requests remain after the
/// occupied row has been consumed.
fn consume_initial_formatter_row(
    content: mant_ir::ContentContext<'_>,
    blocks: &mut Vec<Block>,
    consumption: PendingHeadConsumption,
) {
    let PendingHeadConsumption::OccupiedUntil(boundary_line) = consumption;
    let mut index = 0;
    while index < blocks.len() {
        match &mut blocks[index] {
            Block::Paragraph { children, .. } => {
                let first_content = children
                    .iter()
                    .position(|child| !matches!(child, Inline::Anchor { .. }));
                let Some(first_content) = first_content else {
                    index += 1;
                    continue;
                };
                let generated_gap_break = children[first_content..]
                    .iter()
                    .position(|child| matches!(child, Inline::LineBreak { .. }))
                    .filter(|break_offset| {
                        children[first_content..first_content + break_offset]
                            .iter()
                            .all(|child| {
                                matches!(child, Inline::Text { content: text } if content.resolve_text(*text).is_some_and(|value| value.chars().all(char::is_whitespace)))
                            })
                    });
                if let Some(break_offset) = generated_gap_break {
                    children.drain(first_content..=first_content + break_offset);
                } else if matches!(&children[first_content], Inline::LineBreak { .. }) {
                    children.remove(first_content);
                } else if matches!(&children[first_content], Inline::Text { content: text } if content.resolve_text(*text).is_some_and(str::is_empty))
                    && matches!(
                        children.get(first_content + 1),
                        Some(Inline::LineBreak { .. })
                    )
                {
                    children.drain(first_content..=first_content + 1);
                }
                return;
            }
            Block::VerticalSpace { source, .. } => {
                let belongs_to_head_row = source
                    .as_ref()
                    .is_none_or(|source| source.line < boundary_line);
                if belongs_to_head_row {
                    blocks.remove(index);
                    continue;
                }
                return;
            }
            _ => return,
        }
    }
}

fn retain_pending_head_rows(blocks: &mut Vec<Block>, pending: PendingHeadRows) {
    // Invisible synopsis words are initially represented by source-less
    // formatter-cell placeholders.  The pending-head plan is the authoritative
    // CVS execution of that same prefix, so discard only those internal
    // projections.  Authored `.sp` nodes retain a source and remain additive.
    while matches!(
        blocks.first(),
        Some(Block::VerticalSpace { source: None, .. })
    ) {
        blocks.remove(0);
    }
    if let Some(Block::VerticalSpace { lines, source }) = blocks.first_mut() {
        if source.is_some_and(|source| source.line == pending.boundary_line) {
            // Ordinary body lowering already projected this exact
            // synopsis_pre() boundary. Relocate it without counting it twice.
            *lines = pending.rows;
        } else {
            // An authored `.sp` is independent from the synopsis boundary.
            *lines = lines.saturating_add(pending.rows);
        }
    } else if let Some(block) = blocks.first_mut()
        && let Some(layout) = mant_ir::geometry::block_layout_mut(block)
    {
        // The ordinary first-paragraph gap is merely the detached body's
        // fallback projection. Replace it with the exact native boundary,
        // including a tight `term_newln()` represented by zero.
        layout.spacing_before_lines = pending.rows;
    } else {
        blocks.insert(
            0,
            Block::VerticalSpace {
                lines: pending.rows,
                source: None,
            },
        );
    }
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
    term: Vec<crate::mandoc::inline::DraftInline>,
) -> Vec<Vec<crate::mandoc::inline::DraftInline>> {
    use crate::mandoc::inline::DraftInline;

    let mut terms = Vec::new();
    let mut current = Vec::new();
    for node in term {
        if matches!(node, DraftInline::LineBreak) {
            if crate::mandoc::inline::draft::has_printable_character(&current) {
                terms.push(std::mem::take(&mut current));
            } else {
                // An executed but invisible author word (`\&`, or a
                // control-only `\p`) still owns a physical formatter row.
                // Keep that row attached to the next visible spelling rather
                // than manufacturing an empty semantic alternative that
                // renderers are required to ignore.
                current.retain(
                    |inline| !matches!(inline, DraftInline::Text { value } if value.is_empty()),
                );
                current.push(node);
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
