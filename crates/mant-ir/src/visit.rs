//! Reusable traversal over normalized document IR.

use crate::{Block, DefinitionItem, Document, Inline, ListItem, Section, TableCell, TableRow};

/// Read-only depth-first traversal with overridable hooks.
pub trait Visit<'ir> {
    fn visit_document(&mut self, document: &'ir Document) {
        walk_document(self, document);
    }

    fn visit_section(&mut self, section: &'ir Section) {
        walk_section(self, section);
    }

    fn visit_block(&mut self, block: &'ir Block) {
        walk_block(self, block);
    }

    fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
        walk_definition_item(self, item);
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        walk_inline(self, inline);
    }
}

pub fn walk_document<'ir, V>(visitor: &mut V, document: &'ir Document)
where
    V: Visit<'ir> + ?Sized,
{
    walk_blocks(visitor, &document.blocks);
    for section in &document.sections {
        visitor.visit_section(section);
    }
}

pub fn walk_section<'ir, V>(visitor: &mut V, section: &'ir Section)
where
    V: Visit<'ir> + ?Sized,
{
    walk_blocks(visitor, &section.blocks);
    for child in &section.children {
        visitor.visit_section(child);
    }
}

pub fn walk_block<'ir, V>(visitor: &mut V, block: &'ir Block)
where
    V: Visit<'ir> + ?Sized,
{
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            walk_inlines(visitor, children);
        }
        Block::List { items, .. } => {
            for ListItem { blocks } in items {
                walk_blocks(visitor, blocks);
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                visitor.visit_definition_item(item);
            }
        }
        Block::Table { rows, .. } => {
            for TableRow { cells } in rows {
                for TableCell { blocks, .. } in cells {
                    walk_blocks(visitor, blocks);
                }
            }
        }
        Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => {}
    }
}

pub fn walk_definition_item<'ir, V>(visitor: &mut V, item: &'ir DefinitionItem)
where
    V: Visit<'ir> + ?Sized,
{
    for term in &item.terms {
        walk_inlines(visitor, term);
    }
    walk_blocks(visitor, &item.description);
}

pub fn walk_inline<'ir, V>(visitor: &mut V, inline: &'ir Inline)
where
    V: Visit<'ir> + ?Sized,
{
    match inline {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => walk_inlines(visitor, children),
        Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {}
    }
}

fn walk_blocks<'ir, V>(visitor: &mut V, blocks: &'ir [Block])
where
    V: Visit<'ir> + ?Sized,
{
    for block in blocks {
        visitor.visit_block(block);
    }
}

fn walk_inlines<'ir, V>(visitor: &mut V, inlines: &'ir [Inline])
where
    V: Visit<'ir> + ?Sized,
{
    for inline in inlines {
        visitor.visit_inline(inline);
    }
}

/// Mutable depth-first traversal with overridable hooks.
pub trait VisitMut {
    fn visit_document_mut(&mut self, document: &mut Document) {
        walk_document_mut(self, document);
    }

    fn visit_section_mut(&mut self, section: &mut Section) {
        walk_section_mut(self, section);
    }

    fn visit_block_mut(&mut self, block: &mut Block) {
        walk_block_mut(self, block);
    }

    fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
        walk_definition_item_mut(self, item);
    }

    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        walk_inline_mut(self, inline);
    }
}

pub fn walk_document_mut<V>(visitor: &mut V, document: &mut Document)
where
    V: VisitMut + ?Sized,
{
    walk_blocks_mut(visitor, &mut document.blocks);
    for section in &mut document.sections {
        visitor.visit_section_mut(section);
    }
}

pub fn walk_section_mut<V>(visitor: &mut V, section: &mut Section)
where
    V: VisitMut + ?Sized,
{
    walk_blocks_mut(visitor, &mut section.blocks);
    for child in &mut section.children {
        visitor.visit_section_mut(child);
    }
}

pub fn walk_block_mut<V>(visitor: &mut V, block: &mut Block)
where
    V: VisitMut + ?Sized,
{
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            walk_inlines_mut(visitor, children);
        }
        Block::List { items, .. } => {
            for ListItem { blocks } in items {
                walk_blocks_mut(visitor, blocks);
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                visitor.visit_definition_item_mut(item);
            }
        }
        Block::Table { rows, .. } => {
            for TableRow { cells } in rows {
                for TableCell { blocks, .. } in cells {
                    walk_blocks_mut(visitor, blocks);
                }
            }
        }
        Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => {}
    }
}

pub fn walk_definition_item_mut<V>(visitor: &mut V, item: &mut DefinitionItem)
where
    V: VisitMut + ?Sized,
{
    for term in &mut item.terms {
        walk_inlines_mut(visitor, term);
    }
    walk_blocks_mut(visitor, &mut item.description);
}

pub fn walk_inline_mut<V>(visitor: &mut V, inline: &mut Inline)
where
    V: VisitMut + ?Sized,
{
    match inline {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => walk_inlines_mut(visitor, children),
        Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {}
    }
}

fn walk_blocks_mut<V>(visitor: &mut V, blocks: &mut [Block])
where
    V: VisitMut + ?Sized,
{
    for block in blocks {
        visitor.visit_block_mut(block);
    }
}

fn walk_inlines_mut<V>(visitor: &mut V, inlines: &mut [Inline])
where
    V: VisitMut + ?Sized,
{
    for inline in inlines {
        visitor.visit_inline_mut(inline);
    }
}
