//! Declaration geometry selected by the native SYNOPSIS flags.
use super::{
    Block, Inline, LoweringContext, Node, NodeKind, ScopeFlow, first_part_children, layout,
    source_span,
};
use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, RoffMacro,
};

pub(in crate::mandoc) mod lifecycle;
pub(super) use lifecycle::body_origin;

#[derive(Clone, Copy)]
pub(super) struct SynopsisFlow {
    pub(super) origin: crate::mandoc::layout::SourceIndent,
    pub(super) spacing_enabled: bool,
    pub(super) previous: Option<SynopsisToken>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum SynopsisBoundary {
    Newline,
    VerticalSpace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum SynopsisToken {
    Nm,
    Vt,
    Cd,
    Fd,
    Ft,
    Fn,
    In,
    Fo,
    Other,
}

impl SynopsisToken {
    pub(in crate::mandoc) fn from_node(node: &Node) -> Self {
        match node.macro_token.as_ref() {
            Some(Mdoc(MdocMacro::Nm)) => Self::Nm,
            Some(Mdoc(MdocMacro::Vt)) => Self::Vt,
            Some(Mdoc(MdocMacro::Cd)) => Self::Cd,
            Some(Mdoc(MdocMacro::Fd)) => Self::Fd,
            Some(Mdoc(MdocMacro::Ft)) => Self::Ft,
            Some(Mdoc(MdocMacro::Fn)) => Self::Fn,
            Some(Mdoc(MdocMacro::In)) => Self::In,
            Some(Mdoc(MdocMacro::Fo)) => Self::Fo,
            _ => Self::Other,
        }
    }

    pub(super) fn invokes_pre(self) -> bool {
        self != Self::Other
    }
}

/// CVS `roff_node_prev()` ignores these direct siblings before
/// `synopsis_pre()` compares macro tokens.  Keep this sibling-local in the
/// caller: nested container children must never become an outer predecessor.
pub(super) fn transparent_synopsis_predecessor(node: &Node) -> bool {
    // The owned Rust tree can represent an executable mdoc wrapper as
    // NODE_NOPRT while its printable normalized child carries SYNPRETTY.
    // Upstream synopsis_pre() is invoked for that child, so retain the
    // wrapper's macro token as the logical predecessor.
    if invokes_synopsis_pre(node) {
        return false;
    }
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

/// Mirror CVS `mdoc_term.c::synopsis_pre()` without guessing from rendered
/// text.  The caller decides whether the native newline is already represented
/// by its paragraph boundary; only vertical space needs an additional IR row.
pub(in crate::mandoc) fn synopsis_boundary(
    previous: SynopsisToken,
    current: SynopsisToken,
) -> SynopsisBoundary {
    if previous == current
        && !matches!(
            current,
            SynopsisToken::Ft | SynopsisToken::Fo | SynopsisToken::Fn
        )
    {
        return SynopsisBoundary::Newline;
    }
    if matches!(
        previous,
        SynopsisToken::Fd
            | SynopsisToken::Fn
            | SynopsisToken::Fo
            | SynopsisToken::In
            | SynopsisToken::Vt
    ) || previous == SynopsisToken::Ft
        && !matches!(current, SynopsisToken::Fn | SynopsisToken::Fo)
    {
        SynopsisBoundary::VerticalSpace
    } else {
        SynopsisBoundary::Newline
    }
}

fn invokes_synopsis_pre(node: &Node) -> bool {
    (node.flags.synopsis_pretty
        || node
            .children
            .iter()
            .any(|child| child.flags.synopsis_pretty))
        && SynopsisToken::from_node(node).invokes_pre()
}

fn is_mdoc_synopsis_declaration(node: &Node) -> bool {
    invokes_synopsis_pre(node)
        && matches!(
            node.macro_token.as_ref(),
            Some(Mdoc(
                MdocMacro::Fd
                    | MdocMacro::Cd
                    | MdocMacro::In
                    | MdocMacro::Vt
                    | MdocMacro::Ft
                    | MdocMacro::Fn
                    | MdocMacro::Fo
            ))
        )
}

pub(super) fn lower_synopsis_head(
    output: &mut Vec<Block>,
    node: &Node,
    context: &LoweringContext<'_>,
    flow: SynopsisFlow,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) {
    let SynopsisFlow {
        origin: indent_columns,
        spacing_enabled,
        previous,
    } = flow;
    let saved_offset_shift = formatter.row_presentation.offset_shift;
    let mut head = if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Nm)) {
        execute_name_head(node, context, spacing_enabled, previous, formatter)
    } else {
        execute_synopsis_head(node, context, indent_columns, spacing_enabled, formatter)
    };
    let head_row_open = formatter.execution.has_open_native_device_row();
    let body_origin = lifecycle::body_origin(node, context, indent_columns);
    if node.macro_token.as_ref() == Some(&Man(ManMacro::Sy)) {
        // The HEAD post already selected Roman; BODY entry is another
        // print_man_node() pre transition, even if its font is Roman too.
        formatter
            .execution
            .begin_man_synopsis_body(body_origin.physical_basic_units());
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
    }
    formatter.begin_definition_body(formatter.definition_head_row_occupied());
    let mut nested = if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Nm)) {
        lower_name_body(
            node,
            context,
            paragraph_distance,
            formatter,
            &ScopeFlow::filled(body_origin, spacing_enabled),
        )
    } else {
        lower_man_synopsis_body(
            node,
            context,
            paragraph_distance,
            formatter,
            &ScopeFlow::filled(body_origin, spacing_enabled),
        )
    };
    let body_execution = formatter.finish_definition_body();
    if node.macro_token.as_ref() == Some(&Man(ManMacro::Sy)) {
        // BODY post and BLOCK post both execute the man(7) font reset.
        // Replacing Roman twice matters to a following \fP.
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
        super::reset_man_macro_offset(formatter, indent_columns);
        formatter.execution.finish_man_synopsis_body();
    } else {
        // print_mdoc_node() restores Nm BODY/BLOCK's saved source offset;
        // unlike offset, ti is never restored by a scope return.
        formatter.row_presentation.offset_shift = saved_offset_shift;
        formatter.row_presentation.parent =
            i32::try_from(indent_columns.physical_basic_units()).unwrap_or(i32::MAX);
    }
    if head.is_empty() {
        output.extend(nested);
        return;
    }

    if (head_row_open || node.macro_token.as_ref() != Some(&Man(ManMacro::Sy)))
        && !body_execution.placement_breaks()
        && let Some(Block::Paragraph {
            children,
            source,
            inline_layout,
            layout,
            ..
        }) = nested.first_mut()
    {
        let head_layout = crate::mandoc::inline::take_inline_layout(&mut head);
        let row_offset = u32::try_from(mant_ir::logical_row_count(&head) - 1).unwrap_or(u32::MAX);
        for hint in &mut inline_layout.row_hints {
            hint.row = hint.row.saturating_add(row_offset);
        }
        // Both destinations have already executed. Their native print
        // receipts own the seam's padding; an append-word boundary here
        // would invent a separator after pre_SY(BODY)'s NOSPACE.
        head.append(children);
        *children = head;
        // The first row shares the HEAD origin; later hard/wrapped rows
        // retain pre_SY/termp_nm_pre(BODY)'s name-derived origin.
        let body_delta = body_origin.offset_from(indent_columns);
        inline_layout.row_hints = merged_synopsis_rows(
            &head_layout,
            inline_layout,
            row_offset,
            body_delta,
            head_row_open,
        );
        layout.indent_columns = indent_columns.relative_columns();
        layout.continuation_indent_columns = layout
            .continuation_indent_columns
            .saturating_add(body_delta);
        *source = source_span(node);
        output.extend(nested);
        return;
    }

    output.push(Block::Paragraph {
        inline_layout: crate::mandoc::inline::take_inline_layout(&mut head),
        children: head,
        layout: layout(indent_columns),
        source: source_span(node),
    });
    output.extend(nested);
}

