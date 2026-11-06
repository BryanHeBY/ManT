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
    } = flow;
    let mut head = execute_synopsis_head(node, context, indent_columns, spacing_enabled, formatter);
    let head_row_open = formatter.execution.has_open_native_device_row();
    let body_origin = lifecycle::body_origin(node, context, indent_columns);
    // The HEAD post already selected Roman; BODY entry is another
    // print_man_node() pre transition, even if its font is Roman too.
    formatter
        .execution
        .begin_man_synopsis_body(body_origin.physical_basic_units());
    formatter
        .font
        .select(crate::mandoc::roff_escape::RoffFont::Regular);
    formatter.begin_definition_body(formatter.definition_head_row_occupied());
    let mut nested = lower_man_synopsis_body(
        node,
        context,
        paragraph_distance,
        formatter,
        &ScopeFlow::filled(body_origin, spacing_enabled),
    );
    let body_execution = formatter.finish_definition_body();
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

    if head.is_empty() {
        output.extend(nested);
        return;
    }

    if head_row_open
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
        // retain pre_SY(BODY)'s name-derived origin.
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

pub(super) fn execute_synopsis_head(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    // Only man SY reaches this structural HEAD path. Nm uses the shared
    // active-output driver and its ordinary source-order BODY traversal.
    // print_man_node() resets the current font at each BLOCK/HEAD entry.
    formatter
        .font
        .select(crate::mandoc::roff_escape::RoffFont::Regular);
    super::reset_man_macro_offset(formatter, indent_columns.macro_origin());
    let mut head_builder = formatter.begin_inline_session(
        spacing_enabled,
        context.active_mdoc_section() == crate::mandoc::source_context::MdocSectionContext::Authors,
        lifecycle::head_effect(node),
    );
    head_builder.begin_definition_head_consumption();
    head_builder.set_definition_native_margin(lifecycle::head_width(node).saturating_mul(24));
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
    let head = formatter.finish_inline_field_flush(head_builder);
    formatter.execution.clear_man_synopsis_head_no_break();
    // print_man_node() replaces the current font at HEAD post and again
    // at BODY entry; both transitions update a BODY-leading \fP's register.
    formatter
        .font
        .select(crate::mandoc::roff_escape::RoffFont::Regular);
    if head.is_empty() {
        return head;
    }
    // HTML represents the whole SY name as one code cell even when a
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
    pub(super) fn push_mdoc_synopsis_declaration(&mut self, node: &Node) -> bool {
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
