//! ABI ranges, keys, and ordered relation validation.

mod fixed;
mod preflight;
mod scalar;
mod table;

use fixed::validate_fixed;
pub(super) use preflight::transfer_preflight;
use preflight::validate_utf8_view;
use scalar::ScalarBoundaryIndex;
use table::validate_tables;

use super::{
    ATOM_BREAK_OPPORTUNITY, ATOM_HARD_BREAK, ATOM_TEXT, ATOM_WHITESPACE, BLOCK_DEFINITION_LIST,
    BLOCK_FIXED_DISPLAY, BLOCK_HEADING, BLOCK_LIST, BLOCK_PARAGRAPH, BLOCK_TABLE, BytesView,
    COORD_NATIVE_NORMALIZED_BYTES, DIAGNOSTIC_CODE_NATIVE_LAST, DIAGNOSTIC_STYLE,
    DIAGNOSTIC_UNSUPPORTED, FORMAT_MAN, FORMAT_MDOC, LINK_LABEL_CONTENT, LINK_LABEL_HARD_BREAK,
    LIST_BULLET, LIST_DEFINITION, LIST_NATIVE_MARKER, LIST_ORDERED, LIST_PLAIN, MetadataView,
    NativeStructuredError, OWNER_DEFINITION_ITEM, OWNER_KIND_LAST, OWNER_LIST_ITEM,
    PROVENANCE_AUTHORED, PROVENANCE_GENERATED, PROVENANCE_UNKNOWN, ROOT_BODY, ROOT_FIXED_BODY,
    ROOT_HEADING, ROOT_KIND_LAST, ROOT_TERM, ResultHandle, ResultView, STYLE_MASK, SliceView,
    SpanView, StructuredSlices, TARGET_ORIGIN_AUTHORED, TARGET_ORIGIN_GENERATED, alloc_error,
    relation_error,
};

pub(super) fn validate_metadata(metadata: MetadataView) -> Result<(), NativeStructuredError> {
    if !(FORMAT_MAN..=FORMAT_MDOC).contains(&metadata.macroset)
        || metadata.presence_flags & !0xff != 0
        || metadata.has_body > 1
        || metadata.reserved_bytes != [0; 3]
        || metadata.reserved != 0
    {
        return Err(relation_error());
    }
    for (flag, field) in [
        (1 << 0, metadata.title),
        (1 << 1, metadata.section),
        (1 << 2, metadata.volume),
        (1 << 3, metadata.operating_system),
        (1 << 4, metadata.architecture),
        (1 << 5, metadata.name),
        (1 << 6, metadata.date),
        (1 << 7, metadata.alias_target),
    ] {
        validate_utf8_view(field)?;
        if metadata.presence_flags & flag == 0 && (field.len != 0 || !field.ptr.is_null()) {
            return Err(relation_error());
        }
    }
    Ok(())
}

pub(super) fn valid_span(span: &SpanView) -> bool {
    let line_valid = if span.line_column_present == 0 {
        span.line_start == 0 && span.column_start == 0 && span.line_end == 0 && span.column_end == 0
    } else {
        span.line_start != 0
            && span.column_start != 0
            && ((span.line_end == 0 && span.column_end == 0)
                || (span.line_end >= span.line_start
                    && span.column_end != 0
                    && (span.line_end != span.line_start || span.column_end >= span.column_start)))
    };
    let bytes_valid = span.byte_range_present == 0 && span.byte_start == 0 && span.byte_end == 0;
    line_valid && bytes_valid
}

pub(super) fn utf8_boundary(view: BytesView, offset: u32) -> bool {
    let Ok(length) = usize::try_from(view.len) else {
        return false;
    };
    let offset = offset as usize;
    if view.ptr.is_null() || offset > length {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(view.ptr, length) };
    std::str::from_utf8(bytes).is_ok_and(|text| text.is_char_boundary(offset))
}

fn utf8_scalar_count(view: BytesView, end: u64) -> Option<u32> {
    let length = usize::try_from(view.len).ok()?;
    let end = usize::try_from(end).ok()?;
    if view.ptr.is_null() || end > length {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(view.ptr, length) };
    let text = std::str::from_utf8(bytes).ok()?;
    let prefix = text.get(..end)?;
    u32::try_from(prefix.chars().count()).ok()
}

