//! Resolve one requested root and establish its original ancestor frames.
use super::{
    Block, ControlFlow, Document, EntryOwner, NavigationEvent, OwnerFrame, ReferenceScanStop,
    ReferenceScope, ReferenceWorkBudget, Scan, Step,
};

impl<'ir, F> Scan<'ir, F>
where
    F: for<'path> FnMut(NavigationEvent<'ir, 'path>, &mut ReferenceWorkBudget) -> ControlFlow<()>,
{
    pub(super) fn scope(
        &mut self,
        document: &'ir Document,
        scope: ReferenceScope<'_>,
    ) -> Result<(), ReferenceScanStop> {
        match scope {
            ReferenceScope::Document => self.document(document),
            ReferenceScope::Overview => self.overview(document),
            ReferenceScope::Section(sections) => {
                self.charge(
                    sections.len(),
                    sections.len(),
                    sections.len().saturating_mul(std::mem::size_of::<u32>()),
                )?;
                let section = crate::resolve_content_section(document, sections)
                    .ok_or(ReferenceScanStop::InvalidRoot)?;
                self.sections.extend_from_slice(sections);
                self.section(section)
            }
            ReferenceScope::Block { sections, blocks } => {
                let block = self.select_block(document, sections, blocks)?;
                self.block(block)
            }
            ReferenceScope::Owner(owner) => {
                let block = self.select_block(document, owner.sections, owner.blocks)?;
                let item = match block {
                    Block::List { items, .. } => EntryOwner::List(
                        items
                            .get(owner.item_index as usize)
                            .ok_or(ReferenceScanStop::InvalidRoot)?,
                    ),
                    Block::DefinitionList { items, .. } => EntryOwner::Definition(
                        items
                            .get(owner.item_index as usize)
                            .ok_or(ReferenceScanStop::InvalidRoot)?,
                    ),
                    _ => return Err(ReferenceScanStop::InvalidRoot),
                };
                self.item(item, owner.item_index)
            }
        }
    }

    fn select_block(
        &mut self,
        document: &'ir Document,
        sections: &[u32],
        path: &[Step],
    ) -> Result<&'ir Block, ReferenceScanStop> {
        let depth = sections.len().saturating_add(path.len());
        let bytes = sections
            .len()
            .saturating_mul(std::mem::size_of::<u32>())
            .saturating_add(path.len().saturating_mul(std::mem::size_of::<Step>()));
        self.charge(depth, depth, bytes)?;
        let root = crate::content_location::content_blocks(document, sections)
            .ok_or(ReferenceScanStop::InvalidRoot)?;
        let (Step::Block { index }, rest) =
            path.split_first().ok_or(ReferenceScanStop::InvalidRoot)?
        else {
            return Err(ReferenceScanStop::InvalidRoot);
        };
        if !rest.len().is_multiple_of(2) {
            return Err(ReferenceScanStop::InvalidRoot);
        }
        let mut block = root
            .get(*index as usize)
            .ok_or(ReferenceScanStop::InvalidRoot)?;
        // All scratch growth is bounded before copying any path. Ancestor
        // frames retain prefix lengths, not a separate path allocation each.
        self.sections.extend_from_slice(sections);
        self.blocks.push(path[0]);
        for pair in rest.as_chunks::<2>().0 {
            let ancestor = match (block, pair[0]) {
                (Block::List { items, .. }, Step::ListItem { index }) => Some((
                    EntryOwner::List(
                        items
                            .get(index as usize)
                            .ok_or(ReferenceScanStop::InvalidRoot)?,
                    ),
                    index,
                )),
                (Block::DefinitionList { items, .. }, Step::DefinitionItem { index }) => Some((
                    EntryOwner::Definition(
                        items
                            .get(index as usize)
                            .ok_or(ReferenceScanStop::InvalidRoot)?,
                    ),
                    index,
                )),
                _ => None,
            };
            if let Some((owner, item)) = ancestor {
                let frame = OwnerFrame {
                    owner,
                    item,
                    blocks_len: self.blocks.len(),
                };
                self.owner = Some(frame);
                if owner.facts().is_some() {
                    self.semantic = Some(frame);
                }
            }
            let children = crate::content_location::block_children(block, pair[0])
                .ok_or(ReferenceScanStop::InvalidRoot)?;
            let Step::Block { index } = pair[1] else {
                return Err(ReferenceScanStop::InvalidRoot);
            };
            block = children
                .get(index as usize)
                .ok_or(ReferenceScanStop::InvalidRoot)?;
            self.blocks.extend_from_slice(pair);
        }
        Ok(block)
    }
}
