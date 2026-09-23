//! Conversion from the private owned transfer model to public typed values.

use super::{
    ATOM_BREAK_OPPORTUNITY, ATOM_HARD_BREAK, ATOM_TEXT, ATOM_WHITESPACE, BLOCK_DEFINITION_LIST,
    BLOCK_FIXED_DISPLAY, BLOCK_HEADING, BLOCK_INDENTED, BLOCK_LIST, BLOCK_PARAGRAPH, BLOCK_TABLE,
    BLOCK_THEMATIC_BREAK, BLOCK_VERTICAL_SPACE, COORD_NATIVE_NORMALIZED_BYTES, FORMAT_MAN,
    FORMAT_MDOC, IDENTITY_BUNDLE_MEMBER, LINK_LABEL_CONTENT, LINK_LABEL_HARD_BREAK, LIST_BULLET,
    LIST_DEFINITION, LIST_NATIVE_MARKER, LIST_ORDERED, LIST_PLAIN, Limits, NativeStructuredError,
    OwnedMetadata, OwnedProvenance, OwnedStructuredDocument, PROFILE_ASCII, PROFILE_UTF8,
    STATUS_BUDGET, STATUS_BUILDER_ALLOC, STATUS_INVALID_INPUT, STATUS_NATIVE, STATUS_REENTRANT,
    STATUS_RELATION, STATUS_UNSUPPORTED, TARGET_ORIGIN_AUTHORED, TARGET_ORIGIN_GENERATED,
};

