//! Checked one-copy transfer from a handle-bound native view to owned data.

use super::view::CheckedAnnotatedView;
use super::{
    AnnotatedDiagnostic, AnnotatedDisplayPoint, AnnotatedDocument, AnnotatedError, AnnotatedLabel,
    AnnotatedLinkTarget, AnnotatedMark, AnnotatedMetadata, AnnotatedProvenance, AnnotatedRow,
    AnnotatedRun, AnnotatedSelectionPart, AnnotatedSource, AnnotatedSpan, AnnotatedTextJoin,
    AnnotationCoverage, AnnotationCoverageCheck, AnnotationCoverageIssue, FORMAT_MAN, FORMAT_MDOC,
    Handle, ResultView, copy_optional, copy_string, invalid_result, reserve, transfer_alloc,
    transfer_budget, transfer_coverage,
};

// Private native mark flags, checked before a component can become IR role
// evidence. Va/Dv retain their macro identity even when term_word() changes
// the font of every surviving glyph.
const HEAD_VARIABLE: u32 = 1 << 11;
const HEAD_DEFINED_VARIABLE: u32 = 1 << 12;
// Pinned against roff.h by the matching native _Static_asserts.
const MDOC_DV_TOKEN: u32 = 276;
const MDOC_VA_TOKEN: u32 = 295;
const HEAD_ROLE_MASK: u32 = 32 | 64 | 128 | 256 | HEAD_VARIABLE | HEAD_DEFINED_VARIABLE;
const ALLOWED_MARK_FLAGS: u32 = 1 | 4 | 8 | 16 | 512 | 1024 | HEAD_ROLE_MASK;

