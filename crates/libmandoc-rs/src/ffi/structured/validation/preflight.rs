//! Owned-transfer object, edge, and byte budget preflight.

use super::super::{
    BytesView, Limits, NativeStructuredError, OwnedAnchor, OwnedBlock, OwnedContentAtom,
    OwnedContentPoint, OwnedContentRef, OwnedContentRoot, OwnedDiagnostic, OwnedForm,
    OwnedHeadingEvidence, OwnedItem, OwnedLink, OwnedLinkLabelPart, OwnedList, OwnedNameHint,
    OwnedOwner, OwnedProvenance, OwnedSource, OwnedSpan, OwnedTable, OwnedTableCell, OwnedTableRow,
    PROVENANCE_AUTHORED, PROVENANCE_GENERATED, ResultView, STATUS_BUDGET, StructuredSlices,
    relation_error,
};

#[allow(clippy::too_many_lines)] // Mirrors every frozen result table and transfer counter.
pub(in super::super) fn transfer_preflight(
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
        slices.anchors.len(),
        slices.heading_evidence.len(),
        slices.link_label_parts.len(),
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
    for point in slices.content_points {
        add_edges(&mut edges, 3 + usize::from(point.atom != 0))?;
    }
    add_edges(&mut edges, slices.links.len())?; // owner
    add_edges(&mut edges, slices.links.len())?; // provenance
    for link in slices.links {
        add_edges(&mut edges, link.label_part_count as usize)?;
    }
    add_edges(&mut edges, slices.link_label_parts.len())?; // atom
    for _ in slices.anchors {
        add_edges(&mut edges, 3)?; // owner, point, provenance
    }
    for _ in slices.heading_evidence {
        add_edges(&mut edges, 3)?; // block, owner, provenance
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
    for table in slices.tables {
        add_edges(&mut edges, 2 + usize::from(table.fixed_view != 0))?;
    }
    for row in slices.table_rows {
        add_edges(&mut edges, 2 + usize::from(row.point != 0))?;
    }
    for cell in slices.table_cells {
        add_edges(&mut edges, 3 + usize::from(cell.point != 0))?;
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
    add_transfer_table_bytes::<OwnedContentPoint, crate::structured::ContentPoint, _>(
        &mut bytes,
        slices.content_points,
    )?;
    add_transfer_table_bytes::<OwnedLinkLabelPart, crate::structured::LinkLabelPart, _>(
        &mut bytes,
        slices.link_label_parts,
    )?;
    add_transfer_table_bytes::<OwnedLink, crate::structured::LinkOccurrence, _>(
        &mut bytes,
        slices.links,
    )?;
    add_transfer_table_bytes::<OwnedAnchor, crate::structured::AnchorEvidence, _>(
        &mut bytes,
        slices.anchors,
    )?;
    add_transfer_table_bytes::<OwnedHeadingEvidence, crate::structured::HeadingEvidence, _>(
        &mut bytes,
        slices.heading_evidence,
    )?;
    add_transfer_table_bytes::<OwnedBlock, crate::structured::NativeBlock, _>(
        &mut bytes,
        slices.blocks,
    )?;
    add_transfer_table_bytes::<OwnedList, crate::structured::NativeList, _>(
        &mut bytes,
        slices.lists,
    )?;
    add_transfer_table_bytes::<OwnedItem, crate::structured::NativeItem, _>(
        &mut bytes,
        slices.items,
    )?;
    add_transfer_table_bytes::<OwnedTable, crate::structured::NativeTable, _>(
        &mut bytes,
        slices.tables,
    )?;
    add_transfer_table_bytes::<OwnedTableRow, crate::structured::NativeTableRow, _>(
        &mut bytes,
        slices.table_rows,
    )?;
    add_transfer_table_bytes::<OwnedTableCell, crate::structured::NativeTableCell, _>(
        &mut bytes,
        slices.table_cells,
    )?;
    add_transfer_table_bytes::<OwnedForm, crate::structured::NativeForm, _>(
        &mut bytes,
        slices.forms,
    )?;
    add_transfer_table_bytes::<OwnedNameHint, crate::structured::NativeNameHint, _>(
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
    for anchor in slices.anchors {
        bytes = bytes
            .checked_add(validate_utf8_view(anchor.target)?)
            .ok_or_else(relation_error)?;
    }
    for heading in slices.heading_evidence {
        if heading.authored_phrase_present == 1 {
            bytes = bytes
                .checked_add(validate_utf8_view(heading.authored_phrase)?)
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

fn add_edges(edges: &mut u64, count: usize) -> Result<(), NativeStructuredError> {
    *edges = edges
        .checked_add(u64::try_from(count).map_err(|_| relation_error())?)
        .ok_or_else(relation_error)?;
    Ok(())
}

fn add_transfer_table_bytes<Raw, Typed, View>(
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
