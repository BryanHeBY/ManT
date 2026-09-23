//! Reusable traversal over normalized document IR.

use crate::{
    AnchorMark, Block, DefinitionItem, DisplayRow, DisplayRun, Document, DocumentBodyMut,
    DocumentBodyRef, FixedBody, Heading, HeadingMark, Inline, LinkMark, ListItem, OwnerMark,
    RegionMark, Section, TableCell, TableRow,
};

/// Read-only depth-first traversal with overridable hooks.
pub trait Visit<'ir> {
    /// Visit authoritative document or section heading inlines.
    fn visit_heading(&mut self, heading: &'ir Heading) {
        walk_heading(self, heading);
    }
    /// Visit a document, descending into root blocks and sections by default.
    fn visit_document(&mut self, document: &'ir Document) {
        walk_document(self, document);
    }

    /// Visit a Fixed body through its native surface and mark tables.
    fn visit_fixed_body(&mut self, fixed: &'ir FixedBody) {
        walk_fixed_body(self, fixed);
    }

    /// Visit one final physical row.
    fn visit_display_row(&mut self, _row: &'ir DisplayRow) {}

    /// Visit one final run of visible bytes.
    fn visit_display_run(&mut self, _run: &'ir DisplayRun) {}

    /// Visit one native section mark.
    fn visit_heading_mark(&mut self, _mark: &'ir HeadingMark) {}

    /// Visit one native owner mark.
    fn visit_owner_mark(&mut self, _mark: &'ir OwnerMark) {}

    /// Visit one native link occurrence mark.
    fn visit_link_mark(&mut self, _mark: &'ir LinkMark) {}

    /// Visit one native anchor declaration mark.
    fn visit_anchor_mark(&mut self, _mark: &'ir AnchorMark) {}

    /// Visit one native region mark.
    fn visit_region_mark(&mut self, _mark: &'ir RegionMark) {}

    /// Visit a section, descending into its blocks and children by default.
    fn visit_section(&mut self, section: &'ir Section) {
        walk_section(self, section);
    }

    /// Visit a block, descending into nested semantic content by default.
    fn visit_block(&mut self, block: &'ir Block) {
        walk_block(self, block);
    }

    /// Visit a definition, descending into terms and descriptions by default.
    fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
        walk_definition_item(self, item);
    }

    /// Visit an ordinary list owner, including any attached semantic facts.
    fn visit_list_item(&mut self, item: &'ir ListItem) {
        walk_list_item(self, item);
    }

    /// Visit an inline node, descending into styled/link children by default.
    fn visit_inline(&mut self, inline: &'ir Inline) {
        walk_inline(self, inline);
    }
}

/// Apply the default immutable traversal for a document.
pub fn walk_document<'ir, V>(visitor: &mut V, document: &'ir Document)
where
    V: Visit<'ir> + ?Sized,
{
    match document.body() {
        DocumentBodyRef::Flow(flow) => {
            if let Some(heading) = flow.heading {
                visitor.visit_heading(heading);
            }
            walk_blocks(visitor, flow.blocks);
            for section in flow.sections {
                visitor.visit_section(section);
            }
        }
        DocumentBodyRef::Fixed(fixed) => visitor.visit_fixed_body(fixed),
    }
}

/// Walk every authoritative Fixed surface row/run and native mark once.
pub fn walk_fixed_body<'ir, V>(visitor: &mut V, fixed: &'ir FixedBody)
where
    V: Visit<'ir> + ?Sized,
{
    for row in &fixed.surface.rows {
        visitor.visit_display_row(row);
    }
    for run in &fixed.surface.runs {
        visitor.visit_display_run(run);
    }
    for mark in &fixed.headings {
        visitor.visit_heading_mark(mark);
    }
    for mark in &fixed.owners {
        visitor.visit_owner_mark(mark);
    }
    for mark in &fixed.links {
        visitor.visit_link_mark(mark);
    }
    for mark in &fixed.anchors {
        visitor.visit_anchor_mark(mark);
    }
    for mark in &fixed.regions {
        visitor.visit_region_mark(mark);
    }
}

/// Apply the default immutable traversal for a section.
pub fn walk_section<'ir, V>(visitor: &mut V, section: &'ir Section)
where
    V: Visit<'ir> + ?Sized,
{
    visitor.visit_heading(&section.heading);
    walk_blocks(visitor, &section.blocks);
    for child in &section.children {
        visitor.visit_section(child);
    }
}

/// Apply the default immutable traversal to heading content.
pub fn walk_heading<'ir, V: Visit<'ir> + ?Sized>(visitor: &mut V, heading: &'ir Heading) {
    walk_inlines(visitor, &heading.content);
}

/// Apply the default immutable traversal for a block.
pub fn walk_block<'ir, V>(visitor: &mut V, block: &'ir Block)
where
    V: Visit<'ir> + ?Sized,
{
    match block {
        Block::Paragraph { children, .. }
        | Block::Preformatted { children, .. }
        | Block::FixedDisplay { children, .. } => {
            walk_inlines(visitor, children);
        }
        Block::List { items, .. } => {
            for item in items {
                visitor.visit_list_item(item);
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                visitor.visit_definition_item(item);
            }
        }
        Block::Table { rows, .. } => {
            for TableRow { cells, .. } in rows {
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

/// Apply the default immutable traversal for an ordinary list item.
pub fn walk_list_item<'ir, V>(visitor: &mut V, item: &'ir ListItem)
where
    V: Visit<'ir> + ?Sized,
{
    walk_blocks(visitor, &item.blocks);
}

/// Apply the default immutable traversal for a definition item.
pub fn walk_definition_item<'ir, V>(visitor: &mut V, item: &'ir DefinitionItem)
where
    V: Visit<'ir> + ?Sized,
{
    for term in &item.terms {
        walk_inlines(visitor, term);
    }
    walk_blocks(visitor, &item.description);
}

/// Apply the default immutable traversal for an inline node.
pub fn walk_inline<'ir, V>(visitor: &mut V, inline: &'ir Inline)
where
    V: Visit<'ir> + ?Sized,
{
    match inline {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => walk_inlines(visitor, children),
        Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Anchor { .. }
        | Inline::LineBreak { .. } => {}
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
    /// Visit authoritative heading content mutably.
    fn visit_heading_mut(&mut self, heading: &mut Heading) {
        walk_heading_mut(self, heading);
    }
    /// Visit a document mutably, descending into all content by default.
    fn visit_document_mut(&mut self, document: &mut Document) {
        walk_document_mut(self, document);
    }

    /// Visit and mutate one Fixed body explicitly.
    fn visit_fixed_body_mut(&mut self, fixed: &mut FixedBody) {
        walk_fixed_body_mut(self, fixed);
    }

    /// Mutate one final physical row.
    fn visit_display_row_mut(&mut self, _row: &mut DisplayRow) {}

    /// Mutate one final run.
    fn visit_display_run_mut(&mut self, _run: &mut DisplayRun) {}

    /// Mutate one native section mark.
    fn visit_heading_mark_mut(&mut self, _mark: &mut HeadingMark) {}

    /// Mutate one native owner mark.
    fn visit_owner_mark_mut(&mut self, _mark: &mut OwnerMark) {}

    /// Mutate one native link mark.
    fn visit_link_mark_mut(&mut self, _mark: &mut LinkMark) {}

    /// Mutate one native anchor mark.
    fn visit_anchor_mark_mut(&mut self, _mark: &mut AnchorMark) {}

    /// Mutate one native region mark.
    fn visit_region_mark_mut(&mut self, _mark: &mut RegionMark) {}

    /// Visit a section mutably, descending into its blocks and children by default.
    fn visit_section_mut(&mut self, section: &mut Section) {
        walk_section_mut(self, section);
    }

    /// Visit a block mutably, descending into nested semantic content by default.
    fn visit_block_mut(&mut self, block: &mut Block) {
        walk_block_mut(self, block);
    }

    /// Visit a definition mutably, descending into terms and descriptions by default.
    fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
        walk_definition_item_mut(self, item);
    }

    /// Visit an ordinary item and its attached semantic facts mutably.
    fn visit_list_item_mut(&mut self, item: &mut ListItem) {
        walk_list_item_mut(self, item);
    }

    /// Visit an inline node mutably, descending into styled/link children by default.
    fn visit_inline_mut(&mut self, inline: &mut Inline) {
        walk_inline_mut(self, inline);
    }
}

/// Apply the default mutable traversal for a document.
pub fn walk_document_mut<V>(visitor: &mut V, document: &mut Document)
where
    V: VisitMut + ?Sized,
{
    match document.body_mut() {
        DocumentBodyMut::Flow(flow) => {
            if let Some(heading) = flow.heading {
                visitor.visit_heading_mut(heading);
            }
            walk_blocks_mut(visitor, flow.blocks);
            for section in flow.sections {
                visitor.visit_section_mut(section);
            }
        }
        DocumentBodyMut::Fixed(fixed) => visitor.visit_fixed_body_mut(fixed),
    }
}

/// Walk every mutable Fixed surface row/run and mark once.
pub fn walk_fixed_body_mut<V>(visitor: &mut V, fixed: &mut FixedBody)
where
    V: VisitMut + ?Sized,
{
    for row in &mut fixed.surface.rows {
        visitor.visit_display_row_mut(row);
    }
    for run in &mut fixed.surface.runs {
        visitor.visit_display_run_mut(run);
    }
    for mark in &mut fixed.headings {
        visitor.visit_heading_mark_mut(mark);
    }
    for mark in &mut fixed.owners {
        visitor.visit_owner_mark_mut(mark);
    }
    for mark in &mut fixed.links {
        visitor.visit_link_mark_mut(mark);
    }
    for mark in &mut fixed.anchors {
        visitor.visit_anchor_mark_mut(mark);
    }
    for mark in &mut fixed.regions {
        visitor.visit_region_mark_mut(mark);
    }
}

/// Apply the default mutable traversal for a section.
pub fn walk_section_mut<V>(visitor: &mut V, section: &mut Section)
where
    V: VisitMut + ?Sized,
{
    visitor.visit_heading_mut(&mut section.heading);
    walk_blocks_mut(visitor, &mut section.blocks);
    for child in &mut section.children {
        visitor.visit_section_mut(child);
    }
}

/// Apply the default mutable traversal to heading content.
pub fn walk_heading_mut<V: VisitMut + ?Sized>(visitor: &mut V, heading: &mut Heading) {
    walk_inlines_mut(visitor, &mut heading.content);
}

/// Apply the default mutable traversal for a block.
pub fn walk_block_mut<V>(visitor: &mut V, block: &mut Block)
where
    V: VisitMut + ?Sized,
{
    match block {
        Block::Paragraph { children, .. }
        | Block::Preformatted { children, .. }
        | Block::FixedDisplay { children, .. } => {
            walk_inlines_mut(visitor, children);
        }
        Block::List { items, .. } => {
            for item in items {
                visitor.visit_list_item_mut(item);
            }
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                visitor.visit_definition_item_mut(item);
            }
        }
        Block::Table { rows, .. } => {
            for TableRow { cells, .. } in rows {
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

/// Apply the default mutable traversal for an ordinary list item.
pub fn walk_list_item_mut<V>(visitor: &mut V, item: &mut ListItem)
where
    V: VisitMut + ?Sized,
{
    walk_blocks_mut(visitor, &mut item.blocks);
}

/// Apply the default mutable traversal for a definition item.
pub fn walk_definition_item_mut<V>(visitor: &mut V, item: &mut DefinitionItem)
where
    V: VisitMut + ?Sized,
{
    for term in &mut item.terms {
        walk_inlines_mut(visitor, term);
    }
    walk_blocks_mut(visitor, &mut item.description);
}

/// Apply the default mutable traversal for an inline node.
pub fn walk_inline_mut<V>(visitor: &mut V, inline: &mut Inline)
where
    V: VisitMut + ?Sized,
{
    match inline {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => walk_inlines_mut(visitor, children),
        Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Anchor { .. }
        | Inline::LineBreak { .. } => {}
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