#[allow(clippy::too_many_lines)]
pub(super) fn validate_structured_relations(
    view: &ResultView,
    slices: &StructuredSlices<'_>,
) -> Result<(), NativeStructuredError> {
    if !slices.relations.is_empty() {
        return Err(relation_error());
    }

    for (index, source) in slices.sources.iter().enumerate() {
        if source.key != dense_key(index)?
            || source.reserved != 0
            || !(1..=3).contains(&source.identity_kind)
            || !(FORMAT_MAN..=FORMAT_MDOC).contains(&source.format)
            || source.coordinate_kind != COORD_NATIVE_NORMALIZED_BYTES
            || source.hash_present > 1
            || source.reserved_bytes != [0; 7]
            || (source.hash_present == 0 && source.hash != [0; 32])
        {
            return Err(relation_error());
        }
    }
    if view.root_source != 1 || slices.sources.is_empty() {
        return Err(relation_error());
    }
    if view.metadata.macroset != slices.sources[view.root_source as usize - 1].format {
        return Err(relation_error());
    }

    for span in slices.spans {
        if span.reserved != 0
            || span.reserved_bytes != [0; 2]
            || span.line_column_present > 1
            || span.byte_range_present > 1
            || span.source == 0
            || span.source as usize > slices.sources.len()
            || !valid_span(span)
        {
            return Err(relation_error());
        }
    }
    for provenance in slices.provenances {
        let active = match provenance.kind {
            PROVENANCE_AUTHORED => {
                provenance.authored_span != 0
                    && provenance.authored_span as usize <= slices.spans.len()
                    && provenance.generated_trigger_span == 0
            }
            PROVENANCE_GENERATED => {
                provenance.authored_span == 0
                    && provenance.generated_trigger_span as usize <= slices.spans.len()
            }
            PROVENANCE_UNKNOWN => {
                provenance.authored_span == 0 && provenance.generated_trigger_span == 0
            }
            _ => false,
        };
        if provenance.reserved != 0 || !active {
            return Err(relation_error());
        }
    }

    for (index, owner) in slices.owners.iter().enumerate() {
        if owner.key != dense_key(index)?
            || !(1..=OWNER_KIND_LAST).contains(&owner.kind)
            || !valid_required_key(owner.provenance, slices.provenances.len())
            || owner.reserved != 0
        {
            return Err(relation_error());
        }
    }

    let mut root_ordinals = Vec::new();
    root_ordinals
        .try_reserve_exact(slices.owners.len())
        .map_err(alloc_error)?;
    root_ordinals.resize(slices.owners.len(), 0_u32);
    for (index, root) in slices.content_roots.iter().enumerate() {
        let owner_index = root.owner.checked_sub(1).map(|owner| owner as usize);
        let expected_root_ordinal = owner_index.and_then(|owner| root_ordinals.get_mut(owner));
        if root.key != dense_key(index)?
            || !valid_required_key(root.owner, slices.owners.len())
            || !(1..=ROOT_KIND_LAST).contains(&root.kind)
            || !valid_required_key(root.provenance, slices.provenances.len())
            || root.reserved != 0
            || expected_root_ordinal
                .as_ref()
                .is_none_or(|expected| root.ordinal != **expected)
        {
            return Err(relation_error());
        }
        let expected_root_ordinal = expected_root_ordinal.expect("validated owner ordinal");
        *expected_root_ordinal = (*expected_root_ordinal)
            .checked_add(1)
            .ok_or_else(relation_error)?;
    }

    let mut root_atom_counts = Vec::new();
    root_atom_counts
        .try_reserve_exact(slices.content_roots.len())
        .map_err(alloc_error)?;
    root_atom_counts.resize(slices.content_roots.len(), 0_u32);
    let mut root_scalar_totals = Vec::new();
    root_scalar_totals
        .try_reserve_exact(slices.content_roots.len())
        .map_err(alloc_error)?;
    root_scalar_totals.resize(slices.content_roots.len(), 0_u32);
    let mut atom_scalar_starts = Vec::new();
    atom_scalar_starts
        .try_reserve_exact(slices.content_atoms.len())
        .map_err(alloc_error)?;
    for (index, atom) in slices.content_atoms.iter().enumerate() {
        let root_index = atom.root.checked_sub(1).map(|index| index as usize);
        let root = root_index.and_then(|index| slices.content_roots.get(index));
        let text_length = validate_utf8_view(atom.text)?;
        let display_valid = match atom.display_override_present {
            0 => atom.display_override.ptr.is_null() && atom.display_override.len == 0,
            1 => {
                atom.display_override.len != 0 && validate_utf8_view(atom.display_override).is_ok()
            }
            _ => false,
        };
        let kind_valid = match atom.kind {
            ATOM_TEXT => text_length != 0 && atom.whitespace_breakable == 0,
            ATOM_WHITESPACE => text_length != 0 && atom.whitespace_breakable <= 1,
            ATOM_BREAK_OPPORTUNITY | ATOM_HARD_BREAK => {
                text_length == 0
                    && atom.display_override_present == 0
                    && atom.whitespace_breakable == 0
                    && atom.style_flags == 0
                    && atom.role == 0
                    && (atom.kind != ATOM_BREAK_OPPORTUNITY || atom.link == 0)
            }
            _ => false,
        };
        let expected_atom_ordinal = root_index.and_then(|index| root_atom_counts.get_mut(index));
        let scalar_total = root_index.and_then(|index| root_scalar_totals.get_mut(index));
        if atom.key != dense_key(index)?
            || root.is_none()
            || root.is_some_and(|root| root.owner != atom.owner)
            || atom.style_flags & !STYLE_MASK != 0
            || atom.role > 5
            || atom.link as usize > slices.links.len()
            || !display_valid
            || !kind_valid
            || atom.display_reserved_bytes != [0; 7]
            || atom.reserved_bytes != [0; 3]
            || !valid_required_key(atom.provenance, slices.provenances.len())
            || atom.reserved != 0
            || expected_atom_ordinal
                .as_ref()
                .is_none_or(|expected| atom.ordinal != **expected)
            || scalar_total.is_none()
        {
            return Err(relation_error());
        }
        let expected_atom_ordinal = expected_atom_ordinal.expect("validated atom root");
        *expected_atom_ordinal = expected_atom_ordinal
            .checked_add(1)
            .ok_or_else(relation_error)?;
        let scalar_total = scalar_total.expect("validated atom scalar root");
        atom_scalar_starts.push(*scalar_total);
        let added_scalars = match atom.kind {
            ATOM_TEXT | ATOM_WHITESPACE => {
                utf8_scalar_count(atom.text, atom.text.len).ok_or_else(relation_error)?
            }
            ATOM_HARD_BREAK => 1,
            ATOM_BREAK_OPPORTUNITY => 0,
            _ => unreachable!("atom kind validated above"),
        };
        *scalar_total = scalar_total
            .checked_add(added_scalars)
            .ok_or_else(relation_error)?;
    }

    // A bounded 64-byte checkpoint index makes both in-atom points and
    // physical placement boundaries independent of prefix length.  The
    // handle owns the text for the whole validation call; the index retains
    // counts only, never a borrowed native slice.
    let scalar_index = ScalarBoundaryIndex::build(slices.content_atoms)?;

    let mut root_atom_offsets = Vec::new();
    root_atom_offsets
        .try_reserve_exact(slices.content_roots.len() + 1)
        .map_err(alloc_error)?;
    root_atom_offsets.push(0_usize);
    for count in &root_atom_counts {
        let next = root_atom_offsets
            .last()
            .copied()
            .expect("root atom offset seed")
            .checked_add(*count as usize)
            .ok_or_else(relation_error)?;
        root_atom_offsets.push(next);
    }
    if root_atom_offsets.last().copied() != Some(slices.content_atoms.len()) {
        return Err(relation_error());
    }
    let mut root_atoms = Vec::new();
    root_atoms
        .try_reserve_exact(slices.content_atoms.len())
        .map_err(alloc_error)?;
    root_atoms.resize(slices.content_atoms.len(), (0_u32, 0_u32));
    for (index, atom) in slices.content_atoms.iter().enumerate() {
        let root_index = atom.root as usize - 1;
        let position = root_atom_offsets[root_index]
            .checked_add(atom.ordinal as usize)
            .ok_or_else(relation_error)?;
        let Some(slot) = root_atoms.get_mut(position) else {
            return Err(relation_error());
        };
        if slot.0 != 0 {
            return Err(relation_error());
        }
        *slot = (atom.key, atom_scalar_starts[index]);
    }

    for content_ref in slices.content_refs {
        let atom = content_ref
            .atom
            .checked_sub(1)
            .and_then(|index| slices.content_atoms.get(index as usize));
        if content_ref.reserved != 0
            || atom.is_none()
            || atom.is_some_and(|atom| !matches!(atom.kind, ATOM_TEXT | ATOM_WHITESPACE))
            || content_ref.byte_start >= content_ref.byte_end
            || atom.is_some_and(|atom| u64::from(content_ref.byte_end) > atom.text.len)
            || atom.is_some_and(|atom| {
                !utf8_boundary(atom.text, content_ref.byte_start)
                    || !utf8_boundary(atom.text, content_ref.byte_end)
            })
        {
            return Err(relation_error());
        }
    }
    let mut point_ordinals = Vec::new();
    point_ordinals
        .try_reserve_exact(slices.content_roots.len())
        .map_err(alloc_error)?;
    point_ordinals.resize(slices.content_roots.len(), 0_u32);
    for (index, point) in slices.content_points.iter().enumerate() {
        let root = point
            .root
            .checked_sub(1)
            .and_then(|root| slices.content_roots.get(root as usize));
        let expected = point
            .root
            .checked_sub(1)
            .and_then(|root| point_ordinals.get_mut(root as usize));
        let scalar_boundary = match point.boundary_kind {
            1 => {
                let root_index = point.root.checked_sub(1).map(|root| root as usize);
                let count = root_index
                    .and_then(|root| root_atom_counts.get(root))
                    .copied();
                let boundary = point.atom_boundary as usize;
                if point.atom != 0
                    || point.byte_offset != 0
                    || count.is_none_or(|count| point.atom_boundary > count)
                {
                    None
                } else if point.atom_boundary == count.expect("validated point atom boundary") {
                    root_index
                        .and_then(|root| root_scalar_totals.get(root))
                        .copied()
                } else {
                    root_index
                        .and_then(|root| root_atom_offsets.get(root))
                        .and_then(|offset| offset.checked_add(boundary))
                        .and_then(|index| root_atoms.get(index))
                        .map(|(_, scalar)| *scalar)
                }
            }
            2 => {
                let atom_index = point.atom.checked_sub(1).map(|atom| atom as usize);
                let atom = atom_index.and_then(|atom| slices.content_atoms.get(atom));
                if point.atom_boundary != 0
                    || atom.is_none_or(|atom| {
                        atom.root != point.root
                            || !matches!(atom.kind, ATOM_TEXT | ATOM_WHITESPACE)
                            || u64::from(point.byte_offset) > atom.text.len
                            || !utf8_boundary(atom.text, point.byte_offset)
                    })
                {
                    None
                } else {
                    atom_index.and_then(|index| {
                        let start = *atom_scalar_starts.get(index)?;
                        let prefix = scalar_index.prefix(
                            index,
                            atom.expect("validated point atom").text,
                            point.byte_offset,
                        )?;
                        start.checked_add(prefix)
                    })
                }
            }
            _ => None,
        };
        if point.key != dense_key(index)?
            || root.is_none()
            || root.is_some_and(|root| root.owner != point.owner)
            || expected
                .as_ref()
                .is_none_or(|expected| point.ordinal != **expected)
            || scalar_boundary != Some(point.scalar_boundary)
            || !valid_required_key(point.provenance, slices.provenances.len())
            || point.reserved != 0
        {
            return Err(relation_error());
        }
        let expected = expected.expect("validated point root");
        *expected = expected.checked_add(1).ok_or_else(relation_error)?;
    }
    for (index, anchor) in slices.anchors.iter().enumerate() {
        let point = anchor
            .point
            .checked_sub(1)
            .and_then(|point| slices.content_points.get(point as usize));
        if anchor.key != dense_key(index)?
            || !valid_required_key(anchor.owner, slices.owners.len())
            || point.is_none_or(|point| point.owner != anchor.owner)
            || !matches!(
                anchor.origin,
                value if value == u32::from(TARGET_ORIGIN_GENERATED)
                    || value == u32::from(TARGET_ORIGIN_AUTHORED)
            )
            || validate_utf8_view(anchor.target).is_err()
            || anchor.target.len == 0
            || !valid_required_key(anchor.provenance, slices.provenances.len())
            || anchor.reserved != 0
        {
            return Err(relation_error());
        }
    }
    let mut linked_atom_refs = Vec::new();
    linked_atom_refs
        .try_reserve_exact(slices.content_atoms.len())
        .map_err(alloc_error)?;
    linked_atom_refs.resize(slices.content_atoms.len(), 0_u32);
    let mut next_label_part = 0_usize;
    for (index, link) in slices.links.iter().enumerate() {
        let label_start = link.first_label_part.checked_sub(1).map(|key| key as usize);
        let label_end =
            label_start.and_then(|start| start.checked_add(link.label_part_count as usize));
        let target_b_valid = match link.target_b_present {
            0 => link.target_b.ptr.is_null() && link.target_b.len == 0 && link.target_kind != 4,
            1 => {
                validate_utf8_view(link.target_b).is_ok()
                    && link.target_b.len != 0
                    && link.target_kind == 4
            }
            _ => false,
        };
        let title_valid = match link.title_present {
            0 => link.title.ptr.is_null() && link.title.len == 0,
            1 => validate_utf8_view(link.title).is_ok(),
            _ => false,
        };
        if link.key != dense_key(index)?
            || !valid_required_key(link.owner, slices.owners.len())
            || !(1..=5).contains(&link.target_kind)
            || validate_utf8_view(link.target_a).is_err()
            || link.target_a.len == 0
            || !target_b_valid
            || !title_valid
            || link.target_b_reserved_bytes != [0; 7]
            || link.title_reserved_bytes != [0; 7]
            || link.first_label_ref != 0
            || link.label_ref_count != 0
            || link.label_part_count == 0
            || label_start != Some(next_label_part)
            || label_end.is_none_or(|end| end > slices.link_label_parts.len())
            || !valid_required_key(link.provenance, slices.provenances.len())
            || link.reserved != 0
        {
            return Err(relation_error());
        }
        let mut previous_atom = 0;
        for part in &slices.link_label_parts[label_start.unwrap()..label_end.unwrap()] {
            let atom = part
                .atom
                .checked_sub(1)
                .and_then(|atom| slices.content_atoms.get(atom as usize))
                .ok_or_else(relation_error)?;
            let part_valid = match part.kind {
                LINK_LABEL_CONTENT => {
                    matches!(atom.kind, ATOM_TEXT | ATOM_WHITESPACE)
                        && part.byte_start == 0
                        && u64::from(part.byte_end) == atom.text.len
                }
                LINK_LABEL_HARD_BREAK => {
                    atom.kind == ATOM_HARD_BREAK && part.byte_start == 0 && part.byte_end == 0
                }
                _ => false,
            };
            if atom.link != link.key
                || part.atom <= previous_atom
                || part.reserved != 0
                || !part_valid
            {
                return Err(relation_error());
            }
            let seen = &mut linked_atom_refs[part.atom as usize - 1];
            *seen = seen.checked_add(1).ok_or_else(relation_error)?;
            previous_atom = part.atom;
        }
        next_label_part = label_end.expect("validated link label part range");
    }
    if next_label_part != slices.link_label_parts.len() {
        return Err(relation_error());
    }
    if slices
        .content_atoms
        .iter()
        .zip(&linked_atom_refs)
        .any(|(atom, seen)| (atom.link != 0) != (*seen == 1))
    {
        return Err(relation_error());
    }

    let mut top_level_ordinal = 0_u32;
    let mut child_ordinals = Vec::new();
    child_ordinals
        .try_reserve_exact(slices.blocks.len())
        .map_err(alloc_error)?;
    child_ordinals.resize(slices.blocks.len(), 0_u32);
    for (index, block) in slices.blocks.iter().enumerate() {
        let block_root = (block.root != 0)
            .then(|| slices.content_roots.get(block.root as usize - 1))
            .flatten();
        let expected_ordinal = if block.parent == 0 {
            let ordinal = top_level_ordinal;
            top_level_ordinal = top_level_ordinal
                .checked_add(1)
                .ok_or_else(relation_error)?;
            ordinal
        } else {
            let count = child_ordinals
                .get_mut(block.parent as usize - 1)
                .ok_or_else(relation_error)?;
            let ordinal = *count;
            *count = (*count).checked_add(1).ok_or_else(relation_error)?;
            ordinal
        };
        let payload_valid = match block.kind {
            BLOCK_HEADING => {
                block_root.is_some_and(|root| root.kind == ROOT_HEADING)
                    && block.table == 0
                    && block.fixed_view == 0
            }
            BLOCK_PARAGRAPH => {
                block_root.is_some_and(|root| root.kind == ROOT_BODY)
                    && block.table == 0
                    && block.fixed_view == 0
            }
            BLOCK_LIST | BLOCK_DEFINITION_LIST => {
                block.root == 0 && block.table == 0 && block.fixed_view == 0
            }
            BLOCK_TABLE => {
                block.root == 0
                    && block.table != 0
                    && block.fixed_view as usize <= slices.fixed_views.len()
            }
            BLOCK_FIXED_DISPLAY => {
                block_root.is_some_and(|root| root.kind == ROOT_FIXED_BODY)
                    && block.table == 0
                    && valid_required_key(block.fixed_view, slices.fixed_views.len())
            }
            _ => false,
        };
        if block.key != dense_key(index)?
            || !valid_required_key(block.owner, slices.owners.len())
            || !valid_required_key(block.provenance, slices.provenances.len())
            || block.parent as usize > slices.blocks.len()
            || block.parent >= block.key
            || block_root.is_some_and(|root| root.owner != block.owner)
            || !payload_valid
            || block.reserved != 0
            || block.ordinal != expected_ordinal
        {
            return Err(relation_error());
        }
    }
    let mut evidenced_headings = Vec::new();
    evidenced_headings
        .try_reserve_exact(slices.blocks.len())
        .map_err(alloc_error)?;
    evidenced_headings.resize(slices.blocks.len(), false);
    for (index, heading) in slices.heading_evidence.iter().enumerate() {
        let block_index = heading.block.checked_sub(1).map(|block| block as usize);
        let block = block_index.and_then(|block| slices.blocks.get(block));
        let phrase_valid = match heading.authored_phrase_present {
            0 => heading.authored_phrase.ptr.is_null() && heading.authored_phrase.len == 0,
            1 => {
                heading.authored_phrase.len != 0
                    && validate_utf8_view(heading.authored_phrase).is_ok()
            }
            _ => false,
        };
        if heading.key != dense_key(index)?
            || block.is_none_or(|block| block.kind != BLOCK_HEADING || block.owner != heading.owner)
            || !valid_required_key(heading.owner, slices.owners.len())
            || block_index
                .and_then(|block| evidenced_headings.get(block))
                .is_none_or(|evidenced| *evidenced)
            || !phrase_valid
            || heading.authored_phrase_reserved_bytes != [0; 7]
            || !valid_required_key(heading.provenance, slices.provenances.len())
            || heading.reserved != 0
        {
            return Err(relation_error());
        }
        evidenced_headings[block_index.expect("validated heading block")] = true;
    }
    if slices
        .blocks
        .iter()
        .enumerate()
        .any(|(index, block)| (block.kind == BLOCK_HEADING) != evidenced_headings[index])
    {
        return Err(relation_error());
    }

    let mut list_blocks = Vec::new();
    list_blocks
        .try_reserve_exact(slices.blocks.len())
        .map_err(alloc_error)?;
    list_blocks.resize(slices.blocks.len(), false);
    for (index, list) in slices.lists.iter().enumerate() {
        let block = list
            .block
            .checked_sub(1)
            .and_then(|block| slices.blocks.get(block as usize));
        let kind_valid = match list.kind {
            LIST_BULLET | LIST_PLAIN => {
                list.start == 0 && block.is_some_and(|block| block.kind == BLOCK_LIST)
            }
            LIST_ORDERED => list.start != 0 && block.is_some_and(|block| block.kind == BLOCK_LIST),
            LIST_DEFINITION | LIST_NATIVE_MARKER => {
                list.start == 0 && block.is_some_and(|block| block.kind == BLOCK_DEFINITION_LIST)
            }
            _ => false,
        };
        if list.key != dense_key(index)?
            || !kind_valid
            || list_blocks
                .get(list.block as usize - 1)
                .is_none_or(|used| *used)
            || list.compact > 1
            || !valid_required_key(list.provenance, slices.provenances.len())
            || list.reserved != 0
        {
            return Err(relation_error());
        }
        list_blocks[list.block as usize - 1] = true;
    }
    let mut item_ordinals = Vec::new();
    item_ordinals
        .try_reserve_exact(slices.lists.len())
        .map_err(alloc_error)?;
    item_ordinals.resize(slices.lists.len(), 0_u32);
    let mut item_owners = Vec::new();
    item_owners
        .try_reserve_exact(slices.owners.len())
        .map_err(alloc_error)?;
    item_owners.resize(slices.owners.len(), false);
    let mut next_form = 0_usize;
    let mut previous_item_owner = 0_u32;
    for (index, item) in slices.items.iter().enumerate() {
        let list_index = item.list.checked_sub(1).map(|list| list as usize);
        let expected = list_index.and_then(|list| item_ordinals.get_mut(list));
        let forms = if item.form_count == 0 {
            (item.first_form == 0).then_some(&[][..])
        } else {
            item.first_form.checked_sub(1).and_then(|first| {
                let start = first as usize;
                let end = start.checked_add(item.form_count as usize)?;
                slices.forms.get(start..end)
            })
        };
        let legacy_target_is_zero = item.target_present == 0
            && item.target.ptr.is_null()
            && item.target.len == 0
            && item.target_origin == 0;
        let expected_owner_kind = list_index
            .and_then(|list| slices.lists.get(list))
            .map(|list| {
                if matches!(list.kind, LIST_DEFINITION | LIST_NATIVE_MARKER) {
                    OWNER_DEFINITION_ITEM
                } else {
                    OWNER_LIST_ITEM
                }
            });
        let owner_index = item.owner.checked_sub(1).map(|owner| owner as usize);
        let first_form = item.first_form.checked_sub(1).map(|form| form as usize);
        if item.key != dense_key(index)?
            || !valid_required_key(item.list, slices.lists.len())
            || !valid_required_key(item.owner, slices.owners.len())
            || item.owner <= previous_item_owner
            || expected
                .as_ref()
                .is_none_or(|expected| item.ordinal != **expected)
            || owner_index
                .and_then(|owner| slices.owners.get(owner))
                .is_none_or(|owner| Some(owner.kind) != expected_owner_kind)
            || owner_index
                .and_then(|owner| item_owners.get(owner))
                .is_none_or(|used| *used)
            || (item.form_count != 0 && first_form != Some(next_form))
            || forms.is_none_or(|forms| forms.iter().any(|form| form.owner != item.owner))
            || !legacy_target_is_zero
            || item.target_reserved_bytes != [0; 6]
            || !valid_required_key(item.provenance, slices.provenances.len())
            || item.reserved != 0
        {
            return Err(relation_error());
        }
        item_owners[owner_index.expect("validated item owner")] = true;
        previous_item_owner = item.owner;
        next_form = next_form
            .checked_add(item.form_count as usize)
            .ok_or_else(relation_error)?;
        let expected = expected.expect("validated item ordinal");
        *expected = (*expected).checked_add(1).ok_or_else(relation_error)?;
    }
    if next_form != slices.forms.len()
        || slices.blocks.iter().enumerate().any(|(index, block)| {
            matches!(block.kind, BLOCK_LIST | BLOCK_DEFINITION_LIST) && !list_blocks[index]
        })
    {
        return Err(relation_error());
    }
    validate_tables(slices)?;
    validate_fixed(slices, &atom_scalar_starts, &scalar_index)?;
    let mut term_root_evidence = Vec::new();
    term_root_evidence
        .try_reserve_exact(slices.content_roots.len())
        .map_err(alloc_error)?;
    term_root_evidence.resize(slices.content_roots.len(), 0_u8);
    for (index, form) in slices.forms.iter().enumerate() {
        let start = form.first_ref.checked_sub(1).map(|first| first as usize);
        let end = start.and_then(|start| start.checked_add(form.ref_count as usize));
        if form.key != dense_key(index)?
            || !valid_required_key(form.owner, slices.owners.len())
            || form.role > 5
            || form.ref_count == 0
            || end.is_none_or(|end| end > slices.content_refs.len())
            || !valid_required_key(form.provenance, slices.provenances.len())
            || form.reserved != 0
        {
            return Err(relation_error());
        }
        let mut form_root = None;
        for content_ref in &slices.content_refs[start.unwrap()..end.unwrap()] {
            let atom = &slices.content_atoms[content_ref.atom as usize - 1];
            let root_index = atom.root as usize - 1;
            if atom.owner != form.owner
                || slices.content_roots[root_index].kind != ROOT_TERM
                || form_root.is_some_and(|root| root != root_index)
            {
                return Err(relation_error());
            }
            form_root = Some(root_index);
        }
        let form_root = form_root.ok_or_else(relation_error)?;
        if term_root_evidence[form_root] & 1 != 0 {
            return Err(relation_error());
        }
        term_root_evidence[form_root] |= 1;
    }
    for atom in slices.content_atoms {
        let root_index = atom.root as usize - 1;
        if atom.kind == ATOM_TEXT && slices.content_roots[root_index].kind == ROOT_TERM {
            term_root_evidence[root_index] |= 2;
        }
    }
    for (index, root) in slices.content_roots.iter().enumerate() {
        if root.kind == ROOT_TERM {
            let owner_index = root.owner as usize - 1;
            if term_root_evidence[index] & 1 == 0
                && (term_root_evidence[index] & 2 != 0 || !item_owners[owner_index])
            {
                return Err(relation_error());
            }
        } else if term_root_evidence[index] != 0 {
            return Err(relation_error());
        }
    }
    let mut previous_hint_form = 0_u32;
    for (index, hint) in slices.name_hints.iter().enumerate() {
        let form = hint
            .form
            .checked_sub(1)
            .and_then(|form| slices.forms.get(form as usize));
        let hint_end = hint.first_ref.checked_add(hint.ref_count);
        if hint.key != dense_key(index)?
            || form.is_none()
            || hint.ref_count == 0
            || hint_end.is_none()
            || hint.form < previous_hint_form
            || form.is_some_and(|form| {
                let Some(form_end) = form.first_ref.checked_add(form.ref_count) else {
                    return true;
                };
                hint.first_ref < form.first_ref
                    || hint_end.is_none_or(|hint_end| hint_end > form_end)
            })
            || !valid_required_key(hint.provenance, slices.provenances.len())
            || hint.reserved != 0
        {
            return Err(relation_error());
        }
        previous_hint_form = hint.form;
    }

    for diagnostic in slices.diagnostics {
        if diagnostic.reserved != 0
            || !(DIAGNOSTIC_STYLE..=DIAGNOSTIC_UNSUPPORTED).contains(&diagnostic.level)
            || !(1..=DIAGNOSTIC_CODE_NATIVE_LAST).contains(&diagnostic.code)
            || diagnostic.span as usize > slices.spans.len()
            || diagnostic.owner as usize > slices.owners.len()
        {
            return Err(relation_error());
        }
    }

    let has_collected_body = !slices.owners.is_empty()
        || !slices.content_roots.is_empty()
        || !slices.content_atoms.is_empty()
        || !slices.blocks.is_empty();
    if (view.metadata.has_body == 0 && has_collected_body)
        || (view.metadata.has_body == 1
            && (slices.owners.is_empty()
                || slices.content_roots.is_empty()
                || slices.blocks.is_empty()))
    {
        return Err(relation_error());
    }
    Ok(())
}

pub(super) fn dense_key(index: usize) -> Result<u32, NativeStructuredError> {
    u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(relation_error)
}

pub(super) fn valid_required_key(key: u32, length: usize) -> bool {
    key != 0 && key as usize <= length
}

#[allow(clippy::needless_lifetimes)] // The explicit lifetime documents handle-bound borrowing.
pub(super) fn checked_slice<'a, T>(
    view: SliceView,
    _handle: &'a ResultHandle,
) -> Result<&'a [T], NativeStructuredError> {
    if view.stride as usize != std::mem::size_of::<T>() {
        return Err(relation_error());
    }
    if view.count == 0 {
        return if view.ptr.is_null() {
            Ok(&[])
        } else {
            Err(relation_error())
        };
    }
    if view.ptr.is_null() || !(view.ptr as usize).is_multiple_of(std::mem::align_of::<T>()) {
        return Err(relation_error());
    }
    let count = view.count as usize;
    let bytes = count
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(relation_error)?;
    if bytes > isize::MAX as usize
        || (bytes != 0 && (view.ptr as usize).checked_add(bytes - 1).is_none())
    {
        return Err(relation_error());
    }
    Ok(unsafe { std::slice::from_raw_parts(view.ptr.cast(), count) })
}
