//! Guard existing recursive render helpers without predicting emitted math.
use super::{MAX_MARKDOWN_MATH_NODES, MarkdownMathError, MarkdownMathLimit};
use crate::{ResolvedContent, math::MAX_MATH_DEPTH};
use mant_ir::{
    Block, DefinitionItem, DefinitionTerm, Inline, ListItem, Section, TableCell, TableRow,
};

#[derive(Clone, Copy)]
enum Node<'a> {
    Section(&'a Section),
    Block(&'a Block),
    Inline(&'a Inline),
    Item(&'a ListItem),
    Definition(&'a DefinitionItem),
    Term(&'a DefinitionTerm),
    Row(&'a TableRow),
    Cell(&'a TableCell),
}

pub(super) fn check(query: &ResolvedContent) -> Result<(), MarkdownMathError> {
    let Some(document) = &query.document else {
        return Ok(());
    };
    let mut pending = Vec::new();
    let mut visited = 0;
    push(
        &mut pending,
        document.blocks.iter().map(Node::Block),
        1,
        visited,
    )?;
    push(
        &mut pending,
        document.sections.iter().map(Node::Section),
        1,
        visited,
    )?;
    if let Some(heading) = &document.heading {
        push(
            &mut pending,
            heading.content.iter().map(Node::Inline),
            1,
            visited,
        )?;
    }
    while let Some((node, depth)) = pending.pop() {
        visited += 1;
        let next = depth + 1;
        append_children(node, &mut pending, next, visited)?;
    }
    Ok(())
}

fn append_children<'a>(
    node: Node<'a>,
    pending: &mut Vec<(Node<'a>, usize)>,
    next: usize,
    visited: usize,
) -> Result<(), MarkdownMathError> {
    match node {
        Node::Section(section) => {
            push(
                pending,
                section.children.iter().map(Node::Section),
                next,
                visited,
            )?;
            push(
                pending,
                section.blocks.iter().map(Node::Block),
                next,
                visited,
            )?;
            push(
                pending,
                section.heading.content.iter().map(Node::Inline),
                next,
                visited,
            )?;
        }
        Node::Block(block) => match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                push(pending, children.iter().map(Node::Inline), next, visited)?;
            }
            Block::List { items, .. } => {
                push(pending, items.iter().map(Node::Item), next, visited)?;
            }
            Block::DefinitionList { items, .. } => {
                push(pending, items.iter().map(Node::Definition), next, visited)?;
            }
            Block::Table { rows, .. } => push(pending, rows.iter().map(Node::Row), next, visited)?,
            _ => {}
        },
        Node::Inline(
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. },
        ) => push(pending, children.iter().map(Node::Inline), next, visited)?,
        Node::Inline(_) => {}
        Node::Item(item) => push(pending, item.blocks.iter().map(Node::Block), next, visited)?,
        Node::Definition(item) => {
            push(pending, item.terms.iter().map(Node::Term), next, visited)?;
            push(
                pending,
                item.description.iter().map(Node::Block),
                next,
                visited,
            )?;
        }
        Node::Term(term) => push(
            pending,
            term.content.iter().map(Node::Inline),
            next,
            visited,
        )?,
        Node::Row(row) => push(pending, row.cells.iter().map(Node::Cell), next, visited)?,
        Node::Cell(cell) => push(pending, cell.blocks.iter().map(Node::Block), next, visited)?,
    }
    Ok(())
}

fn push<'a>(
    pending: &mut Vec<(Node<'a>, usize)>,
    nodes: impl ExactSizeIterator<Item = Node<'a>>,
    depth: usize,
    visited: usize,
) -> Result<(), MarkdownMathError> {
    if nodes.len() == 0 {
        return Ok(());
    }
    if depth > MAX_MATH_DEPTH {
        return Err(MarkdownMathError::LimitExceeded {
            limit: MarkdownMathLimit::Depth,
        });
    }
    if nodes.len()
        > MAX_MARKDOWN_MATH_NODES
            .saturating_sub(visited)
            .saturating_sub(pending.len())
    {
        return Err(MarkdownMathError::LimitExceeded {
            limit: MarkdownMathLimit::Nodes,
        });
    }
    pending.extend(nodes.map(|node| (node, depth)));
    Ok(())
}