fn merged_synopsis_rows(
    head: &mant_ir::InlineLayout,
    body: &mant_ir::InlineLayout,
    head_last_row: u32,
    body_delta: i32,
    head_row_open: bool,
) -> Vec<mant_ir::RowLayoutHint> {
    let mut hints = body
        .row_hints
        .iter()
        .map(|hint| (hint.row, *hint))
        .collect::<std::collections::BTreeMap<_, _>>();
    for row in 0..=head_last_row {
        if row > 0 && row == head_last_row && !head_row_open {
            // The real HEAD post closed its last device row. The delimiter's
            // empty logical tail is the BODY's first row, not another
            // already printed HEAD origin.
            continue;
        }
        let mut hint = head.row_layout(usize::try_from(row).unwrap_or(usize::MAX));
        hint.row = row;
        if row > 0 {
            // BODY continuation applies to later soft wraps, not to the
            // already executed first origin of a HEAD hard row. Preserve
            // even a native default-zero origin relative to that new base.
            hint.first_line_indent_columns =
                hint.first_line_indent_columns.saturating_sub(body_delta);
        }
        hints.remove(&row);
        if !hint.is_empty() {
            hints.insert(row, hint);
        }
    }
    hints.into_values().collect()
}

fn execute_name_head(
    node: &Node,
    context: &LoweringContext<'_>,
    spacing_enabled: bool,
    previous: Option<SynopsisToken>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    let mut builder = formatter.begin_inline_session(
        spacing_enabled,
        context.active_mdoc_section() == crate::mandoc::source_context::MdocSectionContext::Authors,
        crate::mandoc::inline::AuthorBreakEffect::Line,
    );
    builder.scope_posts = context.scope_posts.clone();
    builder.observe_no_fill_source_lines(true);
    builder.begin_definition_head_consumption();
    builder.enter_keep_words();
    let head_index = node
        .children
        .iter()
        .position(|part| part.kind == NodeKind::Head);
    let occupied = if let Some(index) = head_index {
        crate::mandoc::inline::append_inline_nodes(
            &mut builder,
            &node.children[..index],
            context.default_name,
        );
        crate::mandoc::inline::names::append_head(
            &mut builder,
            node,
            &node.children[index],
            previous,
            context.default_name,
        )
    } else {
        false
    };
    builder.observe_no_fill_source_lines(false);
    let (head, preserved) = formatter.finish_inline_scope_with_occupied(builder, occupied);
    let mut no_output = Vec::new();
    formatter.with_output_builder(&mut no_output, |builder| {
        builder.inherit_preserved_execution(preserved);
    });
    assert_eq!(no_output.len(), 0);
    head
}

