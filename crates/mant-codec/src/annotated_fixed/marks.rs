//! Projection of native heading, owner, link, anchor and region marks.

use super::{
    AnchorMark, AnnotatedDocument, AnnotatedMark, AnnotatedProjectionError, Diagnostic,
    DiagnosticImpact, DiagnosticLevel, DisplayPoint, HeadingMark, Identities, KeyMap, LinkMark,
    LinkTarget, OwnerMark, OwnerRole, RegionKind, RegionMark, Result, TextJoin, TextSelection,
    mark_source, point, region_selection, selection,
};

pub(super) fn project_heading(
    page: &AnnotatedDocument,
    keys: &KeyMap,
    identities: &Identities,
    mark: &AnnotatedMark,
) -> Result<HeadingMark> {
    let title = region_selection(page, mark.title_region)?;
    let region = page
        .marks
        .get(mark.title_region.saturating_sub(1) as usize)
        .filter(|region| region.key == mark.title_region && region.kind == 5)
        .ok_or(AnnotatedProjectionError::Relation(
            "missing heading title region",
        ))?;
    // man_term.c/mdoc_term.c emit the region point after the heading pre
    // handler and before its child output. Keep that structural boundary if
    // it precedes the first visible title run, including native indentation.
    // A point inside a coalesced run cannot be converted from terminal cells
    // to UTF-8 bytes, so that case uses the exact first title byte instead.
    let native_at = point(region.point.ok_or(AnnotatedProjectionError::Relation(
        "heading title has no final point",
    ))?)?;
    let at = if let Some(first) = title.parts.first() {
        let run = page.runs.get((first.run.get() - 1) as usize).ok_or(
            AnnotatedProjectionError::Relation("heading title run is missing"),
        )?;
        let run_index = first.run.get() - 1;
        let row_index = page
            .rows
            .partition_point(|row| row.first_run.saturating_add(row.run_count) <= run_index);
        let run_row = page
            .rows
            .get(row_index)
            .ok_or(AnnotatedProjectionError::Relation(
                "heading title run has no row",
            ))?;
        if run_index < run_row.first_run {
            return Err(AnnotatedProjectionError::Relation(
                "heading title run has no row",
            ));
        }
        match native_at {
            DisplayPoint::RowColumn { row, column }
                if row.get() < run_row.key
                    || (row.get() == run_row.key && column <= run.column) =>
            {
                native_at
            }
            DisplayPoint::RowColumn { row, .. } if row.get() <= run_row.key => {
                DisplayPoint::RunBoundary {
                    run: first.run,
                    byte: first.start_byte,
                }
            }
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "heading point follows title",
                ));
            }
        }
    } else {
        native_at
    };
    Ok(HeadingMark {
        key: keys.required(mark.key, 1)?,
        id: identities.heading(mark.key)?,
        fragment_aliases: identities.heading_aliases(mark.key)?,
        generated_fragment_aliases: identities.heading_generated_aliases(mark.key)?,
        rendered_fragment_aliases: identities.heading_rendered_aliases(mark.key)?,
        parent: keys.lookup(mark.parent, 1)?,
        // MAN_SS/MDOC_Ss remain subsections even before any top heading.
        level_hint: if mark.flags & 8 != 0 { 2 } else { 1 },
        at,
        title,
        direct_body: region_selection(page, mark.body_region)?,
        source: mark_source(mark)?,
    })
}

pub(super) fn project_owner(
    page: &AnnotatedDocument,
    keys: &KeyMap,
    identities: &Identities,
    mark: &AnnotatedMark,
    body_regions: &[u32],
) -> Result<OwnerMark> {
    let head = region_selection(page, mark.title_region)?;
    if body_regions.last().copied() != Some(mark.body_region) {
        return Err(AnnotatedProjectionError::Relation(
            "owner body pointer is not its latest direct body",
        ));
    }
    let mut pieces = Vec::new();
    for &region_key in body_regions {
        let body = region_selection(page, region_key)?;
        for (index, part) in body.parts.into_iter().enumerate() {
            let join = if index == 0 {
                None
            } else {
                Some(body.joins[index - 1].clone())
            };
            pieces.push((part, region_key, index, join));
        }
    }
    pieces.sort_unstable_by_key(|(part, _, _, _)| (part.run, part.start_byte));
    let mut direct_body = TextSelection {
        parts: Vec::with_capacity(pieces.len()),
        joins: Vec::with_capacity(pieces.len().saturating_sub(1)),
    };
    let mut previous = None;
    for (part, region, index, join) in pieces {
        if let Some((previous_region, previous_index)) = previous {
            direct_body.joins.push(
                if previous_region == region && previous_index + 1 == index {
                    join.ok_or(AnnotatedProjectionError::Relation(
                        "owner body lost its native join",
                    ))?
                } else {
                    TextJoin::Unknown
                },
            );
        }
        direct_body.parts.push(part);
        previous = Some((region, index));
    }
    let empty_point = if head.parts.is_empty() && direct_body.parts.is_empty() {
        Some(point(mark.point.ok_or(
            AnnotatedProjectionError::Relation("empty owner has no final point"),
        )?)?)
    } else {
        None
    };
    Ok(OwnerMark {
        key: keys.required(mark.key, 2)?,
        id: identities.owner(mark.key)?,
        parent: keys.nearest(mark.parent, 2)?,
        section: keys.nearest(mark.parent, 1)?,
        role: if mark.flags & 16 != 0 {
            OwnerRole::Definition
        } else {
            OwnerRole::Other
        },
        entry: None,
        head,
        direct_body,
        empty_point,
        source: mark_source(mark)?,
    })
}