#[allow(clippy::too_many_lines)] // Mirrors the checked one-copy wire transfer.
pub(super) fn transfer(
    handle: &Handle,
    view: &ResultView,
    limits: &super::super::raw::Limits,
    surface_only: bool,
) -> Result<AnnotatedDocument, AnnotatedError> {
    let CheckedAnnotatedView {
        source_views,
        span_views,
        provenance_views,
        diagnostic_views,
        mark_views,
        selection_part_views,
        join_text_view,
        coverage_check_views,
        coverage_issue_views,
        rows,
        runs,
        text_bytes,
        text_view,
    } = CheckedAnnotatedView::bind(handle, view, limits, surface_only)?;
    let object_count = [
        source_views.len(),
        span_views.len(),
        provenance_views.len(),
        diagnostic_views.len(),
        mark_views.len(),
        selection_part_views.len(),
        coverage_check_views.len(),
        coverage_issue_views.len(),
        rows.len(),
        runs.len(),
    ]
    .into_iter()
    .try_fold(2_u64, |sum, count| {
        sum.checked_add(count as u64).ok_or_else(invalid_result)
    })?;
    if object_count > limits.max_transfer_objects {
        return Err(transfer_budget(
            32,
            object_count,
            limits.max_transfer_objects,
        ));
    }
    let mut transfer_bytes = 0_u64;
    let mut charge = |amount: u64| -> Result<(), AnnotatedError> {
        transfer_bytes = transfer_bytes
            .checked_add(amount)
            .ok_or_else(invalid_result)?;
        if transfer_bytes > limits.max_transfer_bytes {
            return Err(transfer_budget(
                34,
                transfer_bytes,
                limits.max_transfer_bytes,
            ));
        }
        Ok(())
    };
    for (count, item_size) in [
        (source_views.len(), std::mem::size_of::<AnnotatedSource>()),
        (span_views.len(), std::mem::size_of::<AnnotatedSpan>()),
        (
            provenance_views.len(),
            std::mem::size_of::<AnnotatedProvenance>(),
        ),
        (
            diagnostic_views.len(),
            std::mem::size_of::<AnnotatedDiagnostic>(),
        ),
        (mark_views.len(), std::mem::size_of::<AnnotatedMark>()),
        (
            selection_part_views.len(),
            std::mem::size_of::<AnnotatedSelectionPart>(),
        ),
        (
            coverage_check_views.len(),
            std::mem::size_of::<AnnotationCoverageCheck>(),
        ),
        (
            coverage_issue_views.len(),
            std::mem::size_of::<AnnotationCoverageIssue>(),
        ),
        (rows.len(), std::mem::size_of::<AnnotatedRow>()),
        (runs.len(), std::mem::size_of::<AnnotatedRun>()),
    ] {
        charge(
            (count as u64)
                .checked_mul(item_size as u64)
                .ok_or_else(invalid_result)?,
        )?;
    }
    charge(std::mem::size_of::<AnnotatedDocument>() as u64)?;
    charge(std::mem::size_of::<AnnotatedMetadata>() as u64)?;
    charge(text_bytes.len() as u64)?;
    charge(join_text_view.len() as u64)?;
    // Temporary one-byte-per-run census prevents missing or duplicate
    // direct owner/link selections from masquerading as a complete transfer.
    charge(runs.len() as u64)?;
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
        charge(field.len)?;
    }
    for source in source_views {
        charge(source.logical_name.len)?;
    }
    for diagnostic in diagnostic_views {
        charge(diagnostic.message.len)?;
    }
    for mark in mark_views {
        // These optional strings use the content-byte ceiling when copied.
        // Classify an over-limit length before checked_bytes can return a
        // generic relation error; a transfer budget must never be retried
        // as a successful body-only semantic downgrade.
        for length in [mark.name_length, mark.target_a.len, mark.target_b.len] {
            if length > limits.max_content_bytes {
                return Err(transfer_budget(10, length, limits.max_content_bytes));
            }
        }
        charge(mark.name_length)?;
        charge(mark.target_a.len)?;
        charge(mark.target_b.len)?;
    }
    let metadata = view.metadata;
    if (metadata.macroset != FORMAT_MAN && metadata.macroset != FORMAT_MDOC)
        || metadata.reserved != 0
        || metadata.reserved_bytes != [0; 3]
        || metadata.has_body > 1
    {
        return Err(invalid_result());
    }
    let metadata = AnnotatedMetadata {
        macroset: metadata.macroset,
        title: copy_optional(
            handle,
            metadata,
            1,
            metadata.title,
            limits.max_content_bytes,
        )?,
        section: copy_optional(
            handle,
            metadata,
            2,
            metadata.section,
            limits.max_content_bytes,
        )?,
        volume: copy_optional(
            handle,
            metadata,
            4,
            metadata.volume,
            limits.max_content_bytes,
        )?,
        operating_system: copy_optional(
            handle,
            metadata,
            8,
            metadata.operating_system,
            limits.max_content_bytes,
        )?,
        architecture: copy_optional(
            handle,
            metadata,
            16,
            metadata.architecture,
            limits.max_content_bytes,
        )?,
        name: copy_optional(
            handle,
            metadata,
            32,
            metadata.name,
            limits.max_content_bytes,
        )?,
        date: copy_optional(
            handle,
            metadata,
            64,
            metadata.date,
            limits.max_content_bytes,
        )?,
        alias_target: copy_optional(
            handle,
            metadata,
            128,
            metadata.alias_target,
            limits.max_content_bytes,
        )?,
        has_body: metadata.has_body != 0,
    };
    let mut sources = reserve(source_views.len())?;
    for (index, source) in source_views.iter().enumerate() {
        if source.key != u32::try_from(index + 1).map_err(|_| invalid_result())?
            || source.format != metadata.macroset
            || source.reserved != 0
            || source.reserved_bytes != [0; 7]
            || source.hash_present > 1
        {
            return Err(invalid_result());
        }
        sources.push(AnnotatedSource {
            key: source.key,
            identity_kind: source.identity_kind,
            format: source.format,
            coordinate_kind: source.coordinate_kind,
            logical_name: copy_string(handle, source.logical_name, limits.max_source_path_bytes)?,
            decoded_length: source.decoded_length,
            hash: (source.hash_present != 0).then_some(source.hash),
        });
    }
    let mut spans = reserve(span_views.len())?;
    for span in span_views {
        if span.reserved != 0
            || span.reserved_bytes != [0; 2]
            || span.source == 0
            || span.source as usize > sources.len()
            || span.byte_range_present != 0
        {
            return Err(invalid_result());
        }
        spans.push(AnnotatedSpan {
            source: span.source,
            line_column: (span.line_column_present != 0).then_some((
                span.line_start,
                span.column_start,
                span.line_end,
                span.column_end,
            )),
        });
    }
    let mut provenances = reserve(provenance_views.len())?;
    for provenance in provenance_views {
        if provenance.reserved != 0
            || !(1..=3).contains(&provenance.kind)
            || provenance.authored_span as usize > spans.len()
            || provenance.generated_trigger_span as usize > spans.len()
        {
            return Err(invalid_result());
        }
        provenances.push(AnnotatedProvenance {
            kind: provenance.kind,
            authored_span: provenance.authored_span,
            generated_trigger_span: provenance.generated_trigger_span,
        });
    }
    let mut diagnostics = reserve(diagnostic_views.len())?;
    for diagnostic in diagnostic_views {
        if diagnostic.reserved != 0
            || diagnostic.owner != 0
            || diagnostic.span as usize > spans.len()
        {
            return Err(invalid_result());
        }
        diagnostics.push(AnnotatedDiagnostic {
            level: diagnostic.level,
            code: diagnostic.code,
            message: copy_string(handle, diagnostic.message, limits.max_content_bytes)?,
            span: diagnostic.span,
        });
    }
    // bind() supplies an empty mark view for body-only transfer, so the same
    // checked conversion runs only over annotations that remain actionable.
    let mut marks: Vec<AnnotatedMark> = reserve(mark_views.len())?;
    for (index, mark) in mark_views.iter().enumerate() {
        if mark.key != u32::try_from(index + 1).map_err(|_| invalid_result())?
            || !(1..=6).contains(&mark.kind)
            || mark.region_kind > 12
            || (mark.region_kind == 12 && mark.kind != 5)
            || mark.parent >= mark.key
            || mark.owner >= mark.key
            || mark.source as usize > sources.len()
            || (mark.line == 0) != (mark.column == 0)
            || (mark.source == 0 && mark.line != 0)
            || ((mark.flags & 1 != 0) != (mark.line != 0))
            || mark.point_reserved != 0
            || mark.flags & !ALLOWED_MARK_FLAGS != 0
            || (mark.flags & 512 != 0 && mark.kind != 2)
            || (mark.flags & 4 != 0 && mark.kind != 4)
            || (mark.flags & 8 != 0 && mark.kind != 1)
            || (mark.flags & 16 != 0 && mark.kind != 2)
            || (mark.flags & 1024 != 0
                && (mark.kind != 2
                    || !matches!(
                        mark.token,
                        crate::annotated::MAN_TP_TOKEN | crate::annotated::MAN_TQ_TOKEN
                    )
                    || mark.flags & (16 | 256) != (16 | 256)))
            || (mark.flags & HEAD_ROLE_MASK != 0
                && ((mark.kind != 2 && mark.kind != 6)
                    || mark.kind == 2 && mark.flags & 16 == 0
                    || (mark.flags & HEAD_ROLE_MASK).count_ones() != 1))
        {
            return Err(invalid_result());
        }
        if mark.kind == 2 && mark.flags & 512 != 0 {
            let lookup = |key: u32| {
                key.checked_sub(1)
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| mark_views.get(index))
                    .filter(|related| related.key == key)
                    .ok_or_else(invalid_result)
            };
            let parent = lookup(mark.parent)?;
            let title = lookup(mark.title_region)?;
            let body = lookup(mark.body_region)?;
            if mark.preceding_owner != 0
                || parent.kind != 5
                || parent.region_kind != 2
                || (mark.flags & 16 != 0) != (mark.flags & 256 != 0)
                || mark.flags & (HEAD_ROLE_MASK & !256) != 0
                || title.kind != 5
                || title.region_kind != 3
                || title.parent != mark.key
                || body.kind != 5
                || body.region_kind != 4
                || body.parent != mark.key
            {
                return Err(invalid_result());
            }
        }
        if mark.kind == 5 && mark.region_kind == 12 {
            let preceding = marks
                .get(
                    usize::try_from(
                        mark.preceding_owner
                            .checked_sub(1)
                            .ok_or_else(invalid_result)?,
                    )
                    .map_err(|_| invalid_result())?,
                )
                .ok_or_else(invalid_result)?;
            let parent = marks
                .get(
                    usize::try_from(mark.parent.checked_sub(1).ok_or_else(invalid_result)?)
                        .map_err(|_| invalid_result())?,
                )
                .ok_or_else(invalid_result)?;
            if mark.owner != 0
                || mark.preceding_owner >= mark.key
                || mark.title_region != 0
                || mark.body_region != 0
                || preceding.kind != 2
                || preceding.flags & (16 | 256 | 512) != (16 | 256 | 512)
                || preceding.parent != mark.parent
                || parent.kind != 5
                || parent.region_kind != 2
                || mark.token != crate::annotated::MAN_RS_TOKEN
                || [(preceding.title_region, 3), (preceding.body_region, 4)]
                    .into_iter()
                    .any(|(key, kind)| {
                        marks
                            .get(
                                key.checked_sub(1)
                                    .map_or(usize::MAX, |index| index as usize),
                            )
                            .is_none_or(|region: &AnnotatedMark| {
                                region.kind != 5
                                    || region.region_kind != kind
                                    || region.parent != preceding.key
                            })
                    })
            {
                return Err(invalid_result());
            }
        } else if mark.preceding_owner != 0 {
            let preceding = marks
                .get(usize::try_from(mark.preceding_owner - 1).map_err(|_| invalid_result())?)
                .ok_or_else(invalid_result)?;
            let reading_family = |token| {
                if token == crate::annotated::MAN_IP_TOKEN {
                    1
                } else if token == crate::annotated::MAN_TP_TOKEN
                    || token == crate::annotated::MAN_TQ_TOKEN
                {
                    2
                } else {
                    0
                }
            };
            let family = reading_family(mark.token);
            if mark.kind != 2
                || family == 0
                || mark.flags & (16 | 256) != (16 | 256)
                || mark.preceding_owner >= mark.key
                || preceding.kind != 2
                || reading_family(preceding.token) != family
                || preceding.flags & (16 | 256) != (16 | 256)
                || preceding.parent != mark.parent
                || preceding.owner != mark.owner
            {
                return Err(invalid_result());
            }
        }
        let point = match mark.point_kind {
            0 if mark.point_row == 0
                && mark.point_column == 0
                && mark.kind != 4
                && mark.kind != 5 =>
            {
                None
            }
            1 if (mark.kind == 2 || mark.kind == 4 || mark.kind == 5) && mark.point_row != 0 => {
                let row = rows
                    .get(usize::try_from(mark.point_row - 1).map_err(|_| invalid_result())?)
                    .ok_or_else(invalid_result)?;
                if mark.point_column > row.column_count {
                    return Err(invalid_result());
                }
                Some(AnnotatedDisplayPoint::RowColumn {
                    row: mark.point_row,
                    column: mark.point_column,
                })
            }
            2 if (mark.kind == 2 || mark.kind == 4 || mark.kind == 5)
                && mark.point_row == view.display.row_count
                && mark.point_column == 0 =>
            {
                Some(AnnotatedDisplayPoint::DocumentEnd {
                    row_count: mark.point_row,
                })
            }
            _ => return Err(invalid_result()),
        };
        let native_table_position = if mark.kind == 5 && mark.region_kind == 9 {
            if mark.table_position_present != 1
                || mark.parent == 0
                || mark.owner != mark.parent
                || mark.line != 0
                || mark.column != 0
                || mark.flags & 1 != 0
                || marks
                    .get(usize::try_from(mark.parent - 1).map_err(|_| invalid_result())?)
                    .is_none_or(|parent: &AnnotatedMark| {
                        parent.kind != 5 || parent.region_kind != 7
                    })
            {
                return Err(invalid_result());
            }
            Some((mark.table_column, mark.table_offset))
        } else {
            if mark.table_column != 0 || mark.table_position_present != 0 || mark.table_offset != 0
            {
                return Err(invalid_result());
            }
            None
        };
        if mark.kind == 5
            && mark.region_kind == 11
            && (mark.parent == 0
                || mark.owner != mark.parent
                || mark.source != 0
                || mark.line != 0
                || mark.column != 0
                || mark.token != 0
                || mark.flags != 0
                || mark.title_region != 0
                || mark.body_region != 0
                || marks
                    .get(usize::try_from(mark.parent - 1).map_err(|_| invalid_result())?)
                    .is_none_or(|parent: &AnnotatedMark| parent.kind != 5))
        {
            return Err(invalid_result());
        }
        if mark.kind == 6
            && (mark.parent == 0
                || mark.owner != mark.parent
                || mark.source == 0
                || mark.flags & HEAD_ROLE_MASK == 0
                || mark.flags & HEAD_VARIABLE != 0 && mark.token != MDOC_VA_TOKEN
                || mark.flags & HEAD_DEFINED_VARIABLE != 0 && mark.token != MDOC_DV_TOKEN
                || mark.flags & !(1 | HEAD_ROLE_MASK) != 0
                || mark.title_region != 0
                || mark.body_region != 0
                || mark.region_kind != 0
                || marks
                    .get(usize::try_from(mark.parent - 1).map_err(|_| invalid_result())?)
                    .is_none_or(|parent: &AnnotatedMark| {
                        parent.kind != 5 || parent.region_kind != 3
                    }))
        {
            return Err(invalid_result());
        }
        marks.push(AnnotatedMark {
            key: mark.key,
            kind: mark.kind,
            parent: mark.parent,
            owner: mark.owner,
            source: mark.source,
            line: mark.line,
            column: mark.column,
            token: mark.token,
            region_kind: mark.region_kind,
            title_region: mark.title_region,
            body_region: mark.body_region,
            flags: mark.flags,
            preceding_owner: mark.preceding_owner,
            selection_first: mark.selection_first,
            selection_count: mark.selection_count,
            point,
            native_table_position,
            name: if mark.kind == 4
                || mark.kind == 1 && mark.name_length != 0
                || mark.kind == 2 && mark.flags & (32 | 64 | 256) != 0 && mark.name_length != 0
            {
                let name = copy_string(
                    handle,
                    super::super::BytesView {
                        ptr: mark.name,
                        len: mark.name_length,
                    },
                    limits.max_content_bytes,
                )?;
                if name.is_empty() {
                    return Err(invalid_result());
                }
                Some(name)
            } else {
                if !mark.name.is_null() || mark.name_length != 0 {
                    return Err(invalid_result());
                }
                None
            },
            link_target: if mark.kind == 3 {
                if mark.target_kind > 5
                    || mark.target_b_present > 1
                    || (mark.target_b_present == 1) != (mark.target_kind == 4)
                {
                    return Err(invalid_result());
                }
                if mark.target_kind == 0 {
                    if !mark.target_a.ptr.is_null()
                        || mark.target_a.len != 0
                        || !mark.target_b.ptr.is_null()
                        || mark.target_b.len != 0
                    {
                        return Err(invalid_result());
                    }
                    None
                } else {
                    let primary = copy_string(handle, mark.target_a, limits.max_content_bytes)?;
                    if primary.is_empty() && mark.target_kind >= 3 {
                        return Err(invalid_result());
                    }
                    let secondary = if mark.target_b_present == 1 {
                        let value = copy_string(handle, mark.target_b, limits.max_content_bytes)?;
                        if value.is_empty() {
                            return Err(invalid_result());
                        }
                        Some(value)
                    } else {
                        if !mark.target_b.ptr.is_null() || mark.target_b.len != 0 {
                            return Err(invalid_result());
                        }
                        None
                    };
                    Some(AnnotatedLinkTarget {
                        kind: mark.target_kind,
                        primary,
                        secondary,
                    })
                }
            } else {
                if mark.target_kind != 0
                    || mark.target_b_present != 0
                    || !mark.target_a.ptr.is_null()
                    || mark.target_a.len != 0
                    || !mark.target_b.ptr.is_null()
                    || mark.target_b.len != 0
                {
                    return Err(invalid_result());
                }
                None
            },
        });
    }
    let coverage = if surface_only {
        AnnotationCoverage {
            checks: Vec::new(),
            issues: Vec::new(),
        }
    } else {
        transfer_coverage(coverage_check_views, coverage_issue_views, &marks, &sources)?
    };
    let mut owned_rows = reserve(rows.len())?;
    for (index, row) in rows.iter().enumerate() {
        if row.key != u32::try_from(index + 1).map_err(|_| invalid_result())? || row.break_after > 1
        {
            return Err(invalid_result());
        }
        owned_rows.push(AnnotatedRow {
            key: row.key,
            first_run: row.first_run,
            run_count: row.run_count,
            column_count: row.column_count,
            break_after: row.break_after != 0,
        });
    }
    let mut owned_runs = reserve(runs.len())?;
    for (index, run) in runs.iter().enumerate() {
        let end = run
            .byte_start
            .checked_add(run.byte_count)
            .ok_or_else(invalid_result)?;
        let start_usize = usize::try_from(run.byte_start).map_err(|_| invalid_result())?;
        let end_usize = usize::try_from(end).map_err(|_| invalid_result())?;
        if run.key != u32::try_from(index + 1).map_err(|_| invalid_result())?
            || run.reserved != 0
            || run.label.reserved != 0
            || run.label.glyph_origin != 0
            || run.label.flags != 0
            || run.label.style & !(1 | 8) != 0
            || end > view.display.byte_count
            || run.byte_count == 0
            || run.label.source as usize > sources.len()
            || (!surface_only && run.label.owner as usize > marks.len())
            || (!surface_only && run.label.link as usize > marks.len())
            || (!surface_only && run.label.head_component as usize > marks.len())
            || (!surface_only
                && run.label.head_component != 0
                && marks
                    .get(
                        usize::try_from(run.label.head_component - 1)
                            .map_err(|_| invalid_result())?,
                    )
                    .is_none_or(|mark| mark.kind != 6 || mark.owner != run.label.owner))
            || (run.label.role != 1 && run.label.role != 4 && run.label.role != 5)
            || !text_view.is_char_boundary(start_usize)
            || !text_view.is_char_boundary(end_usize)
            || (run.label.role == 5
                && (run.label.owner != 0
                    || run.label.link != 0
                    || run.label.head_component != 0
                    || run.label.source != 0
                    || run.label.style != 0
                    || !text_view[start_usize..end_usize]
                        .bytes()
                        .all(|byte| byte == b' ')))
        {
            return Err(invalid_result());
        }
        owned_runs.push(AnnotatedRun {
            key: run.key,
            column: run.column,
            width: run.width,
            byte_start: run.byte_start,
            byte_count: run.byte_count,
            label: AnnotatedLabel {
                owner: if surface_only { 0 } else { run.label.owner },
                link: if surface_only { 0 } else { run.label.link },
                source: run.label.source,
                head_component: if surface_only {
                    0
                } else {
                    run.label.head_component
                },
                style: run.label.style,
                role: run.label.role,
            },
        });
    }
    let mut selection_parts = reserve(if surface_only {
        0
    } else {
        selection_part_views.len()
    })?;
    if !surface_only {
        let mut selected = reserve::<u8>(runs.len())?;
        selected.resize(runs.len(), 0);
        let mut next_part = 0_usize;
        let mut next_join_text = 0_usize;
        for mark in &marks {
            let first = usize::try_from(mark.selection_first).map_err(|_| invalid_result())?;
            let count = usize::try_from(mark.selection_count).map_err(|_| invalid_result())?;
            let end = first.checked_add(count).ok_or_else(invalid_result)?;
            if first != next_part
                || end > selection_part_views.len()
                || (mark.kind == 4 && count != 0)
            {
                return Err(invalid_result());
            }
            let mut previous_run = 0_u32;
            for (index, part) in selection_part_views[first..end].iter().enumerate() {
                if part.run == 0 || part.run <= previous_run {
                    return Err(invalid_result());
                }
                let run_index = usize::try_from(part.run - 1).map_err(|_| invalid_result())?;
                let run = owned_runs.get(run_index).ok_or_else(invalid_result)?;
                let (bit, direct_key) = if mark.kind == 3 {
                    (2_u8, run.label.link)
                } else if mark.kind == 6 {
                    (4_u8, run.label.head_component)
                } else if mark.kind == 1 || mark.kind == 2 || mark.kind == 5 {
                    (1_u8, run.label.owner)
                } else {
                    return Err(invalid_result());
                };
                let join_before = match (index, part.join_before) {
                    (0, 0) => AnnotatedTextJoin::None,
                    (1.., 1) => AnnotatedTextJoin::DirectContact,
                    (1.., 2) => AnnotatedTextJoin::AuthoredSeparator,
                    (1.., 3) => AnnotatedTextJoin::HardBoundary,
                    (1.., 4) => AnnotatedTextJoin::Unknown,
                    (1.., 5) => AnnotatedTextJoin::GeneratedSeparator,
                    _ => return Err(invalid_result()),
                };
                if matches!(
                    join_before,
                    AnnotatedTextJoin::AuthoredSeparator | AnnotatedTextJoin::GeneratedSeparator
                ) {
                    let join_start =
                        usize::try_from(part.join_text_start).map_err(|_| invalid_result())?;
                    let join_len =
                        usize::try_from(part.join_text_len).map_err(|_| invalid_result())?;
                    let join_end = join_start
                        .checked_add(join_len)
                        .ok_or_else(invalid_result)?;
                    if join_len == 0
                        || join_start != next_join_text
                        || join_text_view
                            .get(join_start..join_end)
                            .is_none_or(|bytes| bytes.iter().any(|byte| *byte != b' '))
                    {
                        return Err(invalid_result());
                    }
                    next_join_text = join_end;
                } else if part.join_text_start != 0 || part.join_text_len != 0 {
                    return Err(invalid_result());
                }
                let start = run
                    .byte_start
                    .checked_add(part.start_byte)
                    .ok_or_else(invalid_result)?;
                let finish = run
                    .byte_start
                    .checked_add(part.end_byte)
                    .ok_or_else(invalid_result)?;
                if direct_key != mark.key
                    || part.start_byte != 0
                    || part.end_byte != run.byte_count
                    || start >= finish
                    || finish > view.display.byte_count
                    || !text_view
                        .is_char_boundary(usize::try_from(start).map_err(|_| invalid_result())?)
                    || !text_view
                        .is_char_boundary(usize::try_from(finish).map_err(|_| invalid_result())?)
                    || selected[run_index] & bit != 0
                {
                    return Err(invalid_result());
                }
                selected[run_index] |= bit;
                selection_parts.push(AnnotatedSelectionPart {
                    run: part.run,
                    start_byte: part.start_byte,
                    end_byte: part.end_byte,
                    join_before,
                    join_text_start: part.join_text_start,
                    join_text_len: part.join_text_len,
                });
                previous_run = part.run;
            }
            next_part = end;
        }
        if next_part != selection_part_views.len() || next_join_text != join_text_view.len() {
            return Err(invalid_result());
        }
        for (index, run) in owned_runs.iter().enumerate() {
            let expected = u8::from(run.label.owner != 0)
                | (u8::from(run.label.link != 0) << 1)
                | (u8::from(run.label.head_component != 0) << 2);
            if selected[index] != expected {
                return Err(invalid_result());
            }
        }
    }
    let mut text = String::new();
    text.try_reserve_exact(text_view.len())
        .map_err(|_| transfer_alloc())?;
    text.push_str(text_view);
    let mut join_text = String::new();
    if !surface_only {
        join_text
            .try_reserve_exact(join_text_view.len())
            .map_err(|_| transfer_alloc())?;
        // Every byte was checked against ASCII space while validating the
        // exact authored-separator partition above.
        join_text.push_str(std::str::from_utf8(join_text_view).map_err(|_| invalid_result())?);
    }
    Ok(AnnotatedDocument {
        root_source: view.root_source,
        profile: view.profile,
        width: view.width,
        annotation_degraded: view.annotation_degraded != 0 || surface_only,
        metadata,
        sources,
        spans,
        provenances,
        diagnostics,
        text,
        rows: owned_rows,
        runs: owned_runs,
        marks,
        selection_parts,
        join_text,
        coverage,
    })
}