#[allow(clippy::too_many_lines)]
pub(super) fn semantic_document(
    raw: OwnedStructuredDocument,
) -> Result<crate::structured::StructuredDocument, crate::structured::StructuredError> {
    use crate::structured::{
        AnchorEvidence, AnchorEvidenceKey, ContentAtom, ContentAtomKey, ContentAtomKind,
        ContentOwner, ContentOwnerKind, ContentPoint, ContentPointKey, ContentRef, ContentRoot,
        ContentRootKey, ContentRootKind, HeadingEvidence, HeadingEvidenceKey, LineColumn,
        LineColumns, LinkLabelPart, LinkOccurrence, LinkOccurrenceKey, NativeBlock, NativeBlockKey,
        NativeBlockKind, NativeCellMapKind, NativeDecoration, NativeDecorationKind,
        NativeDiagnostic, NativeFixedLine, NativeFixedLineKey, NativeFixedView, NativeFixedViewKey,
        NativeForm, NativeFormKey, NativeItem, NativeItemKey, NativeLinkTarget, NativeList,
        NativeListKey, NativeListKind, NativeNameHint, NativeNameHintKey, NativePlacement,
        NativePlacementTarget, NativeRole, NativeTable, NativeTableAlignment, NativeTableCell,
        NativeTableCellKey, NativeTableCellKind, NativeTableKey, NativeTableRow, NativeTableRowKey,
        NativeTableRowKind, NativeTargetOrigin, OwnerKey, PointBoundary, Provenance, ProvenanceKey,
        SourceCoordinates, SourceIdentity, SourceKey, SourceRecord, SourceSpan, SpanKey,
        StructuredDiagnosticCode, StructuredDiagnosticLevel, StructuredDocument,
        StructuredMetadata, StructuredProfile, StructuredStyle,
    };

    let OwnedStructuredDocument {
        root_source,
        profile,
        width,
        metadata,
        sources,
        spans,
        provenances,
        owners,
        content_roots,
        content_atoms,
        content_refs,
        content_points,
        links,
        link_label_parts,
        anchors,
        heading_evidence,
        blocks,
        lists,
        items,
        tables,
        table_rows,
        table_cells,
        fixed_views,
        fixed_lines,
        placements,
        decorations,
        forms,
        name_hints,
        diagnostics,
    } = raw;

    let root_source = SourceKey::new(root_source)
        .ok_or_else(|| semantic_invalid("native root source key is absent"))?;
    let profile = match profile {
        PROFILE_UTF8 => StructuredProfile::Utf8,
        PROFILE_ASCII => StructuredProfile::Ascii,
        _ => return Err(semantic_invalid("native profile discriminator is unknown")),
    };

    let OwnedMetadata {
        macroset,
        title,
        section,
        volume,
        operating_system,
        architecture,
        name,
        date,
        alias_target,
        has_body,
    } = metadata;
    let metadata = StructuredMetadata {
        macro_set: semantic_source_format(macroset)?,
        title,
        section,
        volume,
        operating_system,
        architecture,
        name,
        date,
        alias_target,
        has_body,
    };

    let mut typed_sources = Vec::new();
    typed_sources
        .try_reserve_exact(sources.len())
        .map_err(semantic_allocation)?;
    for source in sources {
        let identity = match source.identity_kind {
            1 => SourceIdentity::Path(source.logical_name),
            IDENTITY_BUNDLE_MEMBER => SourceIdentity::BundleMember(source.logical_name),
            3 => SourceIdentity::Anonymous(source.logical_name),
            _ => return Err(semantic_invalid("native source identity kind is unknown")),
        };
        typed_sources.push(SourceRecord {
            key: SourceKey::new(source.key)
                .ok_or_else(|| semantic_invalid("native source key is absent"))?,
            identity,
            format: semantic_source_format(source.format)?,
            decoded_byte_len: source.decoded_length,
            content_sha256: source.hash,
            coordinates: match source.coordinate_kind {
                1 => SourceCoordinates::DecodedUtf8Bytes,
                COORD_NATIVE_NORMALIZED_BYTES => SourceCoordinates::NativeNormalizedBytes,
                _ => {
                    return Err(semantic_invalid(
                        "native source coordinate discriminator is unknown",
                    ));
                }
            },
        });
    }

    let mut typed_spans = Vec::new();
    typed_spans
        .try_reserve_exact(spans.len())
        .map_err(semantic_allocation)?;
    for span in spans {
        let line_columns = match span.line_columns {
            None => None,
            Some((line, column, end_line, end_column)) => {
                let start = LineColumn::new(line, column)
                    .ok_or_else(|| semantic_invalid("native source span start is invalid"))?;
                let end = match (end_line, end_column) {
                    (0, 0) => None,
                    (0, _) | (_, 0) => {
                        return Err(semantic_invalid("native source span end is incomplete"));
                    }
                    (line, column) => Some(
                        LineColumn::new(line, column)
                            .ok_or_else(|| semantic_invalid("native source span end is invalid"))?,
                    ),
                };
                Some(LineColumns::new(start, end))
            }
        };
        typed_spans.push(SourceSpan {
            source: SourceKey::new(span.source)
                .ok_or_else(|| semantic_invalid("native span source key is absent"))?,
            line_columns,
            byte_range: span.byte_range,
        });
    }

    let mut typed_provenances = Vec::new();
    typed_provenances
        .try_reserve_exact(provenances.len())
        .map_err(semantic_allocation)?;
    for provenance in provenances {
        typed_provenances.push(match provenance {
            OwnedProvenance::Authored { span } => Provenance::Authored {
                span: SpanKey::new(span)
                    .ok_or_else(|| semantic_invalid("native authored span key is absent"))?,
            },
            OwnedProvenance::Generated { trigger_span } => Provenance::Generated {
                trigger: trigger_span
                    .map(|span| {
                        SpanKey::new(span).ok_or_else(|| {
                            semantic_invalid("native generated trigger span key is absent")
                        })
                    })
                    .transpose()?,
            },
            OwnedProvenance::Unknown => Provenance::Unknown,
        });
    }

    let mut typed_owners = Vec::new();
    typed_owners
        .try_reserve_exact(owners.len())
        .map_err(semantic_allocation)?;
    for owner in owners {
        let kind = match owner.kind {
            1 => ContentOwnerKind::Document,
            2 => ContentOwnerKind::Section,
            3 => ContentOwnerKind::Paragraph,
            4 => ContentOwnerKind::ListItem,
            5 => ContentOwnerKind::DefinitionItem,
            6 => ContentOwnerKind::TableCell,
            7 => ContentOwnerKind::FixedDisplay,
            _ => return Err(semantic_invalid("native content owner kind is unknown")),
        };
        typed_owners.push(ContentOwner {
            key: OwnerKey::new(owner.key)
                .ok_or_else(|| semantic_invalid("native owner key is absent"))?,
            kind,
            provenance: ProvenanceKey::new(owner.provenance)
                .ok_or_else(|| semantic_invalid("native owner provenance key is absent"))?,
        });
    }

    let mut typed_roots = Vec::new();
    typed_roots
        .try_reserve_exact(content_roots.len())
        .map_err(semantic_allocation)?;
    for root in content_roots {
        let kind = match root.kind {
            1 => ContentRootKind::Heading,
            2 => ContentRootKind::Term,
            3 => ContentRootKind::Body,
            4 => ContentRootKind::Cell,
            5 => ContentRootKind::FixedBody,
            _ => return Err(semantic_invalid("native content root kind is unknown")),
        };
        typed_roots.push(ContentRoot {
            key: ContentRootKey::new(root.key)
                .ok_or_else(|| semantic_invalid("native content root key is absent"))?,
            owner: OwnerKey::new(root.owner)
                .ok_or_else(|| semantic_invalid("native content root owner key is absent"))?,
            ordinal: root.ordinal,
            kind,
            provenance: ProvenanceKey::new(root.provenance)
                .ok_or_else(|| semantic_invalid("native content root provenance key is absent"))?,
        });
    }

    let mut typed_atoms = Vec::new();
    typed_atoms
        .try_reserve_exact(content_atoms.len())
        .map_err(semantic_allocation)?;
    for atom in content_atoms {
        let kind = match atom.kind {
            ATOM_TEXT => ContentAtomKind::Text {
                text: atom.text,
                display_override: atom.display_override,
            },
            ATOM_WHITESPACE => ContentAtomKind::Whitespace {
                text: atom.text,
                display_override: atom.display_override,
                breakable: atom.whitespace_breakable,
            },
            ATOM_BREAK_OPPORTUNITY => ContentAtomKind::BreakOpportunity,
            ATOM_HARD_BREAK => ContentAtomKind::HardBreak,
            _ => return Err(semantic_invalid("native content atom kind is unknown")),
        };
        let role = atom
            .role
            .map(|role| match role {
                1 => Ok(NativeRole::Flag),
                2 => Ok(NativeRole::EnvironmentVariable),
                3 => Ok(NativeRole::Argument),
                4 => Ok(NativeRole::CommandOrDirective),
                5 => Ok(NativeRole::Path),
                _ => Err(semantic_invalid("native content role is unknown")),
            })
            .transpose()?;
        typed_atoms.push(ContentAtom {
            key: ContentAtomKey::new(atom.key)
                .ok_or_else(|| semantic_invalid("native content atom key is absent"))?,
            root: ContentRootKey::new(atom.root)
                .ok_or_else(|| semantic_invalid("native content atom root key is absent"))?,
            ordinal: atom.ordinal,
            owner: OwnerKey::new(atom.owner)
                .ok_or_else(|| semantic_invalid("native content atom owner key is absent"))?,
            kind,
            style: StructuredStyle::from_flags([
                atom.style_flags & 1 != 0,
                atom.style_flags & 2 != 0,
                atom.style_flags & 4 != 0,
                atom.style_flags & 8 != 0,
            ]),
            role,
            link: atom
                .link
                .map(|link| {
                    LinkOccurrenceKey::new(link)
                        .ok_or_else(|| semantic_invalid("native atom link key is absent"))
                })
                .transpose()?,
            provenance: ProvenanceKey::new(atom.provenance)
                .ok_or_else(|| semantic_invalid("native atom provenance key is absent"))?,
        });
    }

    let mut typed_refs = Vec::new();
    typed_refs
        .try_reserve_exact(content_refs.len())
        .map_err(semantic_allocation)?;
    for content_ref in content_refs {
        typed_refs.push(ContentRef {
            atom: ContentAtomKey::new(content_ref.atom)
                .ok_or_else(|| semantic_invalid("native content reference atom key is absent"))?,
            bytes: content_ref.bytes,
        });
    }

    let mut typed_points = Vec::new();
    typed_points
        .try_reserve_exact(content_points.len())
        .map_err(semantic_allocation)?;
    for point in content_points {
        let boundary = match (point.boundary_kind, point.atom) {
            (1, None) => PointBoundary::BetweenAtoms {
                atom_boundary: point.atom_boundary,
            },
            (2, Some(atom)) => PointBoundary::InAtom {
                atom: ContentAtomKey::new(atom)
                    .ok_or_else(|| semantic_invalid("native point atom key is absent"))?,
                byte_offset: point.byte_offset,
            },
            _ => return Err(semantic_invalid("native content point boundary is invalid")),
        };
        typed_points.push(ContentPoint {
            key: ContentPointKey::new(point.key)
                .ok_or_else(|| semantic_invalid("native content point key is absent"))?,
            root: ContentRootKey::new(point.root)
                .ok_or_else(|| semantic_invalid("native content point root key is absent"))?,
            ordinal: point.ordinal,
            owner: OwnerKey::new(point.owner)
                .ok_or_else(|| semantic_invalid("native content point owner key is absent"))?,
            boundary,
            scalar_boundary: point.scalar_boundary,
            provenance: ProvenanceKey::new(point.provenance)
                .ok_or_else(|| semantic_invalid("native content point provenance is absent"))?,
        });
    }

    let mut typed_label_parts = Vec::new();
    typed_label_parts
        .try_reserve_exact(link_label_parts.len())
        .map_err(semantic_allocation)?;
    for part in link_label_parts {
        let atom = ContentAtomKey::new(part.atom)
            .ok_or_else(|| semantic_invalid("native link label atom key is absent"))?;
        typed_label_parts.push(match part.kind {
            LINK_LABEL_CONTENT => LinkLabelPart::Content(ContentRef {
                atom,
                bytes: part.bytes,
            }),
            LINK_LABEL_HARD_BREAK => LinkLabelPart::HardBreak(atom),
            _ => return Err(semantic_invalid("native link label part kind is unknown")),
        });
    }

    let mut typed_links = Vec::new();
    typed_links
        .try_reserve_exact(links.len())
        .map_err(semantic_allocation)?;
    for link in links {
        let target = match (link.target_kind, link.target_b) {
            (1, None) => NativeLinkTarget::External(link.target_a),
            (2, None) => NativeLinkTarget::Email(link.target_a),
            (3, None) => NativeLinkTarget::Document(link.target_a),
            (4, Some(section)) => NativeLinkTarget::Manual {
                name: link.target_a,
                section,
            },
            (5, None) => NativeLinkTarget::Section(link.target_a),
            _ => return Err(semantic_invalid("native link target shape is invalid")),
        };
        let first = link
            .first_label_part
            .checked_sub(1)
            .ok_or_else(|| semantic_invalid("native link label part key is absent"))?;
        let start = usize::try_from(first)
            .map_err(|_| semantic_invalid("native link label start does not fit usize"))?;
        let count = usize::try_from(link.label_part_count)
            .map_err(|_| semantic_invalid("native link label count does not fit usize"))?;
        let end = start
            .checked_add(count)
            .ok_or_else(|| semantic_invalid("native link label range overflows usize"))?;
        typed_links.push(LinkOccurrence {
            key: LinkOccurrenceKey::new(link.key)
                .ok_or_else(|| semantic_invalid("native link key is absent"))?,
            owner: OwnerKey::new(link.owner)
                .ok_or_else(|| semantic_invalid("native link owner key is absent"))?,
            target,
            title: link.title,
            label_parts: start..end,
            provenance: ProvenanceKey::new(link.provenance)
                .ok_or_else(|| semantic_invalid("native link provenance key is absent"))?,
        });
    }

    let mut typed_anchors = Vec::new();
    typed_anchors
        .try_reserve_exact(anchors.len())
        .map_err(semantic_allocation)?;
    for anchor in anchors {
        let origin = match anchor.origin {
            value if value == u32::from(TARGET_ORIGIN_GENERATED) => NativeTargetOrigin::Generated,
            value if value == u32::from(TARGET_ORIGIN_AUTHORED) => NativeTargetOrigin::Authored,
            _ => return Err(semantic_invalid("native anchor target origin is unknown")),
        };
        typed_anchors.push(AnchorEvidence {
            key: AnchorEvidenceKey::new(anchor.key)
                .ok_or_else(|| semantic_invalid("native anchor evidence key is absent"))?,
            owner: OwnerKey::new(anchor.owner)
                .ok_or_else(|| semantic_invalid("native anchor owner key is absent"))?,
            point: ContentPointKey::new(anchor.point)
                .ok_or_else(|| semantic_invalid("native anchor point key is absent"))?,
            target: anchor.target,
            origin,
            provenance: ProvenanceKey::new(anchor.provenance)
                .ok_or_else(|| semantic_invalid("native anchor provenance key is absent"))?,
        });
    }

    let mut typed_heading_evidence = Vec::new();
    typed_heading_evidence
        .try_reserve_exact(heading_evidence.len())
        .map_err(semantic_allocation)?;
    for heading in heading_evidence {
        typed_heading_evidence.push(HeadingEvidence {
            key: HeadingEvidenceKey::new(heading.key)
                .ok_or_else(|| semantic_invalid("native heading evidence key is absent"))?,
            block: NativeBlockKey::new(heading.block)
                .ok_or_else(|| semantic_invalid("native heading block key is absent"))?,
            owner: OwnerKey::new(heading.owner)
                .ok_or_else(|| semantic_invalid("native heading owner key is absent"))?,
            authored_phrase: heading.authored_phrase,
            provenance: ProvenanceKey::new(heading.provenance)
                .ok_or_else(|| semantic_invalid("native heading provenance key is absent"))?,
        });
    }

    let mut typed_blocks = Vec::new();
    typed_blocks
        .try_reserve_exact(blocks.len())
        .map_err(semantic_allocation)?;
    for block in blocks {
        let kind = match block.kind {
            BLOCK_HEADING => NativeBlockKind::Heading,
            BLOCK_PARAGRAPH => NativeBlockKind::Paragraph,
            BLOCK_LIST => NativeBlockKind::List,
            BLOCK_DEFINITION_LIST => NativeBlockKind::DefinitionList,
            BLOCK_TABLE => NativeBlockKind::Table,
            BLOCK_INDENTED => NativeBlockKind::Indented,
            BLOCK_FIXED_DISPLAY => NativeBlockKind::FixedDisplay,
            BLOCK_VERTICAL_SPACE => NativeBlockKind::VerticalSpace,
            BLOCK_THEMATIC_BREAK => NativeBlockKind::ThematicBreak,
            _ => return Err(semantic_invalid("native block kind is unknown")),
        };
        typed_blocks.push(NativeBlock {
            key: NativeBlockKey::new(block.key)
                .ok_or_else(|| semantic_invalid("native block key is absent"))?,
            owner: OwnerKey::new(block.owner)
                .ok_or_else(|| semantic_invalid("native block owner key is absent"))?,
            kind,
            parent: block
                .parent
                .map(|parent| {
                    NativeBlockKey::new(parent)
                        .ok_or_else(|| semantic_invalid("native block parent key is absent"))
                })
                .transpose()?,
            ordinal: block.ordinal,
            provenance: ProvenanceKey::new(block.provenance)
                .ok_or_else(|| semantic_invalid("native block provenance key is absent"))?,
            root: block
                .root
                .map(|root| {
                    ContentRootKey::new(root)
                        .ok_or_else(|| semantic_invalid("native block root key is absent"))
                })
                .transpose()?,
            table: block
                .table
                .map(|table| {
                    NativeTableKey::new(table)
                        .ok_or_else(|| semantic_invalid("native block table key is absent"))
                })
                .transpose()?,
            fixed_view: block
                .fixed_view
                .map(|view| {
                    NativeFixedViewKey::new(view)
                        .ok_or_else(|| semantic_invalid("native block fixed-view key is absent"))
                })
                .transpose()?,
        });
    }

    let mut typed_lists = Vec::new();
    typed_lists
        .try_reserve_exact(lists.len())
        .map_err(semantic_allocation)?;
    for list in lists {
        let kind = match list.kind {
            LIST_BULLET => NativeListKind::Bullet,
            LIST_ORDERED => NativeListKind::Ordered,
            LIST_PLAIN => NativeListKind::Plain,
            LIST_DEFINITION => NativeListKind::Definition,
            LIST_NATIVE_MARKER => NativeListKind::NativeMarker,
            _ => return Err(semantic_invalid("native list kind is unknown")),
        };
        typed_lists.push(NativeList {
            key: NativeListKey::new(list.key)
                .ok_or_else(|| semantic_invalid("native list key is absent"))?,
            block: NativeBlockKey::new(list.block)
                .ok_or_else(|| semantic_invalid("native list block key is absent"))?,
            kind,
            compact: list.compact,
            start: list.start,
            provenance: ProvenanceKey::new(list.provenance)
                .ok_or_else(|| semantic_invalid("native list provenance key is absent"))?,
        });
    }

    let mut typed_items = Vec::new();
    typed_items
        .try_reserve_exact(items.len())
        .map_err(semantic_allocation)?;
    for item in items {
        let forms = semantic_range(item.first_form, item.form_count, "item form")?;
        typed_items.push(NativeItem {
            key: NativeItemKey::new(item.key)
                .ok_or_else(|| semantic_invalid("native item key is absent"))?,
            list: NativeListKey::new(item.list)
                .ok_or_else(|| semantic_invalid("native item list key is absent"))?,
            owner: OwnerKey::new(item.owner)
                .ok_or_else(|| semantic_invalid("native item owner key is absent"))?,
            ordinal: item.ordinal,
            forms,
            provenance: ProvenanceKey::new(item.provenance)
                .ok_or_else(|| semantic_invalid("native item provenance key is absent"))?,
        });
    }

    let mut typed_tables = Vec::new();
    typed_tables
        .try_reserve_exact(tables.len())
        .map_err(semantic_allocation)?;
    for table in tables {
        typed_tables.push(NativeTable {
            key: NativeTableKey::new(table.key)
                .ok_or_else(|| semantic_invalid("native table key is absent"))?,
            block: NativeBlockKey::new(table.block)
                .ok_or_else(|| semantic_invalid("native table block key is absent"))?,
            fixed_view: table
                .fixed_view
                .map(|view| {
                    NativeFixedViewKey::new(view)
                        .ok_or_else(|| semantic_invalid("native table fixed-view key is absent"))
                })
                .transpose()?,
            provenance: ProvenanceKey::new(table.provenance)
                .ok_or_else(|| semantic_invalid("native table provenance key is absent"))?,
        });
    }
    let mut typed_table_rows = Vec::new();
    typed_table_rows
        .try_reserve_exact(table_rows.len())
        .map_err(semantic_allocation)?;
    for row in table_rows {
        typed_table_rows.push(NativeTableRow {
            key: NativeTableRowKey::new(row.key)
                .ok_or_else(|| semantic_invalid("native table row key is absent"))?,
            table: NativeTableKey::new(row.table)
                .ok_or_else(|| semantic_invalid("native table row table key is absent"))?,
            ordinal: row.ordinal,
            kind: match row.kind {
                1 => NativeTableRowKind::Data,
                2 => NativeTableRowKind::HorizontalRule,
                3 => NativeTableRowKind::DoubleHorizontalRule,
                4 => NativeTableRowKind::LayoutRule,
                _ => return Err(semantic_invalid("native table row kind is unknown")),
            },
            point: row
                .point
                .map(|point| {
                    ContentPointKey::new(point)
                        .ok_or_else(|| semantic_invalid("native table row point key is absent"))
                })
                .transpose()?,
            provenance: ProvenanceKey::new(row.provenance)
                .ok_or_else(|| semantic_invalid("native table row provenance key is absent"))?,
        });
    }
    let mut typed_table_cells = Vec::new();
    typed_table_cells
        .try_reserve_exact(table_cells.len())
        .map_err(semantic_allocation)?;
    for cell in table_cells {
        typed_table_cells.push(NativeTableCell {
            key: NativeTableCellKey::new(cell.key)
                .ok_or_else(|| semantic_invalid("native table cell key is absent"))?,
            row: NativeTableRowKey::new(cell.row)
                .ok_or_else(|| semantic_invalid("native table cell row key is absent"))?,
            column: cell.column,
            owner: OwnerKey::new(cell.owner)
                .ok_or_else(|| semantic_invalid("native table cell owner key is absent"))?,
            kind: match cell.kind {
                1 => NativeTableCellKind::Text,
                2 => NativeTableCellKind::HorizontalRule,
                3 => NativeTableCellKind::DoubleHorizontalRule,
                4 => NativeTableCellKind::IsolatedHorizontalRule,
                5 => NativeTableCellKind::IsolatedDoubleHorizontalRule,
                _ => return Err(semantic_invalid("native table cell kind is unknown")),
            },
            alignment: match cell.alignment {
                1 => NativeTableAlignment::Left,
                2 => NativeTableAlignment::Center,
                3 => NativeTableAlignment::Right,
                _ => return Err(semantic_invalid("native table cell alignment is unknown")),
            },
            row_span: cell.row_span,
            column_span: cell.column_span,
            point: cell
                .point
                .map(|point| {
                    ContentPointKey::new(point)
                        .ok_or_else(|| semantic_invalid("native table cell point key is absent"))
                })
                .transpose()?,
            provenance: ProvenanceKey::new(cell.provenance)
                .ok_or_else(|| semantic_invalid("native table cell provenance key is absent"))?,
        });
    }

    let mut typed_forms = Vec::new();
    typed_forms
        .try_reserve_exact(forms.len())
        .map_err(semantic_allocation)?;
    for form in forms {
        typed_forms.push(NativeForm {
            key: NativeFormKey::new(form.key)
                .ok_or_else(|| semantic_invalid("native form key is absent"))?,
            owner: OwnerKey::new(form.owner)
                .ok_or_else(|| semantic_invalid("native form owner key is absent"))?,
            role: semantic_role(form.role)?,
            refs: semantic_range(
                Some(form.first_ref),
                form.ref_count,
                "form content reference",
            )?,
            provenance: ProvenanceKey::new(form.provenance)
                .ok_or_else(|| semantic_invalid("native form provenance key is absent"))?,
        });
    }

    let mut typed_name_hints = Vec::new();
    typed_name_hints
        .try_reserve_exact(name_hints.len())
        .map_err(semantic_allocation)?;
    for hint in name_hints {
        typed_name_hints.push(NativeNameHint {
            key: NativeNameHintKey::new(hint.key)
                .ok_or_else(|| semantic_invalid("native name hint key is absent"))?,
            form: NativeFormKey::new(hint.form)
                .ok_or_else(|| semantic_invalid("native name hint form key is absent"))?,
            refs: semantic_range(Some(hint.first_ref), hint.ref_count, "name hint reference")?,
            provenance: ProvenanceKey::new(hint.provenance)
                .ok_or_else(|| semantic_invalid("native name hint provenance key is absent"))?,
        });
    }

    let mut typed_fixed_views = Vec::new();
    typed_fixed_views
        .try_reserve_exact(fixed_views.len())
        .map_err(semantic_allocation)?;
    for view in fixed_views {
        typed_fixed_views.push(NativeFixedView {
            key: NativeFixedViewKey::new(view.key)
                .ok_or_else(|| semantic_invalid("native fixed view key is absent"))?,
            owner: OwnerKey::new(view.owner)
                .ok_or_else(|| semantic_invalid("native fixed view owner is absent"))?,
            block: NativeBlockKey::new(view.block)
                .ok_or_else(|| semantic_invalid("native fixed view block is absent"))?,
            table: view
                .table
                .map(|key| {
                    NativeTableKey::new(key)
                        .ok_or_else(|| semantic_invalid("native fixed view table is absent"))
                })
                .transpose()?,
            provenance: ProvenanceKey::new(view.provenance)
                .ok_or_else(|| semantic_invalid("native fixed view provenance is absent"))?,
        });
    }
    let mut typed_fixed_lines = Vec::new();
    typed_fixed_lines
        .try_reserve_exact(fixed_lines.len())
        .map_err(semantic_allocation)?;
    for line in fixed_lines {
        typed_fixed_lines.push(NativeFixedLine {
            key: NativeFixedLineKey::new(line.key)
                .ok_or_else(|| semantic_invalid("native fixed line key is absent"))?,
            view: NativeFixedViewKey::new(line.view)
                .ok_or_else(|| semantic_invalid("native fixed line view is absent"))?,
            ordinal: line.ordinal,
            terminal_columns: line.terminal_columns,
        });
    }
    let mut typed_placements = Vec::new();
    typed_placements
        .try_reserve_exact(placements.len())
        .map_err(semantic_allocation)?;
    for placement in placements {
        let target = match placement.target_kind {
            1 => NativePlacementTarget::Content {
                atom: ContentAtomKey::new(
                    placement
                        .atom
                        .ok_or_else(|| semantic_invalid("native placement atom is absent"))?,
                )
                .ok_or_else(|| semantic_invalid("native placement atom key is absent"))?,
                byte_start: placement.bytes.start,
                byte_end: placement.bytes.end,
            },
            2 => NativePlacementTarget::Point(
                ContentPointKey::new(
                    placement
                        .point
                        .ok_or_else(|| semantic_invalid("native placement point is absent"))?,
                )
                .ok_or_else(|| semantic_invalid("native placement point key is absent"))?,
            ),
            _ => return Err(semantic_invalid("native placement target is unknown")),
        };
        let map = match placement.cell_map_kind {
            1 => NativeCellMapKind::Affine {
                columns_per_scalar: u8::try_from(placement.cell_map_value)
                    .map_err(|_| semantic_invalid("native affine map width is invalid"))?,
            },
            2 => NativeCellMapKind::GraphemeCluster,
            3 => NativeCellMapKind::Overlay,
            _ => return Err(semantic_invalid("native cell map is unknown")),
        };
        typed_placements.push(NativePlacement {
            line: NativeFixedLineKey::new(placement.line)
                .ok_or_else(|| semantic_invalid("native placement line is absent"))?,
            target,
            scalar_range: placement.scalars,
            column_range: placement.columns,
            map,
        });
    }
    let mut typed_decorations = Vec::new();
    typed_decorations
        .try_reserve_exact(decorations.len())
        .map_err(semantic_allocation)?;
    for decoration in decorations {
        typed_decorations.push(NativeDecoration {
            line: NativeFixedLineKey::new(decoration.line)
                .ok_or_else(|| semantic_invalid("native decoration line is absent"))?,
            text: decoration.text,
            column_range: decoration.columns,
            kind: match decoration.kind {
                1 => NativeDecorationKind::Border,
                2 => NativeDecorationKind::Rule,
                3 => NativeDecorationKind::Padding,
                _ => return Err(semantic_invalid("native decoration kind is unknown")),
            },
            provenance: ProvenanceKey::new(decoration.provenance)
                .ok_or_else(|| semantic_invalid("native decoration provenance is absent"))?,
        });
    }

    let mut typed_diagnostics = Vec::new();
    typed_diagnostics
        .try_reserve_exact(diagnostics.len())
        .map_err(semantic_allocation)?;
    for diagnostic in diagnostics {
        let level = match diagnostic.level {
            1 => StructuredDiagnosticLevel::Style,
            2 => StructuredDiagnosticLevel::Warning,
            3 => StructuredDiagnosticLevel::Error,
            4 => StructuredDiagnosticLevel::Unsupported,
            _ => return Err(semantic_invalid("native diagnostic level is unknown")),
        };
        typed_diagnostics.push(NativeDiagnostic {
            level,
            code: StructuredDiagnosticCode::from_native_ordinal(diagnostic.code)
                .ok_or_else(|| semantic_invalid("native diagnostic code is unknown"))?,
            message: diagnostic.message,
            span: diagnostic
                .span
                .map(|span| {
                    SpanKey::new(span)
                        .ok_or_else(|| semantic_invalid("native diagnostic span key is absent"))
                })
                .transpose()?,
            owner: diagnostic
                .owner
                .map(|owner| {
                    OwnerKey::new(owner)
                        .ok_or_else(|| semantic_invalid("native diagnostic owner key is absent"))
                })
                .transpose()?,
        });
    }

    Ok(StructuredDocument {
        root_source,
        profile,
        width,
        metadata,
        sources: typed_sources,
        spans: typed_spans,
        provenances: typed_provenances,
        owners: typed_owners,
        content_roots: typed_roots,
        content_atoms: typed_atoms,
        content_refs: typed_refs,
        content_points: typed_points,
        links: typed_links,
        link_label_parts: typed_label_parts,
        anchors: typed_anchors,
        heading_evidence: typed_heading_evidence,
        blocks: typed_blocks,
        lists: typed_lists,
        items: typed_items,
        tables: typed_tables,
        table_rows: typed_table_rows,
        table_cells: typed_table_cells,
        fixed_views: typed_fixed_views,
        fixed_lines: typed_fixed_lines,
        placements: typed_placements,
        decorations: typed_decorations,
        forms: typed_forms,
        name_hints: typed_name_hints,
        diagnostics: typed_diagnostics,
    })
}

