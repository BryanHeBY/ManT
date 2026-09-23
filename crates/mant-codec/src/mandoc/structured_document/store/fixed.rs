//! Physical native geometry, transferred without copying logical text.

use libmandoc_rs::structured::{
    NativeCellMapKind, NativeDecorationKind, NativePlacementTarget, StructuredFixedTables,
};
use mant_ir::{
    CellMapKind, ContentAtomKey, ContentByteRange, ContentOwnerKey, ContentPointKey, ContentRef,
    ContentStore, Decoration, DecorationKey, DecorationKind, FixedLine, FixedLineKey, FixedView,
    FixedViewKey, Placement, PlacementKey, PlacementTarget, Provenance,
};

use super::{NativeProjectionError, mapped, provenance};

#[allow(clippy::too_many_lines)] // One ordered transfer keeps dense key and containment checks together.
pub(super) fn transfer_fixed(
    store: &mut ContentStore,
    tables: StructuredFixedTables,
    owners: &[ContentOwnerKey],
    atoms: &[ContentAtomKey],
    points: &[ContentPointKey],
    provenances: &[Provenance],
) -> Result<Vec<FixedViewKey>, NativeProjectionError> {
    let (views, lines, placements, decorations) = tables.into_parts();
    let mut view_keys = Vec::new();
    view_keys
        .try_reserve_exact(views.len())
        .map_err(|_| NativeProjectionError::InvalidRelation("fixed view key allocation"))?;
    store
        .fixed_views
        .try_reserve_exact(views.len())
        .map_err(|_| NativeProjectionError::InvalidRelation("fixed view allocation"))?;
    let (mut line_index, mut placement_index, mut decoration_index) = (0_usize, 0_usize, 0_usize);
    for view in views {
        let key = FixedViewKey::new(view.key().get()).ok_or(
            NativeProjectionError::InvalidRelation("fixed view key is absent"),
        )?;
        view_keys.push(key);
        let mut physical_lines = Vec::new();
        while line_index < lines.len() && lines[line_index].view() == view.key() {
            let line = &lines[line_index];
            let line_key = FixedLineKey::new(line.key().get()).ok_or(
                NativeProjectionError::InvalidRelation("fixed line key is absent"),
            )?;
            let mut line_placements = Vec::new();
            while placement_index < placements.len()
                && placements[placement_index].line() == line.key()
            {
                let placement = &placements[placement_index];
                let target = match placement.target() {
                    NativePlacementTarget::Content {
                        atom,
                        byte_start,
                        byte_end,
                    } => PlacementTarget::Content(ContentRef {
                        atom: mapped(atoms, atom.get(), "fixed placement atom is unknown")?,
                        bytes: ContentByteRange {
                            start: byte_start,
                            end: byte_end,
                        },
                    }),
                    NativePlacementTarget::Point(point) => PlacementTarget::Point(mapped(
                        points,
                        point.get(),
                        "fixed placement point is unknown",
                    )?),
                };
                let map = match placement.map() {
                    NativeCellMapKind::Affine { columns_per_scalar } => {
                        CellMapKind::Affine { columns_per_scalar }
                    }
                    NativeCellMapKind::GraphemeCluster => CellMapKind::GraphemeCluster {},
                    NativeCellMapKind::Overlay => CellMapKind::Overlay {},
                };
                line_placements.push(Placement {
                    key: PlacementKey::new(u32::try_from(placement_index + 1).map_err(|_| {
                        NativeProjectionError::InvalidRelation("fixed placement key overflow")
                    })?)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "fixed placement key is absent",
                    ))?,
                    target,
                    root_scalar_range: placement.scalar_range(),
                    start_column: placement.column_range().start,
                    end_column: placement.column_range().end,
                    map,
                });
                placement_index += 1;
            }
            let mut line_decorations = Vec::new();
            while decoration_index < decorations.len()
                && decorations[decoration_index].line() == line.key()
            {
                let decoration = &decorations[decoration_index];
                line_decorations.push(Decoration {
                    key: DecorationKey::new(u32::try_from(decoration_index + 1).map_err(|_| {
                        NativeProjectionError::InvalidRelation("fixed decoration key overflow")
                    })?)
                    .ok_or(NativeProjectionError::InvalidRelation(
                        "fixed decoration key is absent",
                    ))?,
                    text: decoration.text().to_owned(),
                    start_column: decoration.column_range().start,
                    width_columns: decoration.column_range().end - decoration.column_range().start,
                    kind: match decoration.kind() {
                        NativeDecorationKind::Border => DecorationKind::Border,
                        NativeDecorationKind::Rule => DecorationKind::Rule,
                        NativeDecorationKind::Padding => DecorationKind::Padding,
                    },
                    provenance: provenance(provenances, decoration.provenance())?,
                });
                decoration_index += 1;
            }
            physical_lines.push(FixedLine {
                key: line_key,
                terminal_columns: line.terminal_columns(),
                placements: line_placements,
                decorations: line_decorations,
            });
            line_index += 1;
        }
        store.fixed_views.push(FixedView {
            key,
            owner: mapped(owners, view.owner().get(), "fixed view owner is unknown")?,
            lines: physical_lines,
            provenance: provenance(provenances, view.provenance())?,
        });
    }
    if line_index != lines.len()
        || placement_index != placements.len()
        || decoration_index != decorations.len()
    {
        return Err(NativeProjectionError::InvalidRelation(
            "fixed records have no containing view or line",
        ));
    }
    Ok(view_keys)
}
