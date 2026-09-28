//! Declaration geometry selected by the native SYNOPSIS flags.
use super::{
    Block, Inline, InlineBuilder, LoweringContext, Node, NodeKind, first_part_children, layout,
    lower_blocks_with_spacing, lower_inline_nodes_with_spacing, source_span,
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

    let head = vec![Inline::Strong { children: head }];
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