fn lower_name_body(
    node: &Node,
    context: &LoweringContext<'_>,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    flow: &ScopeFlow<'_>,
) -> Vec<Block> {
    let mut lowerer = super::BlockLowerer::new(
        context,
        flow.indent_columns,
        paragraph_distance,
        flow.spacing_enabled,
        Vec::new(),
        std::mem::take(formatter),
    );
    let width = crate::mandoc::inline::names::body_width(node, context.default_name);
    let head_index = node
        .children
        .iter()
        .position(|part| part.kind == NodeKind::Head);
    for part in &node.children[head_index.map_or(0, |index| index + 1)..] {
        if part.kind == NodeKind::Body {
            let saved = lowerer.state.enter_name_body_node(part, width);
            lowerer.push_nodes(&part.children);
            lowerer.state.finish_name_body_node(part, saved);
        } else {
            lowerer.push_nodes(std::slice::from_ref(part));
        }
    }
    lowerer.state.formatter.execution.exit_name_block();
    lowerer.finish_into(formatter, super::FormatterRowBoundary::Preserve)
}

fn lower_man_synopsis_body(
    node: &Node,
    context: &LoweringContext<'_>,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    flow: &ScopeFlow<'_>,
) -> Vec<Block> {
    let mut lowerer = super::BlockLowerer::new(
        context,
        flow.indent_columns,
        paragraph_distance,
        flow.spacing_enabled,
        Vec::new(),
        std::mem::take(formatter),
    );
    lowerer.push_nodes(first_part_children(node, NodeKind::Body));
    // man_term.c::post_SY(BODY) consumes the shared native buffer before
    // its destination drains and before the caller restores mt->offset.
    // The current output owner, not historical no-fill bookkeeping, owns it.
    lowerer.state.finish_native_body_line();
    lowerer.finish_into(formatter, super::FormatterRowBoundary::Preserve)
}

fn terminal_empty_name_body(node: &Node) -> Option<&Node> {
    let head = node
        .children
        .iter()
        .position(|part| part.kind == NodeKind::Head)?;
    match &node.children[head + 1..] {
        [body] if body.kind == NodeKind::Body && body.children.is_empty() => Some(body),
        _ => None,
    }
}

