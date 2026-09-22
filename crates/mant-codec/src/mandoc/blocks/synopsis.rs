//! Declaration geometry selected by the native SYNOPSIS flags.
use super::{
    Block, LoweringContext, Node, NodeKind, first_part_children, layout, lower_blocks_with_spacing,
    lower_inline_nodes_with_spacing, source_span,
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

/// Whether this syntax node has begun the visible body stream.  `In`, `Fn`,
/// and `Fo` generate punctuation even when their authored operands are empty;
/// that output is part of their CVS terminal handlers, not visible AST text.
pub(super) fn node_emits_visible_output(node: &Node, default_name: Option<&str>) -> bool {
    matches!(node.macro_name.as_deref(), Some("In" | "Fn" | "Fo"))
        || crate::mandoc::inline::node_emits_visible_output(node, default_name)
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
    let head = lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Head),
        context.default_name,
        spacing_enabled,
    );
    let mut nested = lower_blocks_with_spacing(
        first_part_children(node, NodeKind::Body),
        context,
        indent_columns,
        paragraph_distance,
        spacing_enabled,
        formatter,
    );
    if head.is_empty() {
        output.extend(nested);
        return;
    }

    let head = context.content.lower(
        mant_ir::ContentRootKind::Body,
        source_span(node),
        vec![crate::mandoc::inline::DraftInline::Strong { children: head }],
    );
    if let Some(Block::Paragraph {
        children, source, ..
    }) = nested.first_mut()
    {
        let mut body = std::mem::take(children);
        let needs_space = context.content.with_context(|content| {
            crate::mandoc::inline::needs_boundary_space(
                content.last_visible_character(&head).ok().flatten(),
                content.first_visible_character(&body).ok().flatten(),
            )
        });
        *children = head;
        if spacing_enabled && needs_space {
            children.extend(context.content.lower(
                mant_ir::ContentRootKind::Body,
                source_span(node),
                vec![crate::mandoc::inline::DraftInline::Text { value: " ".into() }],
            ));
        }
        children.append(&mut body);
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

impl super::BlockLowerer<'_, '_> {
    /// Preserve declaration boundaries selected by mdoc's SYNOPSIS grammar.
    ///
    /// libmandoc marks the declaration-oriented `Fd`, `In`, `Ft`, `Fn`,
    /// `Fo`, and `Vt` nodes with `synopsis_pretty`.  Their terminal layout is
    /// intentionally richer than `ManT`'s IR, but the declaration boundary is
    /// semantic: a following synopsis macro applies its native pre-boundary,
    /// while only `Fd`, `Fn`, and `Fo` end their declaration afterwards.
    /// `Ft` plus an immediately following function macro remain one useful IR
    /// paragraph; macros without a native post break can share their row with
    /// ordinary following body text.
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
                self.state.flush_paragraph();
                self.state.output.push(Block::VerticalSpace {
                    lines: 1,
                    source: source_span(node),
                });
            } else {
                newline_boundary = true;
            }
        }

        let Some(role) = mdoc_synopsis_declaration_role(node) else {
            return false;
        };

        // A wholly invisible leading prefix is accounted for by the pending
        // definition-head execution pass.  Once visible body output exists,
        // preserve CVS vertical spacing at the source position instead of
        // moving it ahead of that output.
        match role {
            SynopsisDeclarationRole::ReturnType | SynopsisDeclarationRole::NoPostBreak => {
                if newline_boundary && self.state.has_formatter_cell() {
                    self.state.flush_paragraph();
                }
                self.push_inline_node(node, None);
            }
            SynopsisDeclarationRole::Function => {
                if newline_boundary
                    && !previous
                        .is_some_and(|node| SynopsisToken::from_node(node) == SynopsisToken::Ft)
                    && self.state.has_formatter_cell()
                {
                    self.state.flush_paragraph();
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
