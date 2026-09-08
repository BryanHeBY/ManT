//! Declaration geometry selected by the native SYNOPSIS flags.
use super::{
    Block, Inline, InlineBuilder, LoweringContext, Node, NodeKind, first_part_children, layout,
    lower_blocks_with_spacing, lower_inline_nodes_with_spacing, source_span,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SynopsisDeclarationRole {
    Standalone,
    ReturnType,
    Function,
}

pub(super) fn mdoc_synopsis_declaration_role(node: &Node) -> Option<SynopsisDeclarationRole> {
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
    indent_columns: u16,
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