fn semantic_range(
    first: Option<u32>,
    count: u32,
    label: &str,
) -> Result<std::ops::Range<usize>, crate::structured::StructuredError> {
    if count == 0 {
        return if first.is_none() {
            Ok(0..0)
        } else {
            Err(semantic_invalid(&format!(
                "native {label} empty range has a first key"
            )))
        };
    }
    let first = first
        .and_then(|first| first.checked_sub(1))
        .ok_or_else(|| semantic_invalid(&format!("native {label} first key is absent")))?;
    let start = usize::try_from(first)
        .map_err(|_| semantic_invalid(&format!("native {label} start does not fit usize")))?;
    let count = usize::try_from(count)
        .map_err(|_| semantic_invalid(&format!("native {label} count does not fit usize")))?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| semantic_invalid(&format!("native {label} range overflows usize")))?;
    Ok(start..end)
}

fn semantic_role(
    role: Option<u32>,
) -> Result<Option<crate::structured::NativeRole>, crate::structured::StructuredError> {
    role.map(|role| match role {
        1 => Ok(crate::structured::NativeRole::Flag),
        2 => Ok(crate::structured::NativeRole::EnvironmentVariable),
        3 => Ok(crate::structured::NativeRole::Argument),
        4 => Ok(crate::structured::NativeRole::CommandOrDirective),
        5 => Ok(crate::structured::NativeRole::Path),
        _ => Err(semantic_invalid("native content role is unknown")),
    })
    .transpose()
}

