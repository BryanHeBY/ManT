use std::collections::HashSet;

use libmandoc_rs::structured::{NativeBlock, NativeBlockKind, NativeItem, NativeListKind};
use mant_ir::{
    Block, DefinitionItem, DefinitionLayout, Heading, Inline, LayoutHint, ListItem, ListItemLayout,
    ListKind, NodeId, Section,
};

use super::{
    NativeHeadEvidence, NativeProjectionError, NativeProseProjection,
    content::{root_inlines, source_for},
    evidence::{evidence_role, item_term_roots, native_declaration_evidence},
    index::NativeLoweringIndex,
};

pub(super) fn lower_section(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    block: &NativeBlock,
    used_ids: &mut HashSet<String>,
    evidence: &mut NativeHeadEvidence,
) -> Result<Section, NativeProjectionError> {
    let heading_content = root_inlines(
        projection,
        block.root().ok_or(NativeProjectionError::InvalidRelation(
            "heading block has no root",
        ))?,
    )?;
    let label = mant_ir::inline_plain_text(&heading_content);
    let base = crate::definitions::document_id_slug(&label);
    let id = unique_id(if base.is_empty() { "section" } else { &base }, used_ids);
    let mut blocks = Vec::new();
    for &child_index in index.block_children(Some(block.key())) {
        let child = &projection.document().blocks()[child_index];
        push_lowered_block(
            &mut blocks,
            lower_block(projection, index, child, None, evidence)?,
        );
    }
    Ok(Section {
        id: NodeId::new(id),
        fragment_aliases: Vec::new(),
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
                block.root().ok_or(NativeProjectionError::InvalidRelation(
                    "paragraph block has no root",
                ))?,
            )?,
            layout: LayoutHint::default(),
            source: source_for(projection, block.provenance()),
        }),
        NativeBlockKind::List | NativeBlockKind::DefinitionList => {
            lower_list(projection, index, block, evidence)
        }
        kind => Err(NativeProjectionError::UnsupportedBlock(kind)),
    }
}

fn lower_list(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
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
    list_block: &NativeBlock,
    item: &NativeItem,
    evidence: &mut NativeHeadEvidence,
) -> Result<DefinitionItem, NativeProjectionError> {
    let native = projection.document();
    let source = source_for(projection, item.provenance());
    let mut terms = Vec::new();
    for root in item_term_roots(native, item)? {
        terms.push(root_inlines(projection, root)?);
    }
    if let Some(target) = item.target() {
        let anchor = Inline::anchor_at(target, source);
        if let Some(term) = terms.first_mut() {
            term.insert(0, anchor);
        } else {
            terms.push(vec![anchor]);
        }
    }
    let lowered = DefinitionItem {
        source,
        entry: None,
        terms,
        description: item_blocks(projection, index, list_block, item, evidence)?,
        layout: DefinitionLayout::default(),
    };
    if let Some(role) = evidence_role(native, item) {
        evidence.record(&lowered, role);
    }
    evidence.record_declaration(&lowered, native_declaration_evidence(projection, item)?);
    Ok(lowered)
}

fn lower_list_item(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    list_block: &NativeBlock,
    item: &NativeItem,
    evidence: &mut NativeHeadEvidence,
) -> Result<ListItem, NativeProjectionError> {
    let source = source_for(projection, item.provenance());
    let mut blocks = item_blocks(projection, index, list_block, item, evidence)?;
    if let Some(target) = item.target() {
        prepend_anchor(&mut blocks, Inline::anchor_at(target, source));
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
            lower_block(projection, index, child, Some(item.owner()), evidence)?,
        );
    }
    Ok(blocks)
}

fn prepend_anchor(blocks: &mut Vec<Block>, anchor: Inline) {
    if let Some(Block::Paragraph { children, .. }) = blocks.first_mut() {
        children.insert(0, anchor);
    } else {
        blocks.insert(
            0,
            Block::Paragraph {
                children: vec![anchor],
                layout: LayoutHint::default(),
                source: None,
            },
        );
    }
}

pub(super) fn push_lowered_block(blocks: &mut Vec<Block>, block: Block) {
    blocks.push(block);
}

fn unique_id(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_owned()) {
        return base.to_owned();
    }
    for suffix in 2_u64.. {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}