pub(super) fn project_link(
    page: &AnnotatedDocument,
    keys: &KeyMap,
    identities: &Identities,
    mark: &AnnotatedMark,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<LinkMark> {
    let authored = mark_source(mark)?;
    let target = match &mark.link_target {
        None => None,
        Some(native) => match native.kind {
            1 => Some(LinkTarget::External {
                uri: native.primary.clone(),
            }),
            2 => Some(LinkTarget::Email {
                address: native.primary.clone(),
            }),
            3 => Some(LinkTarget::Document {
                name: native.primary.clone(),
                fragment: native.secondary.clone(),
            }),
            4 => Some(LinkTarget::Manual {
                name: native.primary.clone(),
                manual_section: native.secondary.clone(),
            }),
            5 => identities
                .section(&native.primary)
                .map(|id| LinkTarget::Section { id })
                .or_else(|| {
                    // The native occurrence and final label remain. A missing or
                    // ambiguous local destination is not made clickable and does
                    // not invent a canonical ID, matching Flow's AddressPlan.
                    diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Warning,
                        impact: DiagnosticImpact::None,
                        code: Some("unresolved-section-reference".to_owned()),
                        message: format!("cannot resolve section reference: {}", native.primary),
                        source: authored,
                        coverage_scope: None,
                    });
                    None
                }),
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "unknown native link target",
                ));
            }
        },
    };
    Ok(LinkMark {
        key: keys.required(mark.key, 3)?,
        target,
        label: selection(page, mark)?,
        source: authored,
    })
}

pub(super) fn project_anchor(
    keys: &KeyMap,
    identities: &Identities,
    mark: &AnnotatedMark,
) -> Result<AnchorMark> {
    Ok(AnchorMark {
        key: keys.required(mark.key, 4)?,
        id: identities.anchor(mark.key)?,
        section: keys.nearest(mark.parent, 1)?,
        name: mark
            .name
            .clone()
            .ok_or(AnnotatedProjectionError::Relation("anchor has no name"))?,
        rendered_fragment: identities.anchor_rendered(mark.key)?,
        authored: mark.flags & 4 != 0,
        at: point(mark.point.ok_or(AnnotatedProjectionError::Relation(
            "anchor has no final point",
        ))?)?,
        source: mark_source(mark)?,
    })
}

pub(super) fn project_region(
    page: &AnnotatedDocument,
    keys: &KeyMap,
    mark: &AnnotatedMark,
) -> Result<RegionMark> {
    if mark.region_kind == 11 {
        let parent = page.marks.get(mark.parent.saturating_sub(1) as usize);
        if parent.is_none_or(|parent| parent.key != mark.parent || parent.kind != 5)
            || mark.owner != mark.parent
            || mark.source != 0
            || mark.line != 0
            || mark.column != 0
            || mark.token != 0
            || mark.flags != 0
            || mark.title_region != 0
            || mark.body_region != 0
            || mark.native_table_position.is_some()
        {
            return Err(AnnotatedProjectionError::Relation(
                "invalid generated margin region",
            ));
        }
    }
    let selection = selection(page, mark)?;
    let empty_point = if selection.parts.is_empty() {
        Some(point(mark.point.ok_or(
            AnnotatedProjectionError::Relation("empty region has no final point"),
        )?)?)
    } else {
        None
    };
    Ok(RegionMark {
        key: keys.required(mark.key, 5)?,
        parent: keys.nearest(mark.parent, 5)?,
        owner: keys.nearest(mark.owner, 2)?,
        section: keys.nearest(mark.parent, 1)?,
        kind: match mark.region_kind {
            10 => RegionKind::Unsectioned,
            1 => RegionKind::HeadingTitle,
            2 => RegionKind::HeadingBody,
            3 => RegionKind::OwnerHead,
            4 => RegionKind::OwnerBody,
            5 => RegionKind::List,
            6 => RegionKind::Literal,
            7 => RegionKind::TableSpan,
            8 => RegionKind::Equation,
            9 => RegionKind::TableCell,
            11 => RegionKind::Margin,
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "unknown native region kind",
                ));
            }
        },
        selection,
        empty_point,
        source: mark_source(mark)?,
    })
}