pub(super) fn semantic_source_format(
    format: u32,
) -> Result<crate::structured::SourceFormat, crate::structured::StructuredError> {
    match format {
        FORMAT_MAN => Ok(crate::structured::SourceFormat::Man),
        FORMAT_MDOC => Ok(crate::structured::SourceFormat::Mdoc),
        _ => Err(semantic_invalid("native source format is unknown")),
    }
}

pub(super) fn semantic_invalid(message: &str) -> crate::structured::StructuredError {
    crate::structured::StructuredError::new(
        crate::structured::StructuredErrorKind::InvalidResult,
        crate::structured::StructuredStage::Check,
        None,
        0,
        0,
        message,
    )
}

pub(super) fn semantic_allocation(
    _: std::collections::TryReserveError,
) -> crate::structured::StructuredError {
    crate::structured::StructuredError::new(
        crate::structured::StructuredErrorKind::BuilderAllocation,
        crate::structured::StructuredStage::Check,
        None,
        0,
        0,
        "structured semantic transfer allocation failed",
    )
}

pub(super) fn semantic_error(error: &NativeStructuredError) -> crate::structured::StructuredError {
    use crate::structured::{
        StructuredError, StructuredErrorKind, StructuredLimitKind, StructuredStage,
    };

    let kind = match error.status {
        STATUS_INVALID_INPUT => StructuredErrorKind::InvalidInput,
        STATUS_REENTRANT => StructuredErrorKind::Reentrant,
        STATUS_BUDGET => StructuredErrorKind::Budget,
        STATUS_BUILDER_ALLOC => StructuredErrorKind::BuilderAllocation,
        STATUS_NATIVE => StructuredErrorKind::Native,
        STATUS_RELATION => StructuredErrorKind::InvalidResult,
        STATUS_UNSUPPORTED => StructuredErrorKind::Unsupported,
        _ => return semantic_invalid("native failure status discriminator is unknown"),
    };
    let stage = match error.stage {
        1 => StructuredStage::Marshal,
        2 => StructuredStage::Resolve,
        3 => StructuredStage::Parse,
        4 => StructuredStage::Render,
        5 => StructuredStage::Finalize,
        6 => StructuredStage::Check,
        _ => return semantic_invalid("native failure stage discriminator is unknown"),
    };
    let limit = match error.limit_kind {
        0 => None,
        1 => Some(StructuredLimitKind::InputSources),
        2 => Some(StructuredLimitKind::Sources),
        3 => Some(StructuredLimitKind::SourcePathBytes),
        4 => Some(StructuredLimitKind::DecodedSourceBytesPerSource),
        5 => Some(StructuredLimitKind::DecodedSourceBytesTotal),
        6 => Some(StructuredLimitKind::SourceMapEntries),
        7 => Some(StructuredLimitKind::SourceMapBytes),
        8 => Some(StructuredLimitKind::BuilderOperations),
        9 => Some(StructuredLimitKind::BuilderAllocatedBytes),
        10 => Some(StructuredLimitKind::ContentBytes),
        11 => Some(StructuredLimitKind::Owners),
        12 => Some(StructuredLimitKind::Blocks),
        13 => Some(StructuredLimitKind::ContentAtoms),
        14 => Some(StructuredLimitKind::ContentRefs),
        15 => Some(StructuredLimitKind::ContentPoints),
        16 => Some(StructuredLimitKind::Links),
        17 => Some(StructuredLimitKind::Tables),
        18 => Some(StructuredLimitKind::TableRows),
        19 => Some(StructuredLimitKind::TableCells),
        20 => Some(StructuredLimitKind::FixedViews),
        21 => Some(StructuredLimitKind::FixedLines),
        22 => Some(StructuredLimitKind::Placements),
        23 => Some(StructuredLimitKind::Decorations),
        24 => Some(StructuredLimitKind::Forms),
        25 => Some(StructuredLimitKind::NameHints),
        26 => Some(StructuredLimitKind::Relations),
        27 => Some(StructuredLimitKind::ConnectionAtoms),
        28 => Some(StructuredLimitKind::AnnotationRuns),
        29 => Some(StructuredLimitKind::AnnotationMutations),
        30 => Some(StructuredLimitKind::RelationEdges),
        31 => Some(StructuredLimitKind::Diagnostics),
        32 => Some(StructuredLimitKind::TransferObjects),
        33 => Some(StructuredLimitKind::TransferEdges),
        34 => Some(StructuredLimitKind::TransferBytes),
        35 => Some(StructuredLimitKind::NestingDepth),
        36 => Some(StructuredLimitKind::IncludeDepth),
        37 => Some(StructuredLimitKind::AnchorEvidence),
        38 => Some(StructuredLimitKind::HeadingEvidence),
        39 => Some(StructuredLimitKind::LinkLabelParts),
        _ => return semantic_invalid("native failure limit discriminator is unknown"),
    };
    StructuredError::new(
        kind,
        stage,
        limit,
        error.observed,
        error.allowed,
        format!("native structured rendering failed at {stage:?} with {kind:?}"),
    )
}

