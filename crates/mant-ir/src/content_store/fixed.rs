//! Checked physical geometry over the single authoritative logical body.

use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

use crate::Provenance;

use super::{
    ContentAtomKind, ContentOwnerKey, ContentPointKey, ContentRef, DecorationKey, FixedLineKey,
    FixedViewKey, PlacementKey,
};
use crate::ContentContext;

/// One document-local native display projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedView {
    /// Dense key equal to this record's position in the content store.
    pub key: FixedViewKey,
    /// Logical owner shared with the fixed block or table.
    pub owner: ContentOwnerKey,
    /// Native physical lines, without viewport clipping or reflow.
    pub lines: Vec<FixedLine>,
    /// Authorship of the display boundary.
    pub provenance: Provenance,
}

impl FixedView {
    /// Materialize complete native physical rows for a text-only display
    /// boundary. Semantic reads must continue to use the logical content.
    #[must_use]
    pub fn physical_lines(&self, content: ContentContext<'_>) -> Option<Vec<String>> {
        let mut lines = Vec::with_capacity(self.lines.len());
        let mut total_width = 0_usize;
        for line in &self.lines {
            let width = usize::try_from(line.terminal_columns).ok()?;
            total_width = total_width.checked_add(width)?;
            if width > 1_048_576 || total_width > 32 * 1024 * 1024 {
                return None;
            }
            // A column stores only a glyph index or a continuation marker.
            // In particular, native padding must not allocate one String per
            // terminal cell on large fixed tables.
            let mut cells = vec![0_u32; width];
            let mut glyphs = Vec::new();
            let mut zero_width = Vec::new();
            for placement in &line.placements {
                let PlacementTarget::Content(reference) = placement.target else {
                    continue;
                };
                let atom = content.atom(reference.atom)?;
                let text = content.resolve_text(reference)?;
                let display = placement_display(&atom.kind, reference, text);
                let start = usize::try_from(placement.start_column).ok()?;
                let end = usize::try_from(placement.end_column).ok()?;
                if start == end {
                    if !matches!(placement.map, CellMapKind::GraphemeCluster {})
                        || UnicodeWidthStr::width(display) != 0
                    {
                        return None;
                    }
                    zero_width.push((start, display));
                    continue;
                }
                match placement.map {
                    CellMapKind::Affine { columns_per_scalar } => {
                        let step = usize::from(columns_per_scalar);
                        if display == text {
                            for (index, scalar) in display.chars().enumerate() {
                                put_glyph(
                                    &mut cells,
                                    &mut glyphs,
                                    start.checked_add(index.checked_mul(step)?)?,
                                    step,
                                    CellGlyph::Scalar(scalar),
                                )?;
                            }
                        } else {
                            put_glyph(
                                &mut cells,
                                &mut glyphs,
                                start,
                                step,
                                CellGlyph::Text(display),
                            )?;
                        }
                    }
                    CellMapKind::GraphemeCluster {} | CellMapKind::Overlay {} => {
                        put_glyph(
                            &mut cells,
                            &mut glyphs,
                            start,
                            end.checked_sub(start)?,
                            CellGlyph::Text(display),
                        )?;
                    }
                }
            }
            for decoration in &line.decorations {
                put_glyph(
                    &mut cells,
                    &mut glyphs,
                    usize::try_from(decoration.start_column).ok()?,
                    usize::try_from(decoration.width_columns).ok()?,
                    CellGlyph::Text(&decoration.text),
                )?;
            }
            // The terminal emits zero-column scalars in byte order at their
            // current column.  Keep them outside the cell array: assigning a
            // synthetic column would shift every later glyph and hit region.
            zero_width.sort_by_key(|(column, _)| *column);
            let mut zero_width = zero_width.into_iter().peekable();
            let mut output = String::with_capacity(width);
            for (column, cell) in cells.into_iter().enumerate() {
                while zero_width.peek().is_some_and(|(start, _)| *start == column) {
                    output.push_str(zero_width.next()?.1);
                }
                match cell {
                    0 => output.push(' '),
                    u32::MAX => {}
                    index => match glyphs.get((index - 1) as usize)? {
                        CellGlyph::Text(value) => output.push_str(value),
                        CellGlyph::Scalar(value) => output.push(*value),
                    },
                }
            }
            for (column, text) in zero_width {
                if column != width {
                    return None;
                }
                output.push_str(text);
            }
            lines.push(output);
        }
        Some(lines)
    }
}

pub(super) fn placement_display<'a>(
    kind: &'a ContentAtomKind,
    reference: ContentRef,
    text: &'a str,
) -> &'a str {
    match kind {
        ContentAtomKind::Text {
            text: whole,
            display_override: Some(display),
        }
        | ContentAtomKind::Whitespace {
            text: whole,
            display_override: Some(display),
            ..
        } if reference.bytes.start == 0 && reference.bytes.end as usize == whole.len() => display,
        _ => text,
    }
}

enum CellGlyph<'a> {
    Text(&'a str),
    Scalar(char),
}