pub(super) fn execute_synopsis_head(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    // man_term.c::print_man_node() and mdoc_term.c::print_mdoc_node()
    // execute HEAD children in the same formatter stream as BODY. The head
    // is a separate IR destination, not a separate font/zero-width state.
    if node.macro_token.as_ref() == Some(&Man(ManMacro::Sy)) {
        // Unlike mdoc Nm, each man SY BLOCK/HEAD boundary resets the native
        // current font before the head pre-handler selects bold.
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
        // pre_SY(HEAD) assigns mt->offset after the BLOCK print_bvspace.
        super::reset_man_macro_offset(formatter, indent_columns.macro_origin());
    }
    let mut head_builder = formatter.begin_inline_session(
        spacing_enabled,
        context.active_mdoc_section() == crate::mandoc::source_context::MdocSectionContext::Authors,
        lifecycle::head_effect(node),
    );
    if lifecycle::flushes_head(node) {
        head_builder.begin_definition_head_consumption();
        head_builder.set_definition_native_margin(lifecycle::head_width(node).saturating_mul(24));
    }
    head_builder.scope_posts = context.scope_posts.clone();
    let saved_font = head_builder
        .font
        .push_scope(crate::mandoc::roff_escape::RoffFont::Strong);
    crate::mandoc::inline::append_inline_nodes(
        &mut head_builder,
        first_part_children(node, NodeKind::Head),
        context.default_name,
    );
    head_builder.font.pop_scope(saved_font);
    let head = if node.macro_token.as_ref() == Some(&Man(ManMacro::Sy)) {
        formatter.finish_inline_field_flush(head_builder)
    } else if !first_part_children(node, NodeKind::Body).is_empty() {
        // mdoc_term.c's termp_nm_post(HEAD) flushes when BODY is present.
        // Settle pending zero-width glyphs before ownership moves to BODY.
        formatter
            .finish_inline_line_with_rows(head_builder, true)
            .output
    } else {
        let (head, preserved) = formatter.finish_inline_scope(head_builder);
        let mut no_output = Vec::new();
        formatter.with_output_builder(&mut no_output, |builder| {
            builder.inherit_preserved_execution(preserved);
        });
        assert_eq!(no_output.len(), 0);
        head
    };
    if node.macro_token.as_ref() == Some(&Man(ManMacro::Sy)) {
        formatter.execution.clear_man_synopsis_head_no_break();
        // man_term.c::print_man_node() replaces the current font at HEAD
        // post and again at BODY entry. Both transitions update fontlast,
        // which a BODY-leading \fP reads before any visible argument.
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
    }
    if head.is_empty() {
        return head;
    }
    // HTML represents the whole SY/Nm name as one code cell even when a
    // font escape changes an interior run. Keep that structural Strong owner
    // after the HEAD has executed; flatten only redundant direct Strong runs.
    let children = head
        .into_iter()
        .flat_map(|inline| match inline {
            Inline::Strong { children } => children,
            inline => vec![inline],
        })
        .collect();
    vec![Inline::Strong { children }]
}

impl super::BlockLowerer<'_, '_> {
    /// An empty Nm BODY has no offset/flush post. Keep its unflushed HEAD
    /// in the current paragraph so the next real native boundary consumes
    /// the same owner, rather than interpreting it in an empty BODY scope.
    fn push_unflushed_name(&mut self, node: &Node, previous: Option<&Node>) -> bool {
        if node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::Nm))
            || terminal_empty_name_body(node).is_none()
        {
            return false;
        }
        let default_name = self.context.default_name;
        self.state.push_inline_with(
            source_span(node),
            super::inline_flow::starts_indented_filled_line(node),
            super::ends_with_line_continuation(node),
            |builder| {
                builder.observe_no_fill_source_lines(true);
                crate::mandoc::inline::append_inline_node_with_siblings(
                    builder,
                    node,
                    previous,
                    None,
                    default_name,
                );
                builder.observe_no_fill_source_lines(false);
            },
        );
        true
    }

    /// Preserve declaration boundaries selected by mdoc's SYNOPSIS grammar.
    ///
    /// libmandoc marks the declaration-oriented `Fd`, `In`, `Ft`, `Fn`,
    /// `Fo`, and `Vt` nodes with `synopsis_pretty`.  Their terminal layout is
    /// intentionally richer than `ManT`'s IR, but the declaration boundary is
    /// semantic: a following synopsis macro applies its native pre-boundary,
    /// while only `Fd`, `Fn`, and `Fo` end their declaration afterwards.
    /// `Ft` and the following function declaration share a paragraph but
    /// retain the native hard row boundary between return type and function.
    /// Macros without a native post break can share their row with ordinary
    /// following body text.
    pub(super) fn push_mdoc_synopsis_declaration(
        &mut self,
        node: &Node,
        previous: Option<&Node>,
    ) -> bool {
        if self.push_unflushed_name(node, previous) {
            return true;
        }
        if !is_mdoc_synopsis_declaration(node) {
            return false;
        }
        // The actual source entry already executed synopsis_pre against the
        // live owner. Macro posts execute inside the shared inline driver;
        // an IR paragraph drain is neither of these native boundaries.
        self.push_inline_node(node, None);
        true
    }
}
