use libmandoc_rs::structured::{NativeBlock, NativeBlockKind, NativeItem, NativeListKind};
use mant_ir::{
    Block, DefinitionItem, DefinitionLayout, Heading, LayoutHint, ListItem, ListItemLayout,
    ListKind, Section,
};

use super::{
    NativeHeadEvidence, NativeProjectionError, NativeProseProjection,
    address::AddressPlan,
    content::{root_inlines, source_for},
    evidence::{evidence_role, item_term_roots, native_declaration_evidence},
    index::NativeLoweringIndex,
    store::NativeContentMap,
};

pub(super) fn lower_section(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    block: &NativeBlock,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    evidence: &mut NativeHeadEvidence,
) -> Result<Section, NativeProjectionError> {
    let heading_content = root_inlines(
        projection,
        addresses,
        content,
        block.root().ok_or(NativeProjectionError::InvalidRelation(
            "heading block has no root",
        ))?,
    )?;
    let address = addresses.section(block.key())?;
    let mut blocks = Vec::new();
    for &child_index in index.block_children(Some(block.key())) {
        let child = &projection.document().blocks()[child_index];
        push_lowered_block(
            &mut blocks,
            lower_block(projection, index, addresses, content, child, None, evidence)?,
        );
    }
    Ok(Section {
        id: address.id().clone(),
        fragment_aliases: address.aliases().to_vec(),
        heading: Heading {
            content: heading_content,
            source: source_for(projection, block.provenance()),
        },
        spacing_before_lines: 0,
        blocks,
        children: Vec::new(),
        source: source_for(projection, block.provenance()),
    })
}

pub(super) fn lower_block(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    block: &NativeBlock,
    owner: Option<libmandoc_rs::structured::OwnerKey>,
    evidence: &mut NativeHeadEvidence,
) -> Result<Block, NativeProjectionError> {
    if owner.is_some_and(|owner| owner != block.owner()) {
        return Err(NativeProjectionError::InvalidRelation(
            "list child block belongs to another item",
        ));
    }
    match block.kind() {
        NativeBlockKind::Paragraph => Ok(Block::Paragraph {
            children: root_inlines(
                projection,
                addresses,
                content,
                block.root().ok_or(NativeProjectionError::InvalidRelation(
                    "paragraph block has no root",
                ))?,
            )?,
            layout: LayoutHint::default(),
            source: source_for(projection, block.provenance()),
        }),
        NativeBlockKind::List | NativeBlockKind::DefinitionList => {
            lower_list(projection, index, addresses, content, block, evidence)
        }
        kind => Err(NativeProjectionError::UnsupportedBlock(kind)),
    }
}

fn lower_list(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    block: &NativeBlock,
    evidence: &mut NativeHeadEvidence,
) -> Result<Block, NativeProjectionError> {
    let native = projection.document();
    let list_index = index
        .list_by_block
        .get(block.key().get() as usize - 1)
        .copied()
        .flatten()
        .ok_or(NativeProjectionError::InvalidRelation(
            "list block has no list record",
        ))?;
    let list = &native.lists()[list_index];
    if list.kind() == NativeListKind::NativeMarker {
        return Err(NativeProjectionError::InvalidRelation(
            "native marker list reached the codec without source classification",
        ));
    }
    if list.kind() == NativeListKind::Definition {
        let mut items = Vec::new();
        for &item_index in &index.items_by_list[list_index] {
            items.push(lower_definition_item(
                projection,
                index,
                addresses,
                content,
                block,
                &native.items()[item_index],
                evidence,
            )?);
        }
        return Ok(Block::DefinitionList {
            items,
            declaration_groups: Vec::new(),
            compact: list.compact(),
            layout: LayoutHint::default(),
            source: source_for(projection, list.provenance()),
        });
    }

    let kind = match list.kind() {
        NativeListKind::Bullet => ListKind::Bullet,
        NativeListKind::Ordered => ListKind::Ordered {
            start: list.start().map(u64::from),
        },
        NativeListKind::Plain => ListKind::Plain,
        NativeListKind::Definition | NativeListKind::NativeMarker => unreachable!(),
    };
    let mut items = Vec::new();
    for &item_index in &index.items_by_list[list_index] {
        items.push(lower_list_item(
            projection,
            index,
            addresses,
            content,
            block,
            &native.items()[item_index],
            evidence,
        )?);
    }
    Ok(Block::List {
        kind,
        compact: list.compact(),
        items,
        layout: LayoutHint::default(),
        source: source_for(projection, list.provenance()),
    })
}

fn lower_definition_item(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    list_block: &NativeBlock,
    item: &NativeItem,
    evidence: &mut NativeHeadEvidence,
) -> Result<DefinitionItem, NativeProjectionError> {
    let native = projection.document();
    let source = source_for(projection, item.provenance());
    let mut terms = Vec::new();
    for root in item_term_roots(native, content, item)? {
        terms.push(root_inlines(projection, addresses, content, root)?);
    }
    let description = item_blocks(
        projection, index, addresses, content, list_block, item, evidence,
    )?;
    if terms.is_empty() && description.is_empty() {
        let anchors = addresses.owner_anchors(item.owner(), content)?;
        if !anchors.is_empty() {
            terms.push(anchors);
        }
    }
    let lowered = DefinitionItem {
        source,
        entry: None,
        terms,
        description,
        layout: DefinitionLayout::default(),
    };
    if let Some(role) = evidence_role(native, item) {
        evidence.record(&lowered, role);
    }
    evidence.record_declaration(
        &lowered,
        native_declaration_evidence(projection, content, item)?,
    );
    Ok(lowered)
}

fn lower_list_item(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    list_block: &NativeBlock,
    item: &NativeItem,
    evidence: &mut NativeHeadEvidence,
) -> Result<ListItem, NativeProjectionError> {
    let source = source_for(projection, item.provenance());
    let mut blocks = item_blocks(
        projection, index, addresses, content, list_block, item, evidence,
    )?;
    if blocks.is_empty() {
        let anchors = addresses.owner_anchors(item.owner(), content)?;
        if !anchors.is_empty() {
            blocks.push(Block::Paragraph {
                children: anchors,
                layout: LayoutHint::default(),
                source,
            });
        }
    }
    Ok(ListItem {
        layout: ListItemLayout::default(),
        source,
        entry: None,
        blocks,
    })
}

fn item_blocks(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    list_block: &NativeBlock,
    item: &NativeItem,
    evidence: &mut NativeHeadEvidence,
) -> Result<Vec<Block>, NativeProjectionError> {
    let mut blocks = Vec::new();
    for &child_index in index.owner_blocks(item.owner()) {
        let child = &projection.document().blocks()[child_index];
        if child.parent() != Some(list_block.key()) {
            continue;
        }
        push_lowered_block(
            &mut blocks,
            lower_block(
                projection,
                index,
                addresses,
                content,
                child,
                Some(item.owner()),
                evidence,
            )?,
        );
    }
    Ok(blocks)
}

pub(super) fn push_lowered_block(blocks: &mut Vec<Block>, block: Block) {
    blocks.push(block);
}