fn put_glyph<'a>(
    cells: &mut [u32],
    glyphs: &mut Vec<CellGlyph<'a>>,
    start: usize,
    width: usize,
    glyph: CellGlyph<'a>,
) -> Option<()> {
    let end = start.checked_add(width)?;
    let region = cells.get_mut(start..end)?;
    let (first, rest) = region.split_first_mut()?;
    let index = u32::try_from(glyphs.len().checked_add(1)?).ok()?;
    if index == u32::MAX {
        return None;
    }
    glyphs.push(glyph);
    *first = index;
    for cell in rest {
        *cell = u32::MAX;
    }
    Some(())
}

/// One native physical line. Its content is referenced, not copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedLine {
    /// Dense document-local line key.
    pub key: FixedLineKey,
    /// Width in terminal columns, independent of the current viewport.
    pub terminal_columns: u32,
    /// Ordered placements of semantic content and zero-width points.
    pub placements: Vec<Placement>,
    /// Native border, rule, and padding glyphs without semantic content.
    pub decorations: Vec<Decoration>,
}

/// One visible use of a logical content slice or point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Placement {
    /// Dense document-local placement key.
    pub key: PlacementKey,
    /// The one authoritative logical object shown here.
    pub target: PlacementTarget,
    /// Root-relative Unicode scalar range of the selected target.
    pub root_scalar_range: Range<u32>,
    /// Inclusive first terminal column.
    pub start_column: u32,
    /// Exclusive last terminal column.
    pub end_column: u32,
    /// Mapping from complete logical graphemes to terminal columns.
    pub map: CellMapKind,
}

/// A placement never carries both text and a zero-width target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum PlacementTarget {
    /// A checked UTF-8 slice of one logical atom.
    Content(ContentRef),
    /// A checked zero-width point in one logical root.
    Point(ContentPointKey),
}

/// Closed terminal-cell mapping family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CellMapKind {
    /// Equal-width scalar cells, such as printable ASCII.
    Affine {
        /// Positive terminal-cell width of each scalar.
        columns_per_scalar: u8,
    },
    /// One indivisible extended grapheme, including wide/combining glyphs.
    GraphemeCluster {},
    /// Explicitly observed overstrike at the same terminal columns.
    Overlay {},
}

/// Non-semantic native drawing glyphs kept outside the logical body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decoration {
    /// Dense document-local decoration key.
    pub key: DecorationKey,
    /// Profile-specific visible glyphs, never searched as logical content.
    pub text: String,
    /// Inclusive first terminal column.
    pub start_column: u32,
    /// Width in terminal columns.
    pub width_columns: u32,
    /// Native-generated drawing role.
    pub kind: DecorationKind,
    /// Authorship, generated or unknown for formatter-created glyphs.
    pub provenance: Provenance,
}

/// Closed set of non-semantic decoration roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DecorationKind {
    /// A table frame or cell separator.
    Border,
    /// A rule line or rule cell.
    Rule,
    /// Native field padding.
    Padding,
}

#[cfg(test)]
mod tests {
    use crate::{
        ContentOwnerKind, ContentRootKind, ContentStore, ContentStoreBuilder, ContentStyle,
        PointBoundary, validate_content_store,
    };

    use super::*;

    fn fixture() -> ContentStore {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::FixedBody, Provenance::Unknown);
        let content = builder.push_text(
            root,
            "a界".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let point = builder.push_point(
            root,
            PointBoundary::BetweenAtoms { atom_boundary: 1 },
            2,
            Provenance::Unknown,
        );
        let mut store = builder.finish();
        store.fixed_views.push(FixedView {
            key: FixedViewKey::FIRST,
            owner,
            lines: vec![FixedLine {
                key: FixedLineKey::FIRST,
                terminal_columns: 5,
                placements: vec![
                    Placement {
                        key: PlacementKey::FIRST,
                        target: PlacementTarget::Content(ContentRef {
                            atom: content.atom,
                            bytes: super::super::ContentByteRange { start: 0, end: 1 },
                        }),
                        root_scalar_range: 0..1,
                        start_column: 0,
                        end_column: 1,
                        map: CellMapKind::Affine {
                            columns_per_scalar: 1,
                        },
                    },
                    Placement {
                        key: PlacementKey::new(2).unwrap(),
                        target: PlacementTarget::Content(ContentRef {
                            atom: content.atom,
                            bytes: super::super::ContentByteRange { start: 1, end: 4 },
                        }),
                        root_scalar_range: 1..2,
                        start_column: 1,
                        end_column: 3,
                        map: CellMapKind::GraphemeCluster {},
                    },
                    Placement {
                        key: PlacementKey::new(3).unwrap(),
                        target: PlacementTarget::Point(point),
                        root_scalar_range: 2..2,
                        start_column: 3,
                        end_column: 3,
                        map: CellMapKind::GraphemeCluster {},
                    },
                ],
                decorations: vec![Decoration {
                    key: DecorationKey::FIRST,
                    text: "│".to_owned(),
                    start_column: 4,
                    width_columns: 1,
                    kind: DecorationKind::Border,
                    provenance: Provenance::Unknown,
                }],
            }],
            provenance: Provenance::Unknown,
        });
        store
    }

