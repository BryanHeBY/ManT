//! Transfer budgets, ABI ranges, keys, and relation validation.

use super::{
    ATOM_BREAK_OPPORTUNITY, ATOM_HARD_BREAK, ATOM_TEXT, ATOM_WHITESPACE, BLOCK_DEFINITION_LIST,
    BLOCK_HEADING, BLOCK_LIST, BLOCK_PARAGRAPH, BytesView, COORD_NATIVE_NORMALIZED_BYTES,
    DIAGNOSTIC_CODE_NATIVE_LAST, DIAGNOSTIC_STYLE, DIAGNOSTIC_UNSUPPORTED, FORMAT_MAN, FORMAT_MDOC,
    LIST_BULLET, LIST_DEFINITION, LIST_NATIVE_MARKER, LIST_ORDERED, LIST_PLAIN, Limits,
    MetadataView, NativeStructuredError, OWNER_DEFINITION_ITEM, OWNER_KIND_LAST, OWNER_LIST_ITEM,
    OwnedBlock, OwnedContentAtom, OwnedContentRef, OwnedContentRoot, OwnedDiagnostic, OwnedLink,
    OwnedOwner, OwnedProvenance, OwnedSource, OwnedSpan, PROVENANCE_AUTHORED, PROVENANCE_GENERATED,
    PROVENANCE_UNKNOWN, ROOT_BODY, ROOT_HEADING, ROOT_KIND_LAST, ROOT_TERM, ResultHandle,
    ResultView, STATUS_BUDGET, STYLE_MASK, SliceView, SpanView, StructuredSlices, alloc_error,
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

#[allow(clippy::too_many_lines)]
pub(super) fn validate_structured_relations(
    view: &ResultView,
    slices: &StructuredSlices<'_>,
) -> Result<(), NativeStructuredError> {
    if !slices.content_points.is_empty()
        || !slices.tables.is_empty()
        || !slices.table_rows.is_empty()
        || !slices.table_cells.is_empty()
        || !slices.fixed_views.is_empty()
        || !slices.fixed_lines.is_empty()
        || !slices.placements.is_empty()
        || !slices.decorations.is_empty()
        || !slices.relations.is_empty()
    {
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

    let mut previous_root = 0_u32;
    let mut expected_atom_ordinal = 0_u32;
    for (index, atom) in slices.content_atoms.iter().enumerate() {
        let root = atom
            .root
            .checked_sub(1)
            .and_then(|index| slices.content_roots.get(index as usize));
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
                    && atom.link == 0
            }
            _ => false,
        };
        if atom.root != previous_root {
            previous_root = atom.root;
            expected_atom_ordinal = 0;
        }
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
            || atom.ordinal != expected_atom_ordinal
        {
            return Err(relation_error());
        }
        expected_atom_ordinal = expected_atom_ordinal
            .checked_add(1)
            .ok_or_else(relation_error)?;
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
    for (index, link) in slices.links.iter().enumerate() {
        let label_start = link.first_label_ref.checked_sub(1).map(|key| key as usize);
        let label_end =
            label_start.and_then(|start| start.checked_add(link.label_ref_count as usize));
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
            || link.label_ref_count == 0
            || label_start.is_none()
            || label_end.is_none_or(|end| end > slices.content_refs.len())
            || !valid_required_key(link.provenance, slices.provenances.len())
            || link.reserved != 0
        {
            return Err(relation_error());
        }
        for content_ref in &slices.content_refs[label_start.unwrap()..label_end.unwrap()] {
            if slices.content_atoms[content_ref.atom as usize - 1].link != link.key {
                return Err(relation_error());
            }
        }
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
        let target_valid = match item.target_present {
            0 => item.target.ptr.is_null() && item.target.len == 0,
            1 => item.target.len != 0 && validate_utf8_view(item.target).is_ok(),
            _ => false,
        };
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
            || !target_valid
            || item.target_reserved_bytes != [0; 7]
            || !valid_required_key(item.provenance, slices.provenances.len())
            || item.reserved != 0
        {
            return Err(relation_error());
        }
        item_owners[owner_index.expect("validated item owner")] = true;
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
    let mut term_roots = Vec::new();
    term_roots
        .try_reserve_exact(slices.content_roots.len())
        .map_err(alloc_error)?;
    term_roots.resize(slices.content_roots.len(), false);
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
        if term_roots[form_root] {
            return Err(relation_error());
        }
        term_roots[form_root] = true;
    }
    if slices
        .content_roots
        .iter()
        .enumerate()
        .any(|(index, root)| (root.kind == ROOT_TERM) != term_roots[index])
    {
        return Err(relation_error());
    }
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

#[allow(clippy::too_many_lines)] // Mirrors every frozen result table and transfer counter.
pub(super) fn transfer_preflight(
    view: &ResultView,
    slices: &StructuredSlices<'_>,
    limits: &Limits,
) -> Result<(), NativeStructuredError> {
    // The top-level result view and metadata are records in addition to every
    // typed table row validated below.
    let mut objects = 2_u64;
    let mut edges = 1_u64; // rootSource -> sources
    let mut bytes = 0_u64;
    for count in [
        slices.sources.len(),
        slices.spans.len(),
        slices.provenances.len(),
        slices.owners.len(),
        slices.content_roots.len(),
        slices.content_atoms.len(),
        slices.content_refs.len(),
        slices.content_points.len(),
        slices.links.len(),
        slices.blocks.len(),
        slices.lists.len(),
        slices.items.len(),
        slices.tables.len(),
        slices.table_rows.len(),
        slices.table_cells.len(),
        slices.fixed_views.len(),
        slices.fixed_lines.len(),
        slices.placements.len(),
        slices.decorations.len(),
        slices.forms.len(),
        slices.name_hints.len(),
        slices.relations.len(),
        slices.diagnostics.len(),
    ] {
        objects = objects
            .checked_add(u64::try_from(count).map_err(|_| relation_error())?)
            .ok_or_else(relation_error)?;
    }
    add_edges(&mut edges, slices.spans.len())?;
    for provenance in slices.provenances {
        if provenance.kind == PROVENANCE_AUTHORED
            || (provenance.kind == PROVENANCE_GENERATED && provenance.generated_trigger_span != 0)
        {
            add_edges(&mut edges, 1)?;
        }
    }
    add_edges(&mut edges, slices.owners.len())?;
    add_edges(&mut edges, slices.content_roots.len())?; // owner
    add_edges(&mut edges, slices.content_roots.len())?; // provenance
    for _ in 0..3 {
        add_edges(&mut edges, slices.content_atoms.len())?; // root, owner, provenance
    }
    add_edges(
        &mut edges,
        slices
            .content_atoms
            .iter()
            .filter(|atom| atom.link != 0)
            .count(),
    )?;
    add_edges(&mut edges, slices.content_refs.len())?; // atom
    add_edges(&mut edges, slices.links.len())?; // owner
    add_edges(&mut edges, slices.links.len())?; // provenance
    for link in slices.links {
        add_edges(&mut edges, link.label_ref_count as usize)?;
    }
    add_edges(&mut edges, slices.blocks.len())?; // owner
    add_edges(&mut edges, slices.blocks.len())?; // provenance
    for count in [
        slices
            .blocks
            .iter()
            .filter(|block| block.parent != 0)
            .count(),
        slices.blocks.iter().filter(|block| block.root != 0).count(),
        slices
            .blocks
            .iter()
            .filter(|block| block.table != 0)
            .count(),
        slices
            .blocks
            .iter()
            .filter(|block| block.fixed_view != 0)
            .count(),
    ] {
        add_edges(&mut edges, count)?;
    }
    add_edges(&mut edges, slices.lists.len())?; // block
    add_edges(&mut edges, slices.lists.len())?; // provenance
    for item in slices.items {
        add_edges(&mut edges, 3)?; // list, owner, provenance
        add_edges(&mut edges, item.form_count as usize)?;
    }
    for form in slices.forms {
        add_edges(&mut edges, 2)?; // owner, provenance
        add_edges(&mut edges, form.ref_count as usize)?;
    }
    for hint in slices.name_hints {
        add_edges(&mut edges, 2)?; // form, provenance
        add_edges(&mut edges, hint.ref_count as usize)?;
    }
    for diagnostic in slices.diagnostics {
        add_edges(
            &mut edges,
            usize::from(diagnostic.span != 0) + usize::from(diagnostic.owner != 0),
        )?;
    }
    add_transfer_table_bytes::<OwnedSource, crate::structured::SourceRecord, _>(
        &mut bytes,
        slices.sources,
    )?;
    add_transfer_table_bytes::<OwnedSpan, crate::structured::SourceSpan, _>(
        &mut bytes,
        slices.spans,
    )?;
    add_transfer_table_bytes::<OwnedProvenance, crate::structured::Provenance, _>(
        &mut bytes,
        slices.provenances,
    )?;
    add_transfer_table_bytes::<OwnedOwner, crate::structured::ContentOwner, _>(
        &mut bytes,
        slices.owners,
    )?;
    add_transfer_table_bytes::<OwnedContentRoot, crate::structured::ContentRoot, _>(
        &mut bytes,
        slices.content_roots,
    )?;
    add_transfer_table_bytes::<OwnedContentAtom, crate::structured::ContentAtom, _>(
        &mut bytes,
        slices.content_atoms,
    )?;
    add_transfer_table_bytes::<OwnedContentRef, crate::structured::ContentRef, _>(
        &mut bytes,
        slices.content_refs,
    )?;
    add_transfer_table_bytes::<OwnedLink, crate::structured::LinkOccurrence, _>(
        &mut bytes,
        slices.links,
    )?;
    add_transfer_table_bytes::<OwnedBlock, crate::structured::NativeBlock, _>(
        &mut bytes,
        slices.blocks,
    )?;
    add_transfer_table_bytes::<super::OwnedList, crate::structured::NativeList, _>(
        &mut bytes,
        slices.lists,
    )?;
    add_transfer_table_bytes::<super::OwnedItem, crate::structured::NativeItem, _>(
        &mut bytes,
        slices.items,
    )?;
    add_transfer_table_bytes::<super::OwnedForm, crate::structured::NativeForm, _>(
        &mut bytes,
        slices.forms,
    )?;
    add_transfer_table_bytes::<super::OwnedNameHint, crate::structured::NativeNameHint, _>(
        &mut bytes,
        slices.name_hints,
    )?;
    add_transfer_table_bytes::<OwnedDiagnostic, crate::structured::NativeDiagnostic, _>(
        &mut bytes,
        slices.diagnostics,
    )?;
    for source in slices.sources {
        bytes = bytes
            .checked_add(validate_utf8_view(source.logical_name)?)
            .ok_or_else(relation_error)?;
    }
    for field in [
        view.metadata.title,
        view.metadata.section,
        view.metadata.volume,
        view.metadata.operating_system,
        view.metadata.architecture,
        view.metadata.name,
        view.metadata.date,
        view.metadata.alias_target,
    ] {
        bytes = bytes
            .checked_add(validate_utf8_view(field)?)
            .ok_or_else(relation_error)?;
    }
    for atom in slices.content_atoms {
        let display_bytes = match atom.display_override_present {
            0 if atom.display_override.ptr.is_null() && atom.display_override.len == 0 => 0,
            1 => validate_utf8_view(atom.display_override)?,
            _ => return Err(relation_error()),
        };
        bytes = bytes
            .checked_add(validate_utf8_view(atom.text)?)
            .and_then(|value| value.checked_add(display_bytes))
            .ok_or_else(relation_error)?;
    }
    for link in slices.links {
        bytes = bytes
            .checked_add(validate_utf8_view(link.target_a)?)
            .and_then(|value| {
                value.checked_add(if link.target_b_present == 1 {
                    validate_utf8_view(link.target_b).ok()?
                } else {
                    0
                })
            })
            .and_then(|value| {
                value.checked_add(if link.title_present == 1 {
                    validate_utf8_view(link.title).ok()?
                } else {
                    0
                })
            })
            .ok_or_else(relation_error)?;
    }
    for item in slices.items {
        if item.target_present == 1 {
            bytes = bytes
                .checked_add(validate_utf8_view(item.target)?)
                .ok_or_else(relation_error)?;
        }
    }
    for diagnostic in slices.diagnostics {
        bytes = bytes
            .checked_add(validate_utf8_view(diagnostic.message)?)
            .ok_or_else(relation_error)?;
    }
    if objects > limits.max_transfer_objects
        || edges > limits.max_transfer_edges
        || bytes > limits.max_transfer_bytes
    {
        return Err(NativeStructuredError {
            status: STATUS_BUDGET,
            stage: 6,
            limit_kind: if objects > limits.max_transfer_objects {
                32
            } else if edges > limits.max_transfer_edges {
                33
            } else {
                34
            },
            observed: if objects > limits.max_transfer_objects {
                objects
            } else if edges > limits.max_transfer_edges {
                edges
            } else {
                bytes
            },
            allowed: if objects > limits.max_transfer_objects {
                limits.max_transfer_objects
            } else if edges > limits.max_transfer_edges {
                limits.max_transfer_edges
            } else {
                limits.max_transfer_bytes
            },
        });
    }
    Ok(())
}

pub(super) fn add_edges(edges: &mut u64, count: usize) -> Result<(), NativeStructuredError> {
    *edges = edges
        .checked_add(u64::try_from(count).map_err(|_| relation_error())?)
        .ok_or_else(relation_error)?;
    Ok(())
}

pub(super) fn add_transfer_table_bytes<Raw, Typed, View>(
    bytes: &mut u64,
    rows: &[View],
) -> Result<(), NativeStructuredError> {
    let row_bytes = std::mem::size_of::<Raw>()
        .checked_add(std::mem::size_of::<Typed>())
        .ok_or_else(relation_error)?;
    let allocated = rows
        .len()
        .checked_mul(row_bytes)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(relation_error)?;
    *bytes = bytes.checked_add(allocated).ok_or_else(relation_error)?;
    Ok(())
}

pub(super) fn validate_utf8_view(view: BytesView) -> Result<u64, NativeStructuredError> {
    if view.len == 0 {
        return if view.ptr.is_null() {
            Ok(0)
        } else {
            Err(relation_error())
        };
    }
    let length = usize::try_from(view.len).map_err(|_| relation_error())?;
    if view.ptr.is_null()
        || length > isize::MAX as usize
        || (view.ptr as usize).checked_add(length - 1).is_none()
    {
        return Err(relation_error());
    }
    let bytes = unsafe { std::slice::from_raw_parts(view.ptr, length) };
    std::str::from_utf8(bytes).map_err(|_| relation_error())?;
    Ok(view.len)
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
