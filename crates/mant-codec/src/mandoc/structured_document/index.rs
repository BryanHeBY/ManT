use libmandoc_rs::structured::StructuredDocument;

use super::NativeProjectionError;

pub(super) struct NativeLoweringIndex {
    pub(super) block_children: Vec<Vec<usize>>,
    pub(super) blocks_by_owner: Vec<Vec<usize>>,
    pub(super) list_by_block: Vec<Option<usize>>,
    pub(super) items_by_list: Vec<Vec<usize>>,
}

impl NativeLoweringIndex {
    pub(super) fn new(native: &StructuredDocument) -> Result<Self, NativeProjectionError> {
        let mut block_children = empty_index_buckets(native.blocks().len() + 1)?;
        let mut blocks_by_owner = empty_index_buckets(native.owners().len() + 1)?;
        for (index, block) in native.blocks().iter().enumerate() {
            let parent = block.parent().map_or(0, |parent| parent.get() as usize);
            push_index(
                block_children
                    .get_mut(parent)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "block parent is outside the lowering index",
                    ))?,
                index,
            )?;
            push_index(
                blocks_by_owner
                    .get_mut(block.owner().get() as usize)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "block owner is outside the lowering index",
                    ))?,
                index,
            )?;
        }
        let mut list_by_block = Vec::new();
        list_by_block
            .try_reserve_exact(native.blocks().len())
            .map_err(|_| NativeProjectionError::InvalidRelation("list index allocation"))?;
        list_by_block.resize(native.blocks().len(), None);
        for (index, list) in native.lists().iter().enumerate() {
            let block = list.block().get() as usize - 1;
            let slot =
                list_by_block
                    .get_mut(block)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "list block is outside the lowering index",
                    ))?;
            if slot.replace(index).is_some() {
                return Err(NativeProjectionError::InvalidRelation(
                    "list block is duplicated in the lowering index",
                ));
            }
        }
        let mut items_by_list = empty_index_buckets(native.lists().len())?;
        for (index, item) in native.items().iter().enumerate() {
            push_index(
                items_by_list
                    .get_mut(item.list().get() as usize - 1)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "item list is outside the lowering index",
                    ))?,
                index,
            )?;
        }
        Ok(Self {
            block_children,
            blocks_by_owner,
            list_by_block,
            items_by_list,
        })
    }

    pub(super) fn block_children(
        &self,
        parent: Option<libmandoc_rs::structured::NativeBlockKey>,
    ) -> &[usize] {
        let index = parent.map_or(0, |parent| parent.get() as usize);
        self.block_children.get(index).map_or(&[], Vec::as_slice)
    }

    pub(super) fn owner_blocks(&self, owner: libmandoc_rs::structured::OwnerKey) -> &[usize] {
        self.blocks_by_owner
            .get(owner.get() as usize)
            .map_or(&[], Vec::as_slice)
    }
}

fn empty_index_buckets(length: usize) -> Result<Vec<Vec<usize>>, NativeProjectionError> {
    let mut buckets = Vec::new();
    buckets
        .try_reserve_exact(length)
        .map_err(|_| NativeProjectionError::InvalidRelation("lowering index allocation"))?;
    buckets.resize_with(length, Vec::new);
    Ok(buckets)
}

fn push_index(bucket: &mut Vec<usize>, index: usize) -> Result<(), NativeProjectionError> {
    bucket
        .try_reserve(1)
        .map_err(|_| NativeProjectionError::InvalidRelation("lowering index allocation"))?;
    bucket.push(index);
    Ok(())
}