pub(super) fn raw_limits(limits: &crate::structured::StructuredLimits) -> Limits {
    Limits {
        max_input_sources: limits.max_input_sources,
        max_sources: limits.max_sources,
        max_source_path_bytes: limits.max_source_path_bytes,
        max_decoded_source_bytes_per_source: limits.max_decoded_source_bytes_per_source,
        max_decoded_source_bytes_total: limits.max_decoded_source_bytes_total,
        max_source_map_entries: limits.max_source_map_entries,
        max_source_map_bytes: limits.max_source_map_bytes,
        max_builder_operations: limits.max_builder_operations,
        max_builder_allocated_bytes: limits.max_builder_allocated_bytes,
        max_content_bytes: limits.max_content_bytes,
        max_owners: limits.max_owners,
        max_blocks: limits.max_blocks,
        max_content_atoms: limits.max_content_atoms,
        max_content_refs: limits.max_content_refs,
        max_content_points: limits.max_content_points,
        max_links: limits.max_links,
        max_tables: limits.max_tables,
        max_table_rows: limits.max_table_rows,
        max_table_cells: limits.max_table_cells,
        max_fixed_views: limits.max_fixed_views,
        max_fixed_lines: limits.max_fixed_lines,
        max_placements: limits.max_placements,
        max_decorations: limits.max_decorations,
        max_forms: limits.max_forms,
        max_name_hints: limits.max_name_hints,
        max_relations: limits.max_relations,
        max_connection_atoms: limits.max_connection_atoms,
        max_annotation_runs: limits.max_annotation_runs,
        max_annotation_mutations: limits.max_annotation_mutations,
        max_relation_edges: limits.max_relation_edges,
        max_diagnostics: limits.max_diagnostics,
        max_transfer_objects: limits.max_transfer_objects,
        max_transfer_edges: limits.max_transfer_edges,
        max_transfer_bytes: limits.max_transfer_bytes,
        max_nesting_depth: limits.max_nesting_depth,
        max_include_depth: limits.max_include_depth,
        max_anchor_evidence: limits.max_anchor_evidence,
        max_heading_evidence: limits.max_heading_evidence,
        max_link_label_parts: limits.max_link_label_parts,
        reserved: 0,
    }
}