    #[test]
    fn native_fixed_view_reuses_logical_atoms_and_accepts_zero_width_points() {
        let store = fixture();
        validate_content_store(&store).unwrap();
        assert_eq!(store.atoms.len(), 1);
        assert_eq!(store.fixed_views[0].lines[0].placements.len(), 3);
        assert_eq!(
            store.fixed_views[0]
                .physical_lines(store.content())
                .unwrap(),
            ["a界 │"]
        );
    }

    #[test]
    fn emitted_combining_scalar_uses_zero_columns_without_losing_byte_order() {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Content, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::FixedBody, Provenance::Unknown);
        let atom = builder.push_text(
            root,
            "A\u{301}B".to_owned(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        let mut store = builder.finish();
        let placement = |key: u32,
                         bytes: (u32, u32),
                         scalars: (u32, u32),
                         columns: (u32, u32),
                         map: CellMapKind| Placement {
            key: PlacementKey::new(key).unwrap(),
            target: PlacementTarget::Content(ContentRef {
                atom: atom.atom,
                bytes: super::super::ContentByteRange {
                    start: bytes.0,
                    end: bytes.1,
                },
            }),
            root_scalar_range: scalars.0..scalars.1,
            start_column: columns.0,
            end_column: columns.1,
            map,
        };
        store.fixed_views.push(FixedView {
            key: FixedViewKey::FIRST,
            owner,
            lines: vec![FixedLine {
                key: FixedLineKey::FIRST,
                terminal_columns: 2,
                placements: vec![
                    placement(
                        1,
                        (0, 1),
                        (0, 1),
                        (0, 1),
                        CellMapKind::Affine {
                            columns_per_scalar: 1,
                        },
                    ),
                    placement(2, (1, 3), (1, 2), (1, 1), CellMapKind::GraphemeCluster {}),
                    placement(
                        3,
                        (3, 4),
                        (2, 3),
                        (1, 2),
                        CellMapKind::Affine {
                            columns_per_scalar: 1,
                        },
                    ),
                ],
                decorations: Vec::new(),
            }],
            provenance: Provenance::Unknown,
        });
        validate_content_store(&store).unwrap();
        assert_eq!(
            store.fixed_views[0]
                .physical_lines(store.content())
                .unwrap(),
            ["A\u{301}B"]
        );
        store.fixed_views[0].lines[0].placements[1].map = CellMapKind::Overlay {};
        assert!(validate_content_store(&store).is_err());
    }

    #[test]
    fn fixed_mapping_rejects_wrong_scalars_overlaps_and_invalid_glyph_widths() {
        let mut store = fixture();
        store.fixed_views[0].lines[0].placements[1].root_scalar_range = 0..1;
        assert!(validate_content_store(&store).is_err());

        let mut store = fixture();
        store.fixed_views[0].lines[0].placements[1].start_column = 0;
        assert!(validate_content_store(&store).is_err());

        let mut store = fixture();
        store.fixed_views[0].lines[0].decorations[0].width_columns = 2;
        assert!(validate_content_store(&store).is_err());

        let mut store = fixture();
        store.fixed_views[0].lines[0].decorations[0].start_column = 2;
        assert!(validate_content_store(&store).is_err());

        let mut store = fixture();
        store.fixed_views[0].lines[0].placements[1].map = CellMapKind::Overlay {};
        store.fixed_views[0].lines[0].placements[1].start_column = 0;
        store.fixed_views[0].lines[0].placements[1].end_column = 2;
        assert!(validate_content_store(&store).is_err());
    }

    #[test]
    fn fixed_projection_keeps_one_logical_atom_and_remaps_points() {
        let store = fixture();
        let mut builder = crate::ContentProjectionBuilder::new(&store);
        let mut blocks = vec![crate::Block::FixedDisplay {
            children: Vec::new(),
            view: FixedViewKey::FIRST,
            layout: crate::LayoutHint::default(),
            source: None,
        }];
        builder.include_blocks(&blocks).unwrap();
        let (projection, remap) = builder.finish().unwrap();
        remap.remap_blocks(&mut blocks).unwrap();
        assert_eq!(projection.content_store.atoms.len(), 1);
        assert_eq!(projection.content_store.points.len(), 1);
        assert_eq!(projection.content_store.fixed_views.len(), 1);
        assert_eq!(
            projection.content_store.fixed_views[0].lines[0]
                .placements
                .len(),
            3
        );
        validate_content_store(&projection.content_store).unwrap();
    }

    #[test]
    fn fixed_wire_rejects_unknown_fields_and_missing_relations() {
        let store = fixture();
        let mut wire = serde_json::to_value(&store).unwrap();
        wire["fixedViews"][0]["unrecognized"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ContentStore>(wire).is_err());

        let mut wire = serde_json::to_value(&store).unwrap();
        wire["fixedViews"][0]["lines"][0]["placements"][0]
            .as_object_mut()
            .unwrap()
            .remove("target");
        assert!(serde_json::from_value::<ContentStore>(wire).is_err());
    }
}
