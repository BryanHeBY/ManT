//! Convert native formatter facts into width-independent IR layout policy.
//!
//! The pinned CVS handlers establish objective field origins and continuation
//! conditions.  This module is the sole owner of the policy mapping from those
//! facts to `ManT`'s responsive `DefinitionLayout`; native code deliberately has
//! no knowledge of `Stacked`, `RunIn`, or `Fit`.

use libmandoc_rs::{
    BoundaryEffect, BoundaryRequest, ExecutionBoundary, ExecutionDefinitionContract,
    ExecutionFlush, ExecutionManBlockKind, ExecutionMdocListKind, FlushOutcome,
};
use mant_ir::{DefinitionFitConstraint, DefinitionLayout, DefinitionPlacement, Inline};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) enum NativeDefinitionKind {
    Man(ExecutionManBlockKind),
    Mdoc(ExecutionMdocListKind),
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct ResponsiveDefinitionLayout {
    pub(super) layout: DefinitionLayout,
    /// Canonical native label origin, retained only to prove translation
    /// invariance.  It is never serialized into the relative IR layout.
    pub(super) label_origin_columns: i32,
    /// Native device wraps are fidelity observations, never hard IR breaks.
    pub(super) soft_flushes: Vec<u32>,
    /// Explicit control-request boundaries remain hard content boundaries.
    pub(super) hard_boundaries: Vec<u32>,
    /// Exact row-relative columns reached by literal tabs in the complete
    /// logical term field. Hard `\p` rows remain distinct; fixed-device soft
    /// wraps do not create epochs.
    pub(super) term_tab_fields: Vec<NativeTermTabField>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeTermTabField {
    /// Stable identity of the one complete native `term_flushln()` buffer.
    pub(super) buffer_generation: u32,
    pub(super) rows: Vec<NativeTermTabRow>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeTermTabRow {
    pub(super) row_epoch: u32,
    pub(super) destinations_columns: Vec<usize>,
}

pub(super) fn project_definition_layout(
    kind: NativeDefinitionKind,
    contract: ExecutionDefinitionContract,
    fit_constraint: Option<DefinitionFitConstraint>,
    head_flushes: impl Iterator<Item = ExecutionFlush>,
    body_flushes: impl Iterator<Item = ExecutionFlush>,
    head_boundaries: impl Iterator<Item = ExecutionBoundary>,
    body_boundaries: impl Iterator<Item = ExecutionBoundary>,
) -> ResponsiveDefinitionLayout {
    let (placement, gap) = match kind {
        NativeDefinitionKind::Man(
            ExecutionManBlockKind::IndentedParagraph
            | ExecutionManBlockKind::TaggedParagraph
            | ExecutionManBlockKind::AdditionalTag,
        )
        | NativeDefinitionKind::Mdoc(ExecutionMdocListKind::Tag) => {
            debug_assert!(contract.head_may_stay_open_if_field_fits);
            (DefinitionPlacement::Fit, contract.trailing_blank_cells)
        }
        NativeDefinitionKind::Man(_) => {
            unreachable!("non-definition man block reached responsive projection")
        }
        NativeDefinitionKind::Mdoc(ExecutionMdocListKind::Hang) => {
            debug_assert!(contract.head_stays_open_unconditionally);
            (DefinitionPlacement::RunIn, contract.trailing_blank_cells)
        }
        NativeDefinitionKind::Mdoc(ExecutionMdocListKind::Inset) => (DefinitionPlacement::RunIn, 1),
        NativeDefinitionKind::Mdoc(ExecutionMdocListKind::Diagnostic) => {
            (DefinitionPlacement::RunIn, 2)
        }
        NativeDefinitionKind::Mdoc(ExecutionMdocListKind::Overhang) => {
            (DefinitionPlacement::Stacked, 0)
        }
        NativeDefinitionKind::Mdoc(_) => {
            unreachable!("non-definition mdoc list reached responsive projection")
        }
    };
    let label_origin_columns = basic_units_to_columns(contract.head.offset_bu, contract.cell_bu);
    // `ascii_advance()` rounds each absolute destination independently.
    // Rounding the BU delta is not equivalent near half-cell boundaries.
    let body_indent_columns = basic_units_to_columns(contract.body.offset_bu, contract.cell_bu)
        .saturating_sub(label_origin_columns);
    let min_term_gap_columns = u16::try_from(gap).unwrap_or(u16::MAX);
    let term_continuation_indent_columns = if contract.wrapped_continuation_uses_field_end {
        body_indent_columns
    } else {
        0
    };
    let mut fit_constraint = match placement {
        DefinitionPlacement::Fit => fit_constraint,
        DefinitionPlacement::Stacked | DefinitionPlacement::RunIn => None,
    };
    let head_flushes = head_flushes.collect::<Vec<_>>();
    let final_head_field_sequence = head_flushes
        .iter()
        .rev()
        .find(|flush| flush.outcome == FlushOutcome::Exhausted)
        .map(|flush| flush.sequence);
    let term_tab_fields = head_flushes
        .iter()
        .filter(|flush| flush.outcome == FlushOutcome::Exhausted && !flush.logical_tabs.is_empty())
        .map(|flush| NativeTermTabField {
            buffer_generation: flush.buffer_generation,
            rows: project_term_tab_rows(flush, contract),
        })
        .collect();
    let soft_flushes = head_flushes
        .iter()
        .cloned()
        .chain(body_flushes)
        .filter(|flush| {
            matches!(
                flush.outcome,
                FlushOutcome::Wrapped | FlushOutcome::DeferredColumn
            )
        })
        .map(|flush| flush.key)
        .collect();
    let head_boundaries = head_boundaries.collect::<Vec<_>>();
    let head_has_hard_boundary = head_boundaries.iter().any(|boundary| {
        boundary_is_hard(boundary, contract, placement, true)
            && final_head_field_sequence.is_none_or(|sequence| boundary.leave_sequence > sequence)
    });
    if head_has_hard_boundary && let Some(constraint) = &mut fit_constraint {
        constraint.forced_separation = true;
    }
    let hard_boundaries = head_boundaries
        .into_iter()
        .chain(body_boundaries)
        .filter(|boundary| {
            let in_head = boundary.node == Some(contract.head.node);
            boundary_is_hard(boundary, contract, placement, in_head)
        })
        .map(|boundary| boundary.key)
        .collect();
    ResponsiveDefinitionLayout {
        layout: DefinitionLayout {
            placement,
            body_indent_columns,
            min_term_gap_columns,
            term_continuation_indent_columns,
            fit_constraint,
            spacing_before_lines: None,
        },
        label_origin_columns,
        soft_flushes,
        hard_boundaries,
        term_tab_fields,
    }
}

fn project_term_tab_rows(
    flush: &ExecutionFlush,
    contract: ExecutionDefinitionContract,
) -> Vec<NativeTermTabRow> {
    const TERMP_BRIND: u32 = 1 << 10;

    let mut rows = Vec::<NativeTermTabRow>::new();
    for tab in &flush.logical_tabs {
        // Pinned `term_flushln()` starts the first logical row at the measured
        // field origin. After a realized `\p`, BRIND starts the next row at
        // rmargin; other flows restart at offset. Round absolute endpoints
        // independently so a fractional row origin cannot shift the relative
        // tab destination by a cell.
        let origin = if tab.row_epoch == 0 {
            flush.logical_origin_bu
        } else if flush.flags_before & TERMP_BRIND != 0 {
            flush.rmargin_bu
        } else {
            flush.offset_bu
        };
        let destination =
            basic_units_to_columns(origin.saturating_add(tab.destination_bu), contract.cell_bu)
                .saturating_sub(basic_units_to_columns(origin, contract.cell_bu));
        let destination = usize::try_from(destination.max(0)).unwrap_or(usize::MAX);
        if let Some(row) = rows.last_mut().filter(|row| row.row_epoch == tab.row_epoch) {
            row.destinations_columns.push(destination);
        } else {
            rows.push(NativeTermTabRow {
                row_epoch: tab.row_epoch,
                destinations_columns: vec![destination],
            });
        }
    }
    rows
}

fn boundary_is_hard(
    boundary: &ExecutionBoundary,
    contract: ExecutionDefinitionContract,
    placement: DefinitionPlacement,
    in_head: bool,
) -> bool {
    const TERMP_NOBREAK: u32 = 1 << 8;

    if matches!(boundary.request, BoundaryRequest::DeviceEndline)
        || !matches!(
            boundary.effect,
            BoundaryEffect::EndedLine | BoundaryEffect::AddedVerticalSpace
        )
    {
        return false;
    }
    let lifecycle_owner = boundary.control.is_none()
        && matches!(boundary.node, Some(node)
            if node == contract.head.node || node == contract.body.node);
    if !lifecycle_owner {
        return true;
    }

    // Pinned CVS `roff_term_pre_mc()` can flush and then clear NOBREAK while
    // a definition HEAD remains open.  The later `termp_it_post()` newline is
    // then a real structural separation, not the ordinary conditional field
    // finalization that responsive placement may replace.
    placement == DefinitionPlacement::Fit && in_head && boundary.flags_before & TERMP_NOBREAK == 0
}

/// Match the fixed character device's half-cell rule without hard-coding its
/// current 24-BU scale.  The scale arrives with the native execution facts.
fn basic_units_to_columns(value: i64, cell_bu: i64) -> i32 {
    debug_assert!(cell_bu > 0);
    let half_down = (cell_bu - 1) / 2;
    let columns = if value < 0 {
        -value.saturating_neg().saturating_add(half_down) / cell_bu
    } else {
        value.saturating_add(half_down) / cell_bu
    };
    i32::try_from(columns).unwrap_or(if columns < 0 { i32::MIN } else { i32::MAX })
}

/// Materialize native logical-row tab destinations without leaking formatter
/// tab state into source-neutral IR or its readers.
pub(super) fn project_term_tabs(term: &[Inline], rows: &[NativeTermTabRow]) -> Vec<Inline> {
    if rows.is_empty() {
        return term.to_vec();
    }
    let tab_counts = tab_counts_by_row(term);
    if tab_counts.len() != rows.len()
        || tab_counts.iter().zip(rows).any(|((epoch, count), row)| {
            *epoch != row.row_epoch || *count != row.destinations_columns.len()
        })
    {
        return term.to_vec();
    }
    let mut projected = term.to_vec();
    let mut state = TabProjection {
        rows,
        row: 0,
        native_row: 0,
        next: 0,
        base_column: 0,
        pending_text: String::new(),
    };
    project_tabs_inlines(&mut projected, &mut state);
    if state.native_row == rows.len() {
        projected
    } else {
        term.to_vec()
    }
}

fn tab_counts_by_row(inlines: &[Inline]) -> Vec<(u32, usize)> {
    fn collect(inlines: &[Inline], row: &mut u32, counts: &mut Vec<(u32, usize)>) {
        for inline in inlines {
            match inline {
                Inline::Text { value } | Inline::Code { value } => {
                    for character in value.chars() {
                        if character == '\t' {
                            if let Some((_, count)) =
                                counts.last_mut().filter(|(epoch, _)| *epoch == *row)
                            {
                                *count += 1;
                            } else {
                                counts.push((*row, 1));
                            }
                        } else if character == '\n' {
                            *row = row.saturating_add(1);
                        }
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => collect(children, row, counts),
                Inline::LineBreak => *row = row.saturating_add(1),
                Inline::Anchor { .. } => {}
            }
        }
    }

    let mut row = 0;
    let mut counts = Vec::new();
    collect(inlines, &mut row, &mut counts);
    counts
}

struct TabProjection<'a> {
    rows: &'a [NativeTermTabRow],
    row: u32,
    native_row: usize,
    next: usize,
    base_column: usize,
    pending_text: String,
}

fn project_tabs_inlines(inlines: &mut [Inline], state: &mut TabProjection<'_>) {
    for inline in inlines {
        match inline {
            Inline::Text { value } | Inline::Code { value } => {
                let mut projected = String::with_capacity(value.len());
                for character in value.chars() {
                    match character {
                        '\t' => {
                            let Some(row) = state.rows.get(state.native_row) else {
                                return;
                            };
                            if row.row_epoch != state.row {
                                return;
                            }
                            let Some(destination) =
                                row.destinations_columns.get(state.next).copied()
                            else {
                                return;
                            };
                            let column = state
                                .base_column
                                .saturating_add(mant_ir::geometry::text_width(&state.pending_text));
                            projected.push_str(&" ".repeat(destination.saturating_sub(column)));
                            state.base_column = destination.max(column);
                            state.pending_text.clear();
                            state.next += 1;
                            if state.next == row.destinations_columns.len() {
                                state.native_row += 1;
                                state.next = 0;
                            }
                        }
                        '\n' => {
                            projected.push(character);
                            state.row = state.row.saturating_add(1);
                            state.base_column = 0;
                            state.pending_text.clear();
                        }
                        _ => {
                            projected.push(character);
                            state.pending_text.push(character);
                        }
                    }
                }
                *value = projected;
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => project_tabs_inlines(children, state),
            Inline::LineBreak => {
                state.row = state.row.saturating_add(1);
                state.base_column = 0;
                state.pending_text.clear();
            }
            Inline::Anchor { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::basic_units_to_columns;

    #[test]
    fn converts_native_basic_units_with_the_reported_cell_scale() {
        assert_eq!(basic_units_to_columns(11, 24), 0);
        assert_eq!(basic_units_to_columns(12, 24), 0);
        assert_eq!(basic_units_to_columns(13, 24), 1);
        assert_eq!(basic_units_to_columns(36, 24), 1);
        assert_eq!(basic_units_to_columns(37, 24), 2);
        assert_eq!(basic_units_to_columns(-13, 24), -1);
        assert_eq!(basic_units_to_columns(26, 16), 2);
    }
}
