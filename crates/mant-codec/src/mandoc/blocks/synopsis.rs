//! Declaration geometry selected by the native SYNOPSIS flags.
use super::{
    Block, Inline, InlineBuilder, LoweringContext, Node, NodeKind, first_part_children, layout,
    lower_blocks_with_spacing, source_span,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SynopsisDeclarationRole {
    PostBreak,
    NoPostBreak,
    ReturnType,
    Function,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SynopsisBoundary {
    Newline,
    VerticalSpace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SynopsisToken {
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
    pub(super) fn from_node(node: &Node) -> Self {
        match node.macro_name.as_deref() {
            Some("Nm") => Self::Nm,
            Some("Vt") => Self::Vt,
            Some("Cd") => Self::Cd,
            Some("Fd") => Self::Fd,
            Some("Ft") => Self::Ft,
            Some("Fn") => Self::Fn,
            Some("In") => Self::In,
            Some("Fo") => Self::Fo,
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
            node.macro_name.as_deref(),
            Some(
                "ft" | "ll"
                    | "mc"
                    | "po"
                    | "ta"
                    | "Db"
                    | "Es"
                    | "Sm"
                    | "Tg"
                    | "DT"
                    | "UC"
                    | "PD"
                    | "AT"
            )
        )
}

/// Mirror CVS `mdoc_term.c::synopsis_pre()` without guessing from rendered
/// text.  The caller decides whether the native newline is already represented
/// by its paragraph boundary; only vertical space needs an additional IR row.
pub(super) fn synopsis_boundary(
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

fn mdoc_synopsis_declaration_role(node: &Node) -> Option<SynopsisDeclarationRole> {
    let synopsis_pretty = node.flags.synopsis_pretty
        || node
            .children
            .iter()
            .any(|child| child.flags.synopsis_pretty);
    if !synopsis_pretty {
        return None;
    }
    match node.macro_name.as_deref()? {
        "Fd" => Some(SynopsisDeclarationRole::PostBreak),
        "Cd" | "In" | "Vt" => Some(SynopsisDeclarationRole::NoPostBreak),
        "Ft" => Some(SynopsisDeclarationRole::ReturnType),
        "Fn" | "Fo" => Some(SynopsisDeclarationRole::Function),
        _ => None,
    }
}

pub(super) fn lower_synopsis_head(
    output: &mut Vec<Block>,
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) {
    let head = execute_synopsis_head(node, context, spacing_enabled, formatter);
    if node.macro_name.as_deref() == Some("SY") {
        // The HEAD post already selected Roman; BODY entry is another
        // print_man_node() pre transition, even if its font is Roman too.
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
    }
    let mut nested = lower_blocks_with_spacing(
        first_part_children(node, NodeKind::Body),
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        formatter,
    );
    if node.macro_name.as_deref() == Some("SY") {
        // BODY post and BLOCK post both execute the man(7) font reset.
        // Replacing Roman twice matters to a following \fP.
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
    }
    if head.is_empty() {
        output.extend(nested);
        return;
    }

    if let Some(Block::Paragraph {
        children, source, ..
    }) = nested.first_mut()
    {
        let body = std::mem::take(children);
        let mut synopsis = InlineBuilder::with_spacing(spacing_enabled);
        synopsis.append(head);
        synopsis.append(body);
        *children = synopsis.finish();
        *source = source_span(node);
        output.extend(nested);
        return;
    }

    output.push(Block::Paragraph {
        children: head,
        layout: layout(indent_columns),
        source: source_span(node),
    });
    output.extend(nested);
}

pub(super) fn execute_synopsis_head(
    node: &Node,
    context: &LoweringContext<'_>,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Inline> {
    // man_term.c::print_man_node() and mdoc_term.c::print_mdoc_node()
    // execute HEAD children in the same formatter stream as BODY. The head
    // is a separate IR destination, not a separate font/zero-width state.
    if node.macro_name.as_deref() == Some("SY") {
        // Unlike mdoc Nm, each man SY BLOCK/HEAD boundary resets the native
        // current font before the head pre-handler selects bold.
        formatter
            .font
            .select(crate::mandoc::roff_escape::RoffFont::Regular);
    }
    let mut head_builder = formatter.begin_inline_session(
        spacing_enabled,
        context.active_mdoc_section() == crate::mandoc::source_context::MdocSectionContext::Authors,
        crate::mandoc::inline::AuthorBreakEffect::Line,
    );
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
    let head = if node.macro_name.as_deref() == Some("SY")
        || !first_part_children(node, NodeKind::Body).is_empty()
    {
        // man_term.c::post_SY(HEAD) always flushes; mdoc_term.c's
        // termp_nm_post(HEAD) flushes when BODY is present. Settle pending
        // zero-width glyphs here before output ownership moves to BODY.
        formatter.finish_inline_line(head_builder).output
    } else {
        let (head, preserved) = formatter.finish_inline_scope(head_builder);
        let mut no_output = Vec::new();
        formatter.with_output_builder(&mut no_output, |builder| {
            builder.inherit_preserved_execution(preserved);
        });
        debug_assert!(no_output.is_empty());
        head
    };
    if node.macro_name.as_deref() == Some("SY") {
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
        let mut newline_boundary = false;
        if invokes_synopsis_pre(node)
            && let Some(previous) = previous
        {
            let boundary = synopsis_boundary(
                SynopsisToken::from_node(previous),
                SynopsisToken::from_node(node),
            );
            if boundary == SynopsisBoundary::VerticalSpace {
                self.state.formatter.note_definition_boundary();
                self.state.flush_paragraph_for_line_request();
                self.state.output.push(Block::VerticalSpace {
                    lines: 1,
                    source: source_span(node),
                });
            } else {
                newline_boundary = true;
                // synopsis_pre() executes term_newln() even if the detached
                // BODY has no local cell: the native definition HEAD may
                // still be buffered on that row.
                self.state.formatter.note_definition_boundary();
                if !self.state.has_formatter_cell() {
                    self.state.formatter.settle_definition_head_rows();
                }
            }
        }

        let Some(role) = mdoc_synopsis_declaration_role(node) else {
            return false;
        };

        // The real BODY walk owns its leading boundary. Once a visible word
        // exists, keep each CVS synopsis break at that source position.
        match role {
            SynopsisDeclarationRole::ReturnType | SynopsisDeclarationRole::NoPostBreak => {
                if newline_boundary && self.state.has_formatter_cell() {
                    self.state.flush_paragraph();
                }
                self.push_inline_node(node, None);
            }
            SynopsisDeclarationRole::Function => {
                if newline_boundary && self.state.has_formatter_cell() {
                    if previous
                        .is_some_and(|node| SynopsisToken::from_node(node) == SynopsisToken::Ft)
                    {
                        // CVS synopsis_pre() calls term_newln() for Ft→Fn/Fo.
                        // This is one declaration paragraph with two rows.
                        self.state.hard_break();
                    } else {
                        self.state.flush_paragraph();
                    }
                }
                self.push_inline_node(node, None);
                self.state.flush_paragraph();
            }
            SynopsisDeclarationRole::PostBreak => {
                if newline_boundary && self.state.has_formatter_cell() {
                    self.state.flush_paragraph();
                }
                self.push_inline_node(node, None);
                self.state.flush_paragraph();
            }
        }
        true
    }
}
