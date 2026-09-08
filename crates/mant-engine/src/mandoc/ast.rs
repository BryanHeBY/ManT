//! Read-only AST part and source-coordinate access.
use super::{Node, SourceSpan};

pub(super) fn source_span(node: &Node) -> Option<SourceSpan> {
    (node.line > 0).then_some(SourceSpan {
        byte_range: None,
        line: node.line,
        column: node.column.max(1),
        end_line: None,
        end_column: None,
    })
}

/// Return the first libmandoc structural part of one kind.
///
/// Most semantic macros own at most one head, body, and tail. Callers whose
/// grammar permits repeated parts must use [`part_child_groups`] instead so
/// the multiplicity remains explicit at the lowering boundary.
pub(super) fn first_part_children(node: &Node, kind: libmandoc_rs::NodeKind) -> &[Node] {
    node.children
        .iter()
        .find(|child| child.kind == kind)
        .map_or(&[], |child| child.children.as_slice())
}

/// Iterate every libmandoc structural part of one kind in source order.
pub(super) fn part_child_groups(
    node: &Node,
    kind: libmandoc_rs::NodeKind,
) -> impl Iterator<Item = &[Node]> {
    node.children
        .iter()
        .filter(move |child| child.kind == kind)
        .map(|child| child.children.as_slice())
}
