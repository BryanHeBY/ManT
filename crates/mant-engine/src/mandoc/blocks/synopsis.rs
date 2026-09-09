//! Declaration geometry selected by the native SYNOPSIS flags.
use super::{
    Block, Inline, InlineBuilder, LoweringContext, Node, NodeKind, first_part_children, layout,
    lower_blocks_with_spacing, lower_inline_nodes_with_spacing, source_span,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SynopsisDeclarationRole {
    Standalone,
    ReturnType,
    Function,
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
        "Fd" | "In" | "Vt" => Some(SynopsisDeclarationRole::Standalone),
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
    /// semantic: an include directive must not merge into the next return
    /// type, and each function declaration must remain independently
    /// addressable.  A return type and the immediately following function
    /// macro form one useful IR paragraph; all other marked declarations own
    /// a paragraph of their own.
    pub(super) fn push_mdoc_synopsis_declaration(&mut self, node: &Node) -> bool {
        let Some(role) = mdoc_synopsis_declaration_role(node) else {
            if self.synopsis_return_type_open {
                self.state.flush_paragraph();
                self.synopsis_return_type_open = false;
            }
            return false;
        };

        match role {
            SynopsisDeclarationRole::ReturnType => {
                self.state.flush_paragraph();
                self.push_inline_node(node, None);
                self.synopsis_return_type_open = true;
            }
            SynopsisDeclarationRole::Function => {
                if !self.synopsis_return_type_open {
                    self.state.flush_paragraph();
                }
                self.push_inline_node(node, None);
                self.state.flush_paragraph();
                self.synopsis_return_type_open = false;
            }
            SynopsisDeclarationRole::Standalone => {
                self.state.flush_paragraph();
                self.push_inline_node(node, None);
                self.state.flush_paragraph();
                self.synopsis_return_type_open = false;
            }
        }
        true
    }
}
