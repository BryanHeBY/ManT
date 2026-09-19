//! First private consumer of the owned native execution report.
//!
//! This is deliberately not the production switch. It establishes the
//! execution-to-projection boundary used by the staged migration without
//! reconstructing formatter state in Rust.

use libmandoc_rs::{
    AtomDisposition, AtomKind, AtomRole, BoundaryEffect, Document as NativeDocument,
    ExecutionAffinity, ExecutionBoundary, ExecutionControl, ExecutionFlush, ExecutionFont,
    ExecutionFragment, ExecutionHeadingKind, ExecutionManBlockKind, ExecutionMdocListKind,
    ExecutionNodeKey, ExecutionReferenceKind, ExecutionRegionKind, ExecutionTableAlignment,
    ExecutionTableCell, ExecutionTableCellFlags, ExecutionTableCellKey, ExecutionTableDataKind,
    ExecutionTableKey, ExecutionTableLayoutKind, ExecutionTableRow, ExecutionTableRowKey,
    ExecutionWrapperKind, FragmentRole, GeometryKind, GeometryOriginKind, NativeExecutionReport,
    Node as NativeNode, NodeKind, NormalizedListKind, TableAlignment as NativeTableAlignment,
    TableCellKind as NativeTableCellKind, TableRowKind as NativeTableRowKind,
    TableRuleCellKind as NativeTableRuleCellKind,
};
use std::{collections::BTreeMap, ops::Range, path::PathBuf};

mod layout;
mod semantics;

use layout::{
    NativeDefinitionKind, ResponsiveDefinitionLayout, project_definition_layout, project_term_tabs,
};

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NativeTextRun {
    pub(super) node: ExecutionNodeKey,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) font: ExecutionFont,
    /// Native provenance shared by all visible atoms in this run.
    pub(super) role: AtomRole,
    /// Exact formatter word that created these atoms, when one exists.
    pub(super) word: Option<u32>,
    /// Structural wrapper active when the atoms entered the native buffer.
    pub(super) wrapper: Option<u32>,
    /// Atom identities retained across delayed and partial native flushes.
    pub(super) atoms: Vec<u32>,
    pub(super) text: String,
    pub(super) device_line: u32,
    pub(super) start_bu: i64,
    pub(super) end_bu: i64,
    pub(super) reference: Option<u32>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeHeadingKind {
    ManSection,
    ManSubsection,
    MdocSection,
    MdocSubsection,
}

/// One section heading projected from the exact native node-execution scope.
///
/// The parser-owned tag remains independent from `display_lines`: fixed CVS
/// derives same-document identities from authored syntax, while terminal
/// execution may remove spacing, overstrike glyphs, or split author names.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeHeadingFact {
    pub(super) kind: NativeHeadingKind,
    pub(super) section: ExecutionNodeKey,
    pub(super) head: ExecutionNodeKey,
    pub(super) body: ExecutionNodeKey,
    pub(super) authored_phrase: Option<String>,
    pub(super) authored_fragment: Option<String>,
    pub(super) atoms: Range<u32>,
    pub(super) boundaries: Vec<ExecutionBoundary>,
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) display_lines: Vec<String>,
    pub(super) label: String,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeAnchor {
    pub(super) key: u32,
    pub(super) node: ExecutionNodeKey,
    pub(super) target: Vec<u8>,
    pub(super) device_line: u32,
    pub(super) atom_cursor: u32,
    pub(super) fragment_cursor: u32,
    pub(super) affinity: ExecutionAffinity,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeReference {
    pub(super) key: u32,
    pub(super) parent: Option<u32>,
    pub(super) owner_node: ExecutionNodeKey,
    pub(super) target_node: ExecutionNodeKey,
    pub(super) kind: ExecutionReferenceKind,
    pub(super) primary: Vec<u8>,
    pub(super) secondary: Option<Vec<u8>>,
    pub(super) execution_atoms: Range<u32>,
    pub(super) atoms: Range<u32>,
    pub(super) affinity: ExecutionAffinity,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeOrigin {
    pub(super) key: ExecutionNodeKey,
    pub(super) parent: Option<ExecutionNodeKey>,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) kind: NodeKind,
    pub(super) macro_name: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeWordFact {
    pub(super) key: u32,
    pub(super) node: Option<ExecutionNodeKey>,
    pub(super) source: PathBuf,
    pub(super) operand: Vec<u8>,
    pub(super) role: AtomRole,
    pub(super) wrapper: Option<u32>,
    pub(super) atoms: Range<u32>,
    pub(super) enter_sequence: u64,
    pub(super) leave_sequence: u64,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeDefinitionFact {
    pub(super) owner: ExecutionNodeKey,
    pub(super) macro_name: String,
    pub(super) head: ExecutionNodeKey,
    pub(super) body: ExecutionNodeKey,
    pub(super) kind: NativeDefinitionKind,
    pub(super) responsive: ResponsiveDefinitionLayout,
    pub(super) head_flushes: Vec<ExecutionFlush>,
    pub(super) body_flushes: Vec<ExecutionFlush>,
}

impl NativeDefinitionFact {
    /// Apply native final-row tab destinations when this fact is materialized
    /// by the production native backend in K23.  The caller must supply the
    /// exact native logical-field identity retained while materializing its
    /// atoms; public IR terms intentionally do not expose this provenance.
    #[allow(dead_code)]
    pub(super) fn project_field_term(
        &self,
        buffer_generation: u32,
        term: &[mant_ir::Inline],
    ) -> Vec<mant_ir::Inline> {
        self.responsive
            .term_tab_fields
            .iter()
            .find(|field| field.buffer_generation == buffer_generation)
            .map_or_else(
                || term.to_vec(),
                |field| project_term_tabs(term, &field.rows),
            )
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeMdocListRole {
    Head,
    Body,
}

/// One native head/body execution interval inside an mdoc `.It` lifecycle.
///
/// A column item may own multiple body segments.  Keeping each native body
/// distinct avoids recovering column ownership from canonical device lines.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeMdocListSegment {
    pub(super) node: ExecutionNodeKey,
    pub(super) role: NativeMdocListRole,
    pub(super) ordinal: u32,
    pub(super) atoms: Range<u32>,
    pub(super) flushes: Vec<ExecutionFlush>,
    pub(super) boundaries: Vec<ExecutionBoundary>,
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) anchors: Vec<u32>,
}

/// One exact fixed-CVS `.It` pre/children/post execution lifecycle.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeMdocListItem {
    pub(super) owner: ExecutionNodeKey,
    pub(super) flow_epoch: usize,
    pub(super) list: ExecutionNodeKey,
    pub(super) wrapper: u32,
    pub(super) kind: ExecutionMdocListKind,
    pub(super) compact: bool,
    pub(super) state_before: u32,
    pub(super) state_after: u32,
    pub(super) atoms: Range<u32>,
    pub(super) head: NativeMdocListSegment,
    pub(super) bodies: Vec<NativeMdocListSegment>,
    pub(super) flushes: Vec<ExecutionFlush>,
    pub(super) boundaries: Vec<ExecutionBoundary>,
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) anchors: Vec<u32>,
}

/// Items grouped by their parser-owned mdoc `Bl` block.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeMdocList {
    pub(super) owner: ExecutionNodeKey,
    pub(super) kind: ExecutionMdocListKind,
    pub(super) compact: bool,
    pub(super) items: Vec<NativeMdocListItem>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeManBlockRole {
    Head,
    Body,
}

/// One parser-owned HEAD or BODY interval within a native man block.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeManBlockSegment {
    pub(super) node: ExecutionNodeKey,
    pub(super) role: NativeManBlockRole,
    pub(super) atoms: Range<u32>,
    pub(super) flushes: Vec<ExecutionFlush>,
    pub(super) boundaries: Vec<ExecutionBoundary>,
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) anchors: Vec<u32>,
}

/// Exact fixed-CVS pre/children/post lifecycle for one man(7) block.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeManBlock {
    pub(super) owner: ExecutionNodeKey,
    pub(super) flow_epoch: usize,
    pub(super) wrapper: u32,
    pub(super) kind: ExecutionManBlockKind,
    pub(super) state_before: u32,
    pub(super) state_after: u32,
    pub(super) atoms: Range<u32>,
    pub(super) head: NativeManBlockSegment,
    pub(super) body: NativeManBlockSegment,
    pub(super) flushes: Vec<ExecutionFlush>,
    pub(super) boundaries: Vec<ExecutionBoundary>,
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) anchors: Vec<u32>,
}

/// One exact display, synopsis, literal, or captured-control lifecycle from
/// the fixed-CVS terminal executor.
///
/// Region ownership is projected from the native wrapper stack.  This keeps
/// captured control operands, emitted runs, flushes, and anchors in one
/// transaction instead of reconstructing their relationship from source
/// coordinates after execution.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeRegion {
    pub(super) owner: ExecutionNodeKey,
    pub(super) wrapper: u32,
    pub(super) kind: ExecutionRegionKind,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) state_before: u32,
    pub(super) state_after: u32,
    pub(super) atoms: Range<u32>,
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) flushes: Vec<ExecutionFlush>,
    pub(super) boundaries: Vec<ExecutionBoundary>,
    pub(super) controls: Vec<ExecutionControl>,
    pub(super) anchors: Vec<u32>,
}

/// One native `term_fill()`/`term_field()` decision together with the atom
/// fates that make its accepted and discarded ranges observable to a
/// projection consumer.
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeFieldFact {
    pub(super) flush: ExecutionFlush,
    pub(super) emitted_atoms: Vec<u32>,
    pub(super) consumed_atoms: Vec<u32>,
    pub(super) replaced_atoms: Vec<u32>,
    pub(super) trailing_discarded_atoms: Vec<u32>,
}

/// One control request paired with its source provenance, exact native state
/// transition, and directly owned primitive boundary effects. This remains private until the staged
/// projection can replace the legacy Rust execution model atomically.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeControlProvenance {
    Authored,
    Generated,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeControlFact {
    pub(super) control: ExecutionControl,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) macro_name: String,
    pub(super) provenance: NativeControlProvenance,
    pub(super) boundaries: Vec<ExecutionBoundary>,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeTableCell {
    pub(super) key: ExecutionTableCellKey,
    pub(super) node: ExecutionNodeKey,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) ordinal: u32,
    pub(super) data_ordinal: u32,
    pub(super) logical_column: u32,
    pub(super) column_span: u32,
    pub(super) row_span: u32,
    pub(super) layout_kind: ExecutionTableLayoutKind,
    pub(super) data_kind: ExecutionTableDataKind,
    pub(super) alignment: ExecutionTableAlignment,
    pub(super) font: ExecutionFont,
    pub(super) flags: ExecutionTableCellFlags,
    pub(super) buffer_generation: Option<u32>,
    pub(super) atoms: Range<u32>,
    pub(super) fragments: Vec<ExecutionFragment>,
    pub(super) flushes: Vec<ExecutionFlush>,
    pub(super) content: Vec<mant_ir::Inline>,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeTableRow {
    pub(super) key: ExecutionTableRowKey,
    pub(super) node: ExecutionNodeKey,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) kind: mant_ir::TableRowKind,
    pub(super) logical_columns: u32,
    pub(super) cells: Vec<NativeTableCell>,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeTable {
    pub(super) key: ExecutionTableKey,
    pub(super) source: PathBuf,
    pub(super) logical_columns: u32,
    pub(super) rows: Vec<NativeTableRow>,
    pub(super) block: mant_ir::Block,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeProjection {
    pub(super) origins: Vec<NativeOrigin>,
    pub(super) words: Vec<NativeWordFact>,
    pub(super) runs: Vec<NativeTextRun>,
    pub(super) visible_lines: Vec<String>,
    pub(super) implicit_spaces: usize,
    pub(super) hard_boundaries: usize,
    pub(super) glyph_geometries: usize,
    pub(super) references: Vec<NativeReference>,
    pub(super) anchors: Vec<NativeAnchor>,
    pub(super) definitions: Vec<NativeDefinitionFact>,
    pub(super) man_blocks: Vec<NativeManBlock>,
    pub(super) mdoc_lists: Vec<NativeMdocList>,
    pub(super) fields: Vec<NativeFieldFact>,
    pub(super) controls: Vec<NativeControlFact>,
    pub(super) headings: Vec<NativeHeadingFact>,
    pub(super) regions: Vec<NativeRegion>,
    pub(super) tables: Vec<NativeTable>,
}

fn partition_control_boundaries<'a>(
    control_count: usize,
    boundaries: impl IntoIterator<Item = &'a ExecutionBoundary>,
) -> Vec<Vec<ExecutionBoundary>> {
    let mut direct_boundaries = vec![Vec::new(); control_count];
    for boundary in boundaries {
        if let Some(control) = boundary.control {
            direct_boundaries[control as usize].push(boundary.clone());
        }
    }
    direct_boundaries
}

fn control_facts(report: &NativeExecutionReport) -> Vec<NativeControlFact> {
    let direct_boundaries =
        partition_control_boundaries(report.controls().len(), report.boundaries());

    report
        .controls()
        .iter()
        .zip(direct_boundaries)
        .map(|(control, boundaries)| {
            let origin = &report.nodes()[control.node.0 as usize];
            NativeControlFact {
                control: control.clone(),
                source: report.sources()[origin.source as usize].path.clone(),
                line: origin.line,
                column: origin.column,
                macro_name: origin
                    .macro_name
                    .clone()
                    .expect("validated native control macro"),
                provenance: if origin
                    .flags
                    .contains(libmandoc_rs::ExecutionNodeFlags::GENERATED)
                {
                    NativeControlProvenance::Generated
                } else {
                    NativeControlProvenance::Authored
                },
                boundaries,
            }
        })
        .collect()
}

fn field_facts(report: &NativeExecutionReport) -> Vec<NativeFieldFact> {
    let mut atoms_by_generation =
        vec![BTreeMap::<u32, Vec<u32>>::new(); report.buffer_generations().len()];
    for atom in report.atoms() {
        let (Some(generation), Some(slot)) = (atom.buffer_generation, atom.slot) else {
            continue;
        };
        atoms_by_generation[generation as usize]
            .entry(slot)
            .or_default()
            .push(atom.key.0);
    }

    report
        .flushes()
        .iter()
        .map(|flush| {
            let atoms = &atoms_by_generation[flush.buffer_generation as usize];
            let mut emitted_atoms = Vec::new();
            let mut consumed_atoms = Vec::new();
            let mut replaced_atoms = Vec::new();
            let mut trailing_discarded_atoms = Vec::new();
            for range in [flush.accepted.clone(), flush.tail_discarded.clone()] {
                for keys in atoms.range(range).map(|(_, keys)| keys) {
                    let occupant = keys
                        .iter()
                        .rposition(|key| {
                            report.atoms()[*key as usize].disposition != AtomDisposition::Replaced
                        })
                        .expect("validated native field slot has a current occupant");
                    let mut first = occupant;
                    while first > 0
                        && report.atoms()[keys[first - 1] as usize].replaced_by
                            == Some(libmandoc_rs::AtomKey(keys[first]))
                    {
                        first -= 1;
                    }
                    for &key in &keys[first..=occupant] {
                        match report.atoms()[key as usize].disposition {
                            AtomDisposition::Emitted => emitted_atoms.push(key),
                            AtomDisposition::Consumed => consumed_atoms.push(key),
                            AtomDisposition::Replaced => replaced_atoms.push(key),
                            AtomDisposition::TrailingDiscard => trailing_discarded_atoms.push(key),
                            AtomDisposition::Buffered => {
                                debug_assert!(
                                    false,
                                    "sealed native field retained a buffered atom"
                                );
                            }
                        }
                    }
                }
            }
            NativeFieldFact {
                flush: flush.clone(),
                emitted_atoms,
                consumed_atoms,
                replaced_atoms,
                trailing_discarded_atoms,
            }
        })
        .collect()
}

fn definition_fit_constraint(
    contract: libmandoc_rs::ExecutionDefinitionContract,
    head_flushes: &[ExecutionFlush],
) -> Option<mant_ir::DefinitionFitConstraint> {
    if !contract.head_may_stay_open_if_field_fits {
        return None;
    }
    let flush = head_flushes
        .iter()
        .rev()
        .find(|flush| flush.outcome == libmandoc_rs::FlushOutcome::Exhausted)?;
    Some(mant_ir::DefinitionFitConstraint {
        fit_content_basic_units: u64::try_from(flush.logical_fit_content_bu)
            .expect("validated native logical fit width"),
        field_basic_units: u64::try_from(flush.logical_field_bu)
            .expect("validated native logical field capacity"),
        origin_phase_basic_units: u64::try_from(
            flush.logical_origin_bu.rem_euclid(contract.cell_bu),
        )
        .expect("validated native origin phase"),
        cell_basic_units: std::num::NonZeroU64::new(
            u64::try_from(contract.cell_bu).expect("validated native cell width"),
        )
        .expect("positive validated native cell width"),
        forced_separation: flush.logical_forced_break,
    })
}

fn definition_facts(
    report: &NativeExecutionReport,
    man_blocks: &[NativeManBlock],
    mdoc_lists: &[NativeMdocList],
) -> Vec<NativeDefinitionFact> {
    let mut facts = man_definition_facts(report, man_blocks);
    facts.extend(mdoc_definition_facts(report, mdoc_lists));
    facts.sort_by_key(|definition| definition.owner);
    facts
}

fn is_man_definition_kind(kind: ExecutionManBlockKind) -> bool {
    matches!(
        kind,
        ExecutionManBlockKind::IndentedParagraph
            | ExecutionManBlockKind::TaggedParagraph
            | ExecutionManBlockKind::AdditionalTag
    )
}

// Keep collection, closest-wrapper ownership, and projection together so the
// HEAD/BODY lifecycle remains visibly symmetric with the native handler.
#[allow(clippy::too_many_lines)]
fn man_definition_facts(
    report: &NativeExecutionReport,
    man_blocks: &[NativeManBlock],
) -> Vec<NativeDefinitionFact> {
    struct Pending<'a> {
        block: &'a NativeManBlock,
        head_flushes: Vec<ExecutionFlush>,
        body_flushes: Vec<ExecutionFlush>,
        head_boundaries: Vec<ExecutionBoundary>,
        body_boundaries: Vec<ExecutionBoundary>,
    }
    let mut pending = man_blocks
        .iter()
        .filter(|block| is_man_definition_kind(block.kind))
        .map(|block| Pending {
            block,
            head_flushes: Vec::new(),
            body_flushes: Vec::new(),
            head_boundaries: Vec::new(),
            body_boundaries: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut wrapper_depths = Vec::with_capacity(report.wrappers().len());
    for wrapper in report.wrappers() {
        wrapper_depths.push(
            wrapper
                .parent
                .map_or(0, |parent| wrapper_depths[parent as usize] + 1),
        );
    }
    let mut owners = vec![None::<(u32, usize, bool)>; report.flushes().len()];
    for (definition, value) in pending.iter().enumerate() {
        let depth = wrapper_depths[value.block.wrapper as usize];
        for (is_head, flushes) in [
            (true, &value.block.head.flushes),
            (false, &value.block.body.flushes),
        ] {
            for flush in flushes {
                let owner = &mut owners[flush.key as usize];
                if owner.is_none_or(|current| current.0 < depth) {
                    *owner = Some((depth, definition, is_head));
                }
            }
        }
    }
    for (flush, owner) in report.flushes().iter().zip(owners) {
        let Some((_, definition, is_head)) = owner else {
            continue;
        };
        if is_head {
            pending[definition].head_flushes.push(flush.clone());
        } else {
            pending[definition].body_flushes.push(flush.clone());
        }
    }
    let mut owners = vec![None::<(u32, usize, bool)>; report.boundaries().len()];
    for (definition, value) in pending.iter().enumerate() {
        let depth = wrapper_depths[value.block.wrapper as usize];
        for (is_head, boundaries) in [
            (true, &value.block.head.boundaries),
            (false, &value.block.body.boundaries),
        ] {
            for boundary in boundaries {
                let owner = &mut owners[boundary.key as usize];
                if owner.is_none_or(|current| current.0 < depth) {
                    *owner = Some((depth, definition, is_head));
                }
            }
        }
    }
    for (boundary, owner) in report.boundaries().iter().zip(owners) {
        let Some((_, definition, is_head)) = owner else {
            continue;
        };
        if is_head {
            pending[definition].head_boundaries.push(boundary.clone());
        } else {
            pending[definition].body_boundaries.push(boundary.clone());
        }
    }
    pending
        .into_iter()
        .map(|value| {
            let contract = report.wrappers()[value.block.wrapper as usize]
                .definition
                .expect("validated native man definition contract");
            let kind = NativeDefinitionKind::Man(value.block.kind);
            let fit_constraint = definition_fit_constraint(contract, &value.head_flushes);
            let responsive = project_definition_layout(
                kind,
                contract,
                fit_constraint,
                value.head_flushes.iter().cloned(),
                value.body_flushes.iter().cloned(),
                value.head_boundaries.iter().cloned(),
                value.body_boundaries.iter().cloned(),
            );
            NativeDefinitionFact {
                owner: value.block.owner,
                macro_name: report.nodes()[value.block.owner.0 as usize]
                    .macro_name
                    .clone()
                    .expect("typed man block macro"),
                head: value.block.head.node,
                body: value.block.body.node,
                kind,
                responsive,
                head_flushes: value.head_flushes,
                body_flushes: value.body_flushes,
            }
        })
        .collect()
}

fn is_mdoc_definition_kind(kind: ExecutionMdocListKind) -> bool {
    matches!(
        kind,
        ExecutionMdocListKind::Hang
            | ExecutionMdocListKind::Overhang
            | ExecutionMdocListKind::Inset
            | ExecutionMdocListKind::Diagnostic
            | ExecutionMdocListKind::Tag
    )
}

// Keep collection, closest-wrapper ownership, and projection together so the
// HEAD/BODY lifecycle remains visibly symmetric with the native handler.
#[allow(clippy::too_many_lines)]
fn mdoc_definition_facts(
    report: &NativeExecutionReport,
    mdoc_lists: &[NativeMdocList],
) -> Vec<NativeDefinitionFact> {
    struct Pending<'a> {
        item: &'a NativeMdocListItem,
        head_flushes: Vec<ExecutionFlush>,
        body_flushes: Vec<ExecutionFlush>,
        head_boundaries: Vec<ExecutionBoundary>,
        body_boundaries: Vec<ExecutionBoundary>,
    }

    let mut pending = mdoc_lists
        .iter()
        .filter(|list| is_mdoc_definition_kind(list.kind))
        .flat_map(|list| &list.items)
        .map(|item| Pending {
            item,
            head_flushes: Vec::new(),
            body_flushes: Vec::new(),
            head_boundaries: Vec::new(),
            body_boundaries: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut wrapper_depths = Vec::with_capacity(report.wrappers().len());
    for wrapper in report.wrappers() {
        let depth = wrapper
            .parent
            .map_or(0, |parent| wrapper_depths[parent as usize] + 1);
        wrapper_depths.push(depth);
    }
    let mut owners = vec![None::<(u32, usize, bool)>; report.flushes().len()];
    for (definition, value) in pending.iter().enumerate() {
        let depth = wrapper_depths[value.item.wrapper as usize];
        for flush in &value.item.head.flushes {
            let owner = &mut owners[flush.key as usize];
            if owner.is_none_or(|current| current.0 < depth) {
                *owner = Some((depth, definition, true));
            }
        }
        for body in &value.item.bodies {
            for flush in &body.flushes {
                let owner = &mut owners[flush.key as usize];
                if owner.is_none_or(|current| current.0 < depth) {
                    *owner = Some((depth, definition, false));
                }
            }
        }
    }
    for (flush, owner) in report.flushes().iter().zip(owners) {
        let Some((_, definition, is_head)) = owner else {
            continue;
        };
        if is_head {
            pending[definition].head_flushes.push(flush.clone());
        } else {
            pending[definition].body_flushes.push(flush.clone());
        }
    }
    let mut owners = vec![None::<(u32, usize, bool)>; report.boundaries().len()];
    for (definition, value) in pending.iter().enumerate() {
        let depth = wrapper_depths[value.item.wrapper as usize];
        for boundary in &value.item.head.boundaries {
            let owner = &mut owners[boundary.key as usize];
            if owner.is_none_or(|current| current.0 < depth) {
                *owner = Some((depth, definition, true));
            }
        }
        for body in &value.item.bodies {
            for boundary in &body.boundaries {
                let owner = &mut owners[boundary.key as usize];
                if owner.is_none_or(|current| current.0 < depth) {
                    *owner = Some((depth, definition, false));
                }
            }
        }
    }
    for (boundary, owner) in report.boundaries().iter().zip(owners) {
        let Some((_, definition, is_head)) = owner else {
            continue;
        };
        if is_head {
            pending[definition].head_boundaries.push(boundary.clone());
        } else {
            pending[definition].body_boundaries.push(boundary.clone());
        }
    }
    pending
        .into_iter()
        .filter_map(|value| {
            let contract = report.wrappers()[value.item.wrapper as usize]
                .definition
                .expect("validated native mdoc definition contract");
            let kind = NativeDefinitionKind::Mdoc(value.item.kind);
            let fit_constraint = definition_fit_constraint(contract, &value.head_flushes);
            let responsive = project_definition_layout(
                kind,
                contract,
                fit_constraint,
                value.head_flushes.iter().cloned(),
                value.body_flushes.iter().cloned(),
                value.head_boundaries.iter().cloned(),
                value.body_boundaries.iter().cloned(),
            );
            Some(NativeDefinitionFact {
                owner: value.item.owner,
                macro_name: "It".to_owned(),
                head: value.item.head.node,
                body: value.item.bodies.first()?.node,
                kind,
                responsive,
                head_flushes: value.head_flushes,
                body_flushes: value.body_flushes,
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
enum MdocSegmentSlot {
    Head,
    Body(usize),
}

fn mdoc_segment_mut(
    items: &mut [NativeMdocListItem],
    owner: (usize, MdocSegmentSlot),
) -> &mut NativeMdocListSegment {
    match owner.1 {
        MdocSegmentSlot::Head => &mut items[owner.0].head,
        MdocSegmentSlot::Body(body) => &mut items[owner.0].bodies[body],
    }
}

fn native_node_wrapper_index(report: &NativeExecutionReport) -> Vec<Option<usize>> {
    let mut by_node = vec![None; report.nodes().len()];
    for (index, wrapper) in report
        .wrappers()
        .iter()
        .enumerate()
        .filter(|(_, wrapper)| wrapper.kind == ExecutionWrapperKind::Node)
    {
        let node = wrapper.node.expect("validated native node wrapper key");
        assert!(
            by_node[node.0 as usize].replace(index).is_none(),
            "one native wrapper per execution node"
        );
    }
    by_node
}

fn empty_mdoc_segment(
    report: &NativeExecutionReport,
    node_wrappers: &[Option<usize>],
    node: ExecutionNodeKey,
    role: NativeMdocListRole,
    ordinal: u32,
) -> NativeMdocListSegment {
    let wrapper =
        &report.wrappers()[node_wrappers[node.0 as usize].expect("validated native node wrapper")];
    NativeMdocListSegment {
        node,
        role,
        ordinal,
        atoms: wrapper.enter_atom..wrapper.leave_atom,
        flushes: Vec::new(),
        boundaries: Vec::new(),
        runs: Vec::new(),
        anchors: Vec::new(),
    }
}

type MdocListDirectOwnership = (
    Vec<NativeMdocListItem>,
    Vec<Option<usize>>,
    Vec<Option<(usize, MdocSegmentSlot)>>,
);
type MdocListInclusiveOwnership = (Vec<Vec<usize>>, Vec<Vec<(usize, MdocSegmentSlot)>>);

fn collect_mdoc_list_items(
    document: &NativeDocument,
    report: &NativeExecutionReport,
) -> MdocListDirectOwnership {
    let ast_nodes = native_nodes_by_execution_key(document, report.nodes().len());
    let node_wrappers = native_node_wrapper_index(report);
    let mut items = Vec::<NativeMdocListItem>::new();
    let mut direct_items = vec![None; report.wrappers().len()];
    let mut direct_segments = vec![None; report.wrappers().len()];

    for wrapper in report
        .wrappers()
        .iter()
        .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::MdocListItem)
    {
        let owner = wrapper.node.expect("validated mdoc list item node");
        let ast_item = ast_nodes[owner.0 as usize];
        let list_body = report.nodes()[owner.0 as usize]
            .parent
            .expect("validated mdoc list body");
        let list = report.nodes()[list_body.0 as usize]
            .parent
            .expect("validated mdoc list block");
        let mut head = None;
        let mut bodies = Vec::new();
        for child in &ast_item.children {
            let key = ExecutionNodeKey(
                child
                    .execution_node_key
                    .expect("executed mdoc list child key"),
            );
            match child.kind {
                NodeKind::Head => {
                    assert!(head.is_none(), "one native mdoc item head");
                    head = Some(empty_mdoc_segment(
                        report,
                        &node_wrappers,
                        key,
                        NativeMdocListRole::Head,
                        0,
                    ));
                }
                NodeKind::Body => {
                    bodies.push(empty_mdoc_segment(
                        report,
                        &node_wrappers,
                        key,
                        NativeMdocListRole::Body,
                        u32::try_from(bodies.len()).expect("bounded mdoc list body count"),
                    ));
                }
                _ => panic!("validated mdoc It child role"),
            }
        }
        let item_index = items.len();
        direct_items[wrapper.key as usize] = Some(item_index);
        let head = head.expect("validated mdoc item head");
        let head_wrapper =
            node_wrappers[head.node.0 as usize].expect("validated native mdoc head wrapper");
        direct_segments[head_wrapper] = Some((item_index, MdocSegmentSlot::Head));
        for (body, segment) in bodies.iter().enumerate() {
            let body_wrapper =
                node_wrappers[segment.node.0 as usize].expect("validated native mdoc body wrapper");
            direct_segments[body_wrapper] = Some((item_index, MdocSegmentSlot::Body(body)));
        }
        items.push(NativeMdocListItem {
            owner,
            flow_epoch: ast_item.flow_epoch,
            list,
            wrapper: wrapper.key,
            kind: wrapper.mdoc_list_kind.expect("typed mdoc list kind"),
            compact: wrapper.mdoc_list_compact(),
            state_before: wrapper.state_before,
            state_after: wrapper.state_after,
            atoms: wrapper.enter_atom..wrapper.leave_atom,
            head,
            bodies,
            flushes: Vec::new(),
            boundaries: Vec::new(),
            runs: Vec::new(),
            anchors: Vec::new(),
        });
    }

    (items, direct_items, direct_segments)
}

fn propagate_wrapper_ownership<T: Copy>(
    report: &NativeExecutionReport,
    direct: &[Option<T>],
) -> Vec<Vec<T>> {
    let mut inherited_by_wrapper: Vec<Vec<T>> = Vec::with_capacity(report.wrappers().len());
    for wrapper in report.wrappers() {
        assert_eq!(wrapper.key as usize, inherited_by_wrapper.len());
        let mut inherited = wrapper.parent.map_or_else(Vec::new, |parent| {
            inherited_by_wrapper[parent as usize].clone()
        });
        if let Some(owner) = direct[wrapper.key as usize] {
            inherited.push(owner);
        }
        inherited_by_wrapper.push(inherited);
    }
    inherited_by_wrapper
}

fn propagate_mdoc_list_ownership(
    report: &NativeExecutionReport,
    direct_items: &[Option<usize>],
    direct_segments: &[Option<(usize, MdocSegmentSlot)>],
) -> MdocListInclusiveOwnership {
    (
        propagate_wrapper_ownership(report, direct_items),
        propagate_wrapper_ownership(report, direct_segments),
    )
}

fn active_wrappers_at_sequences(
    report: &NativeExecutionReport,
    sequences: impl IntoIterator<Item = u64>,
) -> Vec<Option<usize>> {
    let sequences = sequences.into_iter().collect::<Vec<_>>();
    let mut queries = sequences.iter().copied().enumerate().collect::<Vec<_>>();
    queries.sort_unstable_by_key(|(_, sequence)| *sequence);
    let mut wrappers = (0..report.wrappers().len()).collect::<Vec<_>>();
    wrappers.sort_unstable_by_key(|index| report.wrappers()[*index].enter_sequence);

    let mut active = Vec::<usize>::new();
    let mut wrapper_cursor = 0;
    let mut result = vec![None; sequences.len()];
    for (query, sequence) in queries {
        while wrappers
            .get(wrapper_cursor)
            .is_some_and(|index| report.wrappers()[*index].enter_sequence < sequence)
        {
            let wrapper = wrappers[wrapper_cursor];
            let enter = report.wrappers()[wrapper].enter_sequence;
            while active
                .last()
                .is_some_and(|index| report.wrappers()[*index].leave_sequence <= enter)
            {
                active.pop();
            }
            assert_eq!(
                report.wrappers()[wrapper].parent.map(|key| key as usize),
                active.last().copied(),
                "validated native wrapper nesting"
            );
            active.push(wrapper);
            wrapper_cursor += 1;
        }
        while active
            .last()
            .is_some_and(|index| report.wrappers()[*index].leave_sequence <= sequence)
        {
            active.pop();
        }
        result[query] = active.last().copied();
    }
    result
}

fn mdoc_list_facts(
    document: &NativeDocument,
    report: &NativeExecutionReport,
    runs: &[NativeTextRun],
) -> Vec<NativeMdocList> {
    let (mut items, direct_items, direct_segments) = collect_mdoc_list_items(document, report);
    let (items_by_wrapper, segments_by_wrapper) =
        propagate_mdoc_list_ownership(report, &direct_items, &direct_segments);

    for run in runs {
        let Some(wrapper) = run.wrapper.map(|key| key as usize) else {
            continue;
        };
        for &item in &items_by_wrapper[wrapper] {
            items[item].runs.push(run.clone());
        }
        for &owner in &segments_by_wrapper[wrapper] {
            mdoc_segment_mut(&mut items, owner).runs.push(run.clone());
        }
    }
    let flush_wrappers =
        active_wrappers_at_sequences(report, report.flushes().iter().map(|flush| flush.sequence));
    for (flush, wrapper) in report.flushes().iter().zip(flush_wrappers) {
        let Some(wrapper) = wrapper else {
            continue;
        };
        for &item in &items_by_wrapper[wrapper] {
            items[item].flushes.push(flush.clone());
        }
        for &owner in &segments_by_wrapper[wrapper] {
            mdoc_segment_mut(&mut items, owner)
                .flushes
                .push(flush.clone());
        }
    }
    for boundary in report.boundaries() {
        let Some(wrapper) = boundary.wrapper.map(|key| key as usize) else {
            continue;
        };
        for &item in &items_by_wrapper[wrapper] {
            items[item].boundaries.push(boundary.clone());
        }
        for &owner in &segments_by_wrapper[wrapper] {
            mdoc_segment_mut(&mut items, owner)
                .boundaries
                .push(boundary.clone());
        }
    }
    let anchor_wrappers = active_wrappers_at_sequences(
        report,
        report.anchors().iter().map(|anchor| anchor.sequence),
    );
    for (anchor, wrapper) in report.anchors().iter().zip(anchor_wrappers) {
        let Some(wrapper) = wrapper else {
            continue;
        };
        for &item in &items_by_wrapper[wrapper] {
            items[item].anchors.push(anchor.key);
        }
        for &owner in &segments_by_wrapper[wrapper] {
            mdoc_segment_mut(&mut items, owner).anchors.push(anchor.key);
        }
    }

    let mut list_indexes = BTreeMap::<ExecutionNodeKey, usize>::new();
    let mut lists = Vec::<NativeMdocList>::new();
    for item in items {
        let list_index = *list_indexes.entry(item.list).or_insert_with(|| {
            let index = lists.len();
            lists.push(NativeMdocList {
                owner: item.list,
                kind: item.kind,
                compact: item.compact,
                items: Vec::new(),
            });
            index
        });
        assert_eq!(lists[list_index].kind, item.kind);
        assert_eq!(lists[list_index].compact, item.compact);
        lists[list_index].items.push(item);
    }
    append_empty_mdoc_lists(&document.root, &list_indexes, &mut lists);
    lists.sort_by_key(|list| list.owner);
    lists
}

fn append_empty_mdoc_lists(
    node: &NativeNode,
    populated: &BTreeMap<ExecutionNodeKey, usize>,
    lists: &mut Vec<NativeMdocList>,
) {
    if node.kind == NodeKind::Block
        && node.macro_name.as_deref() == Some("Bl")
        && let Some(owner) = node.execution_node_key.map(ExecutionNodeKey)
        && !populated.contains_key(&owner)
        && let Some(kind) = empty_mdoc_list_kind(node)
    {
        lists.push(NativeMdocList {
            owner,
            kind,
            compact: node.compact,
            items: Vec::new(),
        });
    }
    for child in &node.children {
        append_empty_mdoc_lists(child, populated, lists);
    }
}

fn empty_mdoc_list_kind(node: &NativeNode) -> Option<ExecutionMdocListKind> {
    match node.list_kind? {
        NormalizedListKind::Bullet => Some(ExecutionMdocListKind::Bullet),
        NormalizedListKind::Ordered => Some(ExecutionMdocListKind::Enum),
        NormalizedListKind::Column => Some(ExecutionMdocListKind::Column),
        NormalizedListKind::Plain => Some(ExecutionMdocListKind::Item),
        NormalizedListKind::Definition => match node.definition_list_style? {
            libmandoc_rs::DefinitionListStyle::Tag => Some(ExecutionMdocListKind::Tag),
            libmandoc_rs::DefinitionListStyle::Diagnostic => {
                Some(ExecutionMdocListKind::Diagnostic)
            }
            libmandoc_rs::DefinitionListStyle::Hang => Some(ExecutionMdocListKind::Hang),
            libmandoc_rs::DefinitionListStyle::Inset => Some(ExecutionMdocListKind::Inset),
            libmandoc_rs::DefinitionListStyle::Overhang => Some(ExecutionMdocListKind::Overhang),
        },
    }
}

fn empty_man_block_segment(
    report: &NativeExecutionReport,
    node_wrappers: &[Option<usize>],
    node: ExecutionNodeKey,
    role: NativeManBlockRole,
) -> NativeManBlockSegment {
    let wrapper =
        &report.wrappers()[node_wrappers[node.0 as usize].expect("validated native node wrapper")];
    NativeManBlockSegment {
        node,
        role,
        atoms: wrapper.enter_atom..wrapper.leave_atom,
        flushes: Vec::new(),
        boundaries: Vec::new(),
        runs: Vec::new(),
        anchors: Vec::new(),
    }
}

fn man_block_segment_mut(
    blocks: &mut [NativeManBlock],
    owner: (usize, NativeManBlockRole),
) -> &mut NativeManBlockSegment {
    match owner.1 {
        NativeManBlockRole::Head => &mut blocks[owner.0].head,
        NativeManBlockRole::Body => &mut blocks[owner.0].body,
    }
}

type ManBlockDirectOwnership = (
    Vec<NativeManBlock>,
    Vec<Option<usize>>,
    Vec<Option<(usize, NativeManBlockRole)>>,
);

fn collect_man_blocks(
    document: &NativeDocument,
    report: &NativeExecutionReport,
) -> ManBlockDirectOwnership {
    let ast_nodes = native_nodes_by_execution_key(document, report.nodes().len());
    let node_wrappers = native_node_wrapper_index(report);
    let mut blocks = Vec::new();
    let mut direct_blocks = vec![None; report.wrappers().len()];
    let mut direct_segments = vec![None; report.wrappers().len()];

    for wrapper in report
        .wrappers()
        .iter()
        .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::ManBlock)
    {
        let owner = wrapper.node.expect("validated man block owner");
        let ast_block = ast_nodes[owner.0 as usize];
        let child = |kind| {
            ExecutionNodeKey(
                ast_block
                    .children
                    .iter()
                    .find(|child| child.kind == kind)
                    .and_then(|child| child.execution_node_key)
                    .expect("validated man block child"),
            )
        };
        let head = empty_man_block_segment(
            report,
            &node_wrappers,
            child(NodeKind::Head),
            NativeManBlockRole::Head,
        );
        let body = empty_man_block_segment(
            report,
            &node_wrappers,
            child(NodeKind::Body),
            NativeManBlockRole::Body,
        );
        let index = blocks.len();
        direct_blocks[wrapper.key as usize] = Some(index);
        direct_segments[node_wrappers[head.node.0 as usize].expect("man head wrapper")] =
            Some((index, NativeManBlockRole::Head));
        direct_segments[node_wrappers[body.node.0 as usize].expect("man body wrapper")] =
            Some((index, NativeManBlockRole::Body));
        blocks.push(NativeManBlock {
            owner,
            flow_epoch: ast_block.flow_epoch,
            wrapper: wrapper.key,
            kind: wrapper.man_block_kind.expect("typed man block kind"),
            state_before: wrapper.state_before,
            state_after: wrapper.state_after,
            atoms: wrapper.enter_atom..wrapper.leave_atom,
            head,
            body,
            flushes: Vec::new(),
            boundaries: Vec::new(),
            runs: Vec::new(),
            anchors: Vec::new(),
        });
    }

    (blocks, direct_blocks, direct_segments)
}

fn man_block_facts(
    document: &NativeDocument,
    report: &NativeExecutionReport,
    runs: &[NativeTextRun],
) -> Vec<NativeManBlock> {
    let (mut blocks, direct_blocks, direct_segments) = collect_man_blocks(document, report);

    let blocks_by_wrapper = propagate_wrapper_ownership(report, &direct_blocks);
    let segments_by_wrapper = propagate_wrapper_ownership(report, &direct_segments);
    for run in runs {
        let Some(wrapper) = run.wrapper.map(|key| key as usize) else {
            continue;
        };
        for &block in &blocks_by_wrapper[wrapper] {
            blocks[block].runs.push(run.clone());
        }
        for &segment in &segments_by_wrapper[wrapper] {
            man_block_segment_mut(&mut blocks, segment)
                .runs
                .push(run.clone());
        }
    }
    let flush_wrappers =
        active_wrappers_at_sequences(report, report.flushes().iter().map(|flush| flush.sequence));
    for (flush, wrapper) in report.flushes().iter().zip(flush_wrappers) {
        let Some(wrapper) = wrapper else { continue };
        for &block in &blocks_by_wrapper[wrapper] {
            blocks[block].flushes.push(flush.clone());
        }
        for &segment in &segments_by_wrapper[wrapper] {
            man_block_segment_mut(&mut blocks, segment)
                .flushes
                .push(flush.clone());
        }
    }
    for boundary in report.boundaries() {
        let Some(wrapper) = boundary.wrapper.map(|key| key as usize) else {
            continue;
        };
        for &block in &blocks_by_wrapper[wrapper] {
            blocks[block].boundaries.push(boundary.clone());
        }
        for &segment in &segments_by_wrapper[wrapper] {
            man_block_segment_mut(&mut blocks, segment)
                .boundaries
                .push(boundary.clone());
        }
    }
    let anchor_wrappers = active_wrappers_at_sequences(
        report,
        report.anchors().iter().map(|anchor| anchor.sequence),
    );
    for (anchor, wrapper) in report.anchors().iter().zip(anchor_wrappers) {
        let Some(wrapper) = wrapper else { continue };
        for &block in &blocks_by_wrapper[wrapper] {
            blocks[block].anchors.push(anchor.key);
        }
        for &segment in &segments_by_wrapper[wrapper] {
            man_block_segment_mut(&mut blocks, segment)
                .anchors
                .push(anchor.key);
        }
    }
    blocks
}

fn region_facts(report: &NativeExecutionReport, runs: &[NativeTextRun]) -> Vec<NativeRegion> {
    let mut regions = Vec::<NativeRegion>::new();
    let mut direct_regions = vec![None; report.wrappers().len()];
    for wrapper in report
        .wrappers()
        .iter()
        .filter(|wrapper| wrapper.kind == ExecutionWrapperKind::Region)
    {
        let owner = wrapper.node.expect("native region node");
        let origin = &report.nodes()[owner.0 as usize];
        let index = regions.len();
        direct_regions[wrapper.key as usize] = Some(index);
        regions.push(NativeRegion {
            owner,
            wrapper: wrapper.key,
            kind: wrapper.region_kind.expect("typed native execution region"),
            source: report.sources()[origin.source as usize].path.clone(),
            line: origin.line,
            column: origin.column,
            state_before: wrapper.state_before,
            state_after: wrapper.state_after,
            atoms: wrapper.enter_atom..wrapper.leave_atom,
            runs: Vec::new(),
            flushes: Vec::new(),
            boundaries: Vec::new(),
            controls: Vec::new(),
            anchors: Vec::new(),
        });
    }

    let regions_by_wrapper = propagate_wrapper_ownership(report, &direct_regions);
    for run in runs {
        let Some(wrapper) = run.wrapper.map(|key| key as usize) else {
            continue;
        };
        for &region in &regions_by_wrapper[wrapper] {
            regions[region].runs.push(run.clone());
        }
    }

    let flush_wrappers =
        active_wrappers_at_sequences(report, report.flushes().iter().map(|flush| flush.sequence));
    for (flush, wrapper) in report.flushes().iter().zip(flush_wrappers) {
        let Some(wrapper) = wrapper else { continue };
        for &region in &regions_by_wrapper[wrapper] {
            regions[region].flushes.push(flush.clone());
        }
    }
    for boundary in report.boundaries() {
        let Some(wrapper) = boundary.wrapper.map(|key| key as usize) else {
            continue;
        };
        for &region in &regions_by_wrapper[wrapper] {
            regions[region].boundaries.push(boundary.clone());
        }
    }
    for control in report.controls() {
        for &region in &regions_by_wrapper[control.wrapper as usize] {
            regions[region].controls.push(control.clone());
        }
    }
    let anchor_wrappers = active_wrappers_at_sequences(
        report,
        report.anchors().iter().map(|anchor| anchor.sequence),
    );
    for (anchor, wrapper) in report.anchors().iter().zip(anchor_wrappers) {
        let Some(wrapper) = wrapper else { continue };
        for &region in &regions_by_wrapper[wrapper] {
            regions[region].anchors.push(anchor.key);
        }
    }
    regions
}

fn atom_reference_owners(report: &NativeExecutionReport) -> Vec<Option<u32>> {
    let mut atom_references = vec![None; report.atoms().len()];
    let mut reference_events = report
        .references()
        .iter()
        .filter(|reference| !reference.atoms.is_empty())
        .flat_map(|reference| {
            [
                (reference.atoms.start, true, reference.key),
                (reference.atoms.end, false, reference.key),
            ]
        })
        .collect::<Vec<_>>();
    reference_events.sort_unstable_by(|left, right| {
        left.0.cmp(&right.0).then_with(|| match (left.1, right.1) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            (true, true) => left.2.cmp(&right.2),
            (false, false) => right.2.cmp(&left.2),
        })
    });
    let mut references = Vec::new();
    let mut event = 0;
    for (atom, reference) in atom_references.iter_mut().enumerate() {
        while event < reference_events.len() && reference_events[event].0 as usize == atom {
            let (_, entering, key) = reference_events[event];
            if entering {
                references.push(key);
            } else {
                assert_eq!(references.pop(), Some(key), "validated reference nesting");
            }
            event += 1;
        }
        *reference = references.last().copied();
    }
    while event < reference_events.len()
        && reference_events[event].0 as usize == atom_references.len()
    {
        let (_, entering, key) = reference_events[event];
        assert!(!entering, "validated reference interval end");
        assert_eq!(references.pop(), Some(key), "validated reference nesting");
        event += 1;
    }
    assert_eq!(event, reference_events.len(), "validated reference cursor");
    assert!(references.is_empty(), "validated reference closure");
    atom_references
}

fn atom_word_owners(report: &NativeExecutionReport) -> Vec<Option<u32>> {
    let mut owners = vec![None; report.atoms().len()];
    for word in report.words() {
        for owner in &mut owners[word.atoms.start as usize..word.atoms.end as usize] {
            assert!(
                owner.replace(word.key.0).is_none(),
                "validated formatter words do not overlap"
            );
        }
    }
    owners
}

fn text_projection(report: &NativeExecutionReport) -> (Vec<NativeTextRun>, Vec<String>) {
    let mut runs: Vec<NativeTextRun> = Vec::new();
    let atom_references = atom_reference_owners(report);
    let atom_words = atom_word_owners(report);
    let mut visible_cells: BTreeMap<u32, BTreeMap<i64, char>> = BTreeMap::new();
    for fragment in report.fragments() {
        let Some(node) = fragment.node else {
            continue;
        };
        if fragment.role != FragmentRole::Content {
            continue;
        }
        for key in &fragment.atoms {
            let atom = &report.atoms()[key.0 as usize];
            let reference = atom_references[key.0 as usize];
            let word = atom_words[key.0 as usize];
            let origin = &report.nodes()[node.0 as usize];
            let source = report.sources()[origin.source as usize].path.clone();
            let Some(character) = char::from_u32(atom.display_scalar) else {
                continue;
            };
            if character != '\u{8}' {
                visible_cells
                    .entry(fragment.device_line)
                    .or_default()
                    .insert(fragment.start_bu, character);
            }
            if let Some(previous) = runs.last_mut()
                && previous.node == node
                && previous.font == atom.font
                && previous.role == atom.role
                && previous.word == word
                && previous.wrapper == atom.wrapper
                && previous.device_line == fragment.device_line
                && previous.end_bu == fragment.start_bu
                && previous.reference == reference
            {
                previous.text.push(character);
                previous.atoms.push(key.0);
                previous.end_bu = fragment.end_bu;
            } else {
                runs.push(NativeTextRun {
                    node,
                    source,
                    line: origin.line,
                    column: origin.column,
                    font: atom.font,
                    role: atom.role,
                    word,
                    wrapper: atom.wrapper,
                    atoms: vec![key.0],
                    text: character.to_string(),
                    device_line: fragment.device_line,
                    start_bu: fragment.start_bu,
                    end_bu: fragment.end_bu,
                    reference,
                });
            }
        }
    }
    let visible_lines = visible_cells
        .into_values()
        .map(|cells| {
            let Some((&start, _)) = cells.first_key_value() else {
                return String::new();
            };
            let Some((&end, _)) = cells.last_key_value() else {
                return String::new();
            };
            (start..=end)
                .step_by(24)
                .map(|column| cells.get(&column).copied().unwrap_or(' '))
                .collect()
        })
        .collect();
    (runs, visible_lines)
}

fn heading_kind(kind: ExecutionHeadingKind) -> NativeHeadingKind {
    match kind {
        ExecutionHeadingKind::ManSection => NativeHeadingKind::ManSection,
        ExecutionHeadingKind::ManSubsection => NativeHeadingKind::ManSubsection,
        ExecutionHeadingKind::MdocSection => NativeHeadingKind::MdocSection,
        ExecutionHeadingKind::MdocSubsection => NativeHeadingKind::MdocSubsection,
    }
}

pub(super) fn heading_fragment(authored: &str) -> String {
    authored
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"!$&'()*+,-./:;=?@_".contains(&byte) {
                char::from(byte)
            } else {
                '_'
            }
        })
        .collect()
}

fn heading_display_lines(runs: &[NativeTextRun]) -> Vec<String> {
    let mut lines = BTreeMap::<u32, Vec<&NativeTextRun>>::new();
    for run in runs {
        lines.entry(run.device_line).or_default().push(run);
    }
    lines
        .into_values()
        .map(|mut line| {
            line.sort_unstable_by_key(|run| run.start_bu);
            let mut output = String::new();
            let mut cursor = line.first().map_or(0, |run| run.start_bu);
            for run in line {
                if run.start_bu > cursor {
                    let cells = usize::try_from((run.start_bu - cursor) / 24)
                        .expect("validated native heading geometry");
                    output.extend(std::iter::repeat_n(' ', cells));
                }
                output.push_str(&run.text);
                cursor = cursor.max(run.end_bu);
            }
            output
        })
        .collect()
}

struct HeadingCollector<'a> {
    report: &'a NativeExecutionReport,
    wrapper_by_node: &'a [Option<usize>],
    heading_by_head: &'a mut [Option<usize>],
    heading_by_wrapper: &'a mut [Option<usize>],
    headings: &'a mut Vec<NativeHeadingFact>,
}

impl HeadingCollector<'_> {
    fn visit(&mut self, node: &NativeNode) {
        if matches!(
            (node.kind, node.macro_name.as_deref()),
            (NodeKind::Block, Some("SH" | "SS" | "Sh" | "Ss"))
        ) {
            self.collect(node);
        }
        for child in &node.children {
            self.visit(child);
        }
    }

    fn collect(&mut self, node: &NativeNode) {
        let child_key = |kind, message| {
            ExecutionNodeKey(
                node.children
                    .iter()
                    .find(|child| child.kind == kind)
                    .and_then(|child| child.execution_node_key)
                    .expect(message),
            )
        };
        let section = ExecutionNodeKey(
            node.execution_node_key
                .expect("executed section block has a native key"),
        );
        let head = child_key(NodeKind::Head, "native section block has an executed head");
        let body = child_key(NodeKind::Body, "native section block has an executed body");
        let head_node = node
            .children
            .iter()
            .find(|child| child.kind == NodeKind::Head)
            .expect("validated native section head");
        let wrapper_index =
            self.wrapper_by_node[head.0 as usize].expect("validated native heading wrapper");
        let wrapper = &self.report.wrappers()[wrapper_index];
        let heading_index = self.headings.len();
        assert!(
            self.heading_by_head[head.0 as usize]
                .replace(heading_index)
                .is_none(),
            "one native heading per head node"
        );
        assert!(
            self.heading_by_wrapper[wrapper_index]
                .replace(heading_index)
                .is_none(),
            "one native fact per heading wrapper"
        );
        let authored_phrase = wrapper.target.map(|range| {
            String::from_utf8_lossy(
                self.report
                    .pool_bytes(range)
                    .expect("validated native heading phrase"),
            )
            .into_owned()
        });
        let authored_fragment = head_node
            .tag
            .clone()
            .or_else(|| node.tag.clone())
            .or_else(|| {
                (head_node.flags.deep_link_target || node.flags.deep_link_target)
                    .then_some(())
                    .and_then(|()| authored_phrase.as_deref().map(heading_fragment))
            });
        self.headings.push(NativeHeadingFact {
            kind: heading_kind(wrapper.heading_kind.expect("typed native heading kind")),
            section,
            head,
            body,
            authored_phrase,
            authored_fragment,
            atoms: wrapper.enter_atom..wrapper.leave_atom,
            boundaries: Vec::new(),
            runs: Vec::new(),
            display_lines: Vec::new(),
            label: String::new(),
        });
    }
}

fn heading_facts(
    document: &NativeDocument,
    report: &NativeExecutionReport,
    runs: &[NativeTextRun],
) -> Vec<NativeHeadingFact> {
    let mut heading_wrapper_by_node = vec![None; report.nodes().len()];
    for (wrapper_index, wrapper) in report.wrappers().iter().enumerate() {
        if wrapper.kind == ExecutionWrapperKind::Heading {
            let node = wrapper.node.expect("validated native heading node");
            assert!(
                heading_wrapper_by_node[node.0 as usize]
                    .replace(wrapper_index)
                    .is_none(),
                "one native heading wrapper per head node"
            );
        }
    }
    let mut headings = Vec::new();
    let mut heading_by_head = vec![None; report.nodes().len()];
    let mut heading_by_wrapper = vec![None; report.wrappers().len()];
    HeadingCollector {
        report,
        wrapper_by_node: &heading_wrapper_by_node,
        heading_by_head: &mut heading_by_head,
        heading_by_wrapper: &mut heading_by_wrapper,
        headings: &mut headings,
    }
    .visit(&document.root);

    let mut heading_by_node = vec![None; report.nodes().len()];
    for node in report.nodes() {
        heading_by_node[node.key.0 as usize] = heading_by_head[node.key.0 as usize].or_else(|| {
            node.parent
                .and_then(|parent| heading_by_node[parent.0 as usize])
        });
    }
    for run in runs {
        if let Some(heading) = heading_by_node[run.node.0 as usize] {
            headings[heading].runs.push(run.clone());
        }
    }

    let mut heading_by_active_wrapper = vec![None; report.wrappers().len()];
    for wrapper in report.wrappers() {
        heading_by_active_wrapper[wrapper.key as usize] = heading_by_wrapper[wrapper.key as usize]
            .or_else(|| {
                wrapper
                    .parent
                    .and_then(|parent| heading_by_active_wrapper[parent as usize])
            });
    }
    for boundary in report.boundaries() {
        if let Some(heading) = boundary
            .wrapper
            .and_then(|wrapper| heading_by_active_wrapper[wrapper as usize])
        {
            headings[heading].boundaries.push(boundary.clone());
        }
    }
    for heading in &mut headings {
        heading.display_lines = heading_display_lines(&heading.runs);
        heading.label = heading
            .display_lines
            .iter()
            .flat_map(|line| line.split_whitespace())
            .collect::<Vec<_>>()
            .join(" ");
    }
    headings
}

fn native_nodes_by_execution_key(document: &NativeDocument, node_count: usize) -> Vec<&NativeNode> {
    fn visit<'a>(node: &'a NativeNode, indexed: &mut [Option<&'a NativeNode>]) {
        if let Some(key) = node.execution_node_key {
            let slot = indexed
                .get_mut(key as usize)
                .expect("validated native AST execution-node key");
            assert!(
                slot.replace(node).is_none(),
                "unique native AST execution-node key"
            );
        }
        for child in &node.children {
            visit(child, indexed);
        }
    }

    let mut indexed = vec![None; node_count];
    visit(&document.root, &mut indexed);
    indexed
        .into_iter()
        .map(|node| node.expect("every execution node remains in the owned AST"))
        .collect()
}

fn table_row_kind(kind: &NativeTableRowKind) -> mant_ir::TableRowKind {
    match kind {
        NativeTableRowKind::Data => mant_ir::TableRowKind::Data,
        NativeTableRowKind::HorizontalRule => mant_ir::TableRowKind::HorizontalRule,
        NativeTableRowKind::DoubleHorizontalRule => mant_ir::TableRowKind::DoubleHorizontalRule,
        NativeTableRowKind::LayoutRule { cells } => mant_ir::TableRowKind::LayoutRule {
            cells: cells
                .iter()
                .map(|cell| match cell {
                    NativeTableRuleCellKind::Horizontal => mant_ir::TableRuleCellKind::Horizontal,
                    NativeTableRuleCellKind::DoubleHorizontal => {
                        mant_ir::TableRuleCellKind::DoubleHorizontal
                    }
                })
                .collect(),
        },
    }
}

const fn table_cell_kind(kind: NativeTableCellKind) -> mant_ir::TableCellKind {
    match kind {
        NativeTableCellKind::Text | NativeTableCellKind::Empty => mant_ir::TableCellKind::Text,
        NativeTableCellKind::HorizontalRule => mant_ir::TableCellKind::HorizontalRule,
        NativeTableCellKind::DoubleHorizontalRule => mant_ir::TableCellKind::DoubleHorizontalRule,
        NativeTableCellKind::IsolatedHorizontalRule => {
            mant_ir::TableCellKind::IsolatedHorizontalRule
        }
        NativeTableCellKind::IsolatedDoubleHorizontalRule => {
            mant_ir::TableCellKind::IsolatedDoubleHorizontalRule
        }
    }
}

const fn execution_table_cell_kind(kind: ExecutionTableDataKind) -> mant_ir::TableCellKind {
    match kind {
        ExecutionTableDataKind::None | ExecutionTableDataKind::Text => mant_ir::TableCellKind::Text,
        ExecutionTableDataKind::HorizontalRule => mant_ir::TableCellKind::HorizontalRule,
        ExecutionTableDataKind::DoubleHorizontalRule => {
            mant_ir::TableCellKind::DoubleHorizontalRule
        }
        ExecutionTableDataKind::IsolatedHorizontalRule => {
            mant_ir::TableCellKind::IsolatedHorizontalRule
        }
        ExecutionTableDataKind::IsolatedDoubleHorizontalRule => {
            mant_ir::TableCellKind::IsolatedDoubleHorizontalRule
        }
    }
}

const fn table_alignment(alignment: NativeTableAlignment) -> mant_ir::TableAlignment {
    match alignment {
        NativeTableAlignment::Left => mant_ir::TableAlignment::Left,
        NativeTableAlignment::Center => mant_ir::TableAlignment::Center,
        NativeTableAlignment::Right => mant_ir::TableAlignment::Right,
    }
}

fn styled_table_text(value: String, font: ExecutionFont) -> mant_ir::Inline {
    let text = mant_ir::Inline::Text { value };
    match font {
        ExecutionFont::Roman => text,
        ExecutionFont::Bold => mant_ir::Inline::Strong {
            children: vec![text],
        },
        ExecutionFont::Underline => mant_ir::Inline::Emphasis {
            children: vec![text],
        },
        ExecutionFont::BoldUnderline => mant_ir::Inline::Strong {
            children: vec![mant_ir::Inline::Emphasis {
                children: vec![text],
            }],
        },
    }
}

fn table_cell_content(report: &NativeExecutionReport, atoms: Range<u32>) -> Vec<mant_ir::Inline> {
    fn flush_run(
        output: &mut Vec<mant_ir::Inline>,
        buffer: &mut String,
        font: &mut Option<ExecutionFont>,
    ) {
        if let Some(font) = font.take()
            && !buffer.is_empty()
        {
            output.push(styled_table_text(std::mem::take(buffer), font));
        }
    }

    let start = usize::try_from(atoms.start).expect("validated table atom start");
    let end = usize::try_from(atoms.end).expect("validated table atom end");
    let mut output = Vec::new();
    let mut buffer = String::new();
    let mut font = None;
    let mut consume_break_space = false;
    for atom in &report.atoms()[start..end] {
        if atom.role != AtomRole::TableCellPayload {
            continue;
        }
        if atom.kind == AtomKind::WordEndBreak {
            flush_run(&mut output, &mut buffer, &mut font);
            output.push(mant_ir::Inline::LineBreak);
            consume_break_space = true;
            continue;
        }
        if consume_break_space
            && atom.kind == AtomKind::BreakableSpace
            && atom.disposition == AtomDisposition::Consumed
        {
            consume_break_space = false;
            continue;
        }
        consume_break_space = false;
        let character = match atom.kind {
            AtomKind::Glyph if atom.disposition == AtomDisposition::Emitted => {
                char::from_u32(atom.display_scalar)
            }
            AtomKind::BreakableSpace
                if matches!(
                    atom.disposition,
                    AtomDisposition::Emitted | AtomDisposition::Consumed
                ) =>
            {
                Some(' ')
            }
            AtomKind::NonBreakingSpace
                if matches!(
                    atom.disposition,
                    AtomDisposition::Emitted | AtomDisposition::Consumed
                ) =>
            {
                Some('\u{a0}')
            }
            AtomKind::Tab
                if matches!(
                    atom.disposition,
                    AtomDisposition::Emitted | AtomDisposition::Consumed
                ) =>
            {
                Some('\t')
            }
            AtomKind::Glyph
            | AtomKind::BreakableSpace
            | AtomKind::NonBreakingSpace
            | AtomKind::BreakableHyphen
            | AtomKind::ZeroWidth
            | AtomKind::Tab
            | AtomKind::TabReference
            | AtomKind::Backspace
            | AtomKind::BreakPoint => None,
            AtomKind::WordEndBreak => unreachable!("handled above"),
        };
        let Some(character) = character else {
            continue;
        };
        if font != Some(atom.font) {
            flush_run(&mut output, &mut buffer, &mut font);
            font = Some(atom.font);
        }
        buffer.push(character);
    }
    flush_run(&mut output, &mut buffer, &mut font);
    output
}

fn project_execution_table_cell(
    report: &NativeExecutionReport,
    cell: &ExecutionTableCell,
    origin: &libmandoc_rs::ExecutionNode,
    source: &std::path::Path,
    fragments_by_generation: &[Vec<ExecutionFragment>],
    flushes_by_generation: &[Vec<ExecutionFlush>],
) -> NativeTableCell {
    let fragments = cell.buffer_generation.map_or_else(Vec::new, |generation| {
        fragments_by_generation[generation as usize].clone()
    });
    let flushes = cell.buffer_generation.map_or_else(Vec::new, |generation| {
        flushes_by_generation[generation as usize].clone()
    });
    NativeTableCell {
        key: cell.key,
        node: cell.node,
        source: source.to_path_buf(),
        line: origin.line,
        column: origin.column,
        ordinal: cell.ordinal,
        data_ordinal: cell.data_ordinal,
        logical_column: cell.logical_column,
        column_span: cell.column_span,
        row_span: cell.row_span,
        layout_kind: cell.layout_kind,
        data_kind: cell.data_kind,
        alignment: cell.alignment,
        font: cell.font,
        flags: cell.flags,
        buffer_generation: cell.buffer_generation,
        atoms: cell.atoms.clone(),
        fragments,
        flushes,
        content: table_cell_content(report, cell.atoms.clone()),
    }
}

fn project_ir_table_cells(
    ast_row: &NativeNode,
    row: &ExecutionTableRow,
    projected_cells: &[NativeTableCell],
    data_cells: Vec<Option<usize>>,
) -> Vec<mant_ir::TableCell> {
    let source = super::source_span(ast_row);
    ast_row
        .table_cells
        .iter()
        .zip(data_cells)
        .scan(0u32, |logical_column, (ast_cell, projected)| {
            let projected =
                &projected_cells[projected.expect("every AST table data cell was executed")];
            let kind = table_cell_kind(ast_cell.kind);
            assert_eq!(projected.node, row.node, "table cell row owner");
            assert_eq!(projected.logical_column, *logical_column);
            assert_eq!(projected.column_span, u32::from(ast_cell.column_span));
            assert_eq!(projected.row_span, u32::from(ast_cell.row_span));
            assert_eq!(execution_table_cell_kind(projected.data_kind), kind);
            assert_eq!(
                projected
                    .flags
                    .contains(ExecutionTableCellFlags::VERTICAL_CONTINUATION,),
                ast_cell.vertical_continuation,
            );
            *logical_column = logical_column
                .checked_add(u32::from(ast_cell.column_span))
                .expect("validated table logical width");
            let blocks = if ast_cell.vertical_continuation
                || kind != mant_ir::TableCellKind::Text
                || projected.content.is_empty()
            {
                Vec::new()
            } else {
                vec![mant_ir::Block::Paragraph {
                    children: projected.content.clone(),
                    layout: mant_ir::LayoutHint::default(),
                    source,
                }]
            };
            Some(mant_ir::TableCell {
                kind,
                blocks,
                column_span: ast_cell.column_span,
                row_span: ast_cell.row_span,
                alignment: Some(table_alignment(ast_cell.alignment)),
            })
        })
        .collect()
}

fn project_table_row(
    ast_nodes: &[&NativeNode],
    report: &NativeExecutionReport,
    row: &ExecutionTableRow,
    fragments_by_generation: &[Vec<ExecutionFragment>],
    flushes_by_generation: &[Vec<ExecutionFlush>],
) -> (NativeTableRow, mant_ir::TableRow) {
    let ast_row = ast_nodes[row.node.0 as usize];
    assert_eq!(ast_row.kind, NodeKind::Table, "table row AST kind");
    let kind = table_row_kind(
        ast_row
            .table_row_kind
            .as_ref()
            .expect("owned AST table row kind"),
    );
    let origin = &report.nodes()[row.node.0 as usize];
    assert_eq!(ast_row.line, origin.line, "table row source line");
    assert_eq!(ast_row.column, origin.column, "table row source column");
    let source = report.sources()[origin.source as usize].path.clone();
    let execution_cells = &report.table_cells()[row.cells.start as usize..row.cells.end as usize];
    let mut data_cells = vec![None; ast_row.table_cells.len()];
    let projected_cells = execution_cells
        .iter()
        .map(|cell| {
            if matches!(&kind, mant_ir::TableRowKind::Data) {
                let slot = data_cells
                    .get_mut(cell.data_ordinal as usize)
                    .expect("validated table data ordinal");
                assert!(slot.replace(cell.ordinal as usize).is_none());
            }
            project_execution_table_cell(
                report,
                cell,
                origin,
                &source,
                fragments_by_generation,
                flushes_by_generation,
            )
        })
        .collect::<Vec<_>>();
    let ir_cells = if matches!(&kind, mant_ir::TableRowKind::Data) {
        project_ir_table_cells(ast_row, row, &projected_cells, data_cells)
    } else {
        Vec::new()
    };
    let ir_row = mant_ir::TableRow {
        kind: kind.clone(),
        cells: ir_cells,
    };
    (
        NativeTableRow {
            key: row.key,
            node: row.node,
            source,
            line: origin.line,
            column: origin.column,
            kind,
            logical_columns: row.logical_columns,
            cells: projected_cells,
        },
        ir_row,
    )
}

fn table_projection(document: &NativeDocument, report: &NativeExecutionReport) -> Vec<NativeTable> {
    if report.tables().is_empty() {
        return Vec::new();
    }
    let ast_nodes = native_nodes_by_execution_key(document, report.nodes().len());
    let mut fragments_by_generation = vec![Vec::new(); report.buffer_generations().len()];
    for fragment in report.fragments() {
        if let Some(generation) = fragment.buffer_generation {
            fragments_by_generation[generation as usize].push(fragment.clone());
        }
    }
    let mut flushes_by_generation = vec![Vec::new(); report.buffer_generations().len()];
    for flush in report.flushes() {
        flushes_by_generation[flush.buffer_generation as usize].push(flush.clone());
    }

    report
        .tables()
        .iter()
        .map(|table| {
            let first_origin = &report.nodes()[table.first_row_node.0 as usize];
            let source = report.sources()[first_origin.source as usize].path.clone();
            let row_start = table.rows.start as usize;
            let row_end = table.rows.end as usize;
            let mut rows = Vec::with_capacity(row_end - row_start);
            let mut ir_rows = Vec::with_capacity(row_end - row_start);
            for row in &report.table_rows()[row_start..row_end] {
                let (projected, ir_row) = project_table_row(
                    &ast_nodes,
                    report,
                    row,
                    &fragments_by_generation,
                    &flushes_by_generation,
                );
                rows.push(projected);
                ir_rows.push(ir_row);
            }
            NativeTable {
                key: table.key,
                source,
                logical_columns: table.logical_columns,
                rows,
                block: mant_ir::Block::Table {
                    rows: ir_rows,
                    layout: mant_ir::LayoutHint::default(),
                    source: super::source_span(ast_nodes[table.first_row_node.0 as usize]),
                },
            }
        })
        .collect()
}

fn origin_facts(report: &NativeExecutionReport) -> Vec<NativeOrigin> {
    report
        .nodes()
        .iter()
        .map(|node| NativeOrigin {
            key: node.key,
            parent: node.parent,
            source: report.sources()[node.source as usize].path.clone(),
            line: node.line,
            column: node.column,
            kind: node.kind,
            macro_name: node.macro_name.clone(),
        })
        .collect()
}

#[allow(dead_code)]
pub(super) fn project(
    document: &NativeDocument,
    report: &NativeExecutionReport,
) -> NativeProjection {
    let (runs, visible_lines) = text_projection(report);
    let mdoc_lists = mdoc_list_facts(document, report, &runs);
    let man_blocks = man_block_facts(document, report, &runs);
    let regions = region_facts(report, &runs);
    NativeProjection {
        origins: origin_facts(report),
        words: report
            .words()
            .iter()
            .map(|word| NativeWordFact {
                key: word.key.0,
                node: word.node,
                source: report.sources()[word.source as usize].path.clone(),
                operand: report
                    .pool_bytes(word.operand)
                    .expect("validated native formatter operand")
                    .to_vec(),
                role: word.role,
                wrapper: word.wrapper,
                atoms: word.atoms.clone(),
                enter_sequence: word.enter_sequence,
                leave_sequence: word.leave_sequence,
            })
            .collect(),
        headings: heading_facts(document, report, &runs),
        regions,
        runs,
        visible_lines,
        implicit_spaces: report
            .atoms()
            .iter()
            .filter(|atom| atom.role == AtomRole::ImplicitSpace)
            .count(),
        hard_boundaries: report
            .boundaries()
            .iter()
            .filter(|boundary| boundary.effect == BoundaryEffect::EndedLine)
            .count(),
        glyph_geometries: report
            .geometry()
            .iter()
            .filter(|fact| {
                fact.kind == GeometryKind::Glyph && fact.origin_kind == GeometryOriginKind::Atom
            })
            .count(),
        references: report
            .references()
            .iter()
            .map(|reference| NativeReference {
                key: reference.key,
                parent: reference.parent,
                owner_node: reference.owner_node,
                target_node: reference.target_node,
                kind: reference.kind,
                primary: report
                    .pool_bytes(reference.primary)
                    .expect("validated native reference target")
                    .to_vec(),
                secondary: reference.secondary.map(|range| {
                    report
                        .pool_bytes(range)
                        .expect("validated native reference component")
                        .to_vec()
                }),
                execution_atoms: reference.execution_atoms.clone(),
                atoms: reference.atoms.clone(),
                affinity: reference.affinity,
            })
            .collect(),
        anchors: report
            .anchors()
            .iter()
            .map(|anchor| NativeAnchor {
                key: anchor.key,
                node: anchor.node,
                target: report
                    .pool_bytes(anchor.target)
                    .expect("validated native anchor pool range")
                    .to_vec(),
                device_line: anchor.device_line,
                atom_cursor: anchor.atom_cursor,
                fragment_cursor: anchor.fragment_cursor,
                affinity: anchor.affinity,
            })
            .collect(),
        definitions: definition_facts(report, &man_blocks, &mdoc_lists),
        man_blocks,
        mdoc_lists,
        fields: field_facts(report),
        controls: control_facts(report),
        tables: table_projection(document, report),
    }
}

/// Build the staged source-neutral semantic document without changing the
/// lower-level execution-fact projection used by K03--K18.
#[allow(dead_code)]
pub(super) fn project_semantic_document(
    document: &NativeDocument,
    report: &NativeExecutionReport,
) -> semantics::NativeSemanticProjection {
    let projection = project(document, report);
    semantics::project_semantics(
        report.sources().first().map_or_else(
            || std::path::Path::new("manual"),
            |source| source.path.as_path(),
        ),
        document,
        report,
        &projection,
    )
}

/// Materialize the staged native semantic projection for development audits.
///
/// This is intentionally hidden from the stable documentation surface until
/// the K19--K24 migration switches the production roff path.
#[doc(hidden)]
#[must_use]
#[cfg(feature = "staged-native-audit")]
pub fn lower_staged_semantic_document(report: &libmandoc_rs::ExecutionReport) -> mant_ir::Document {
    project_semantic_document(&report.document, &report.execution).document
}

#[cfg(test)]
mod tests {
    use super::*;
    use libmandoc_rs::{ExecutionLimits, FlushOutcome, InputFormat, ParseOptions, Parser};

    fn table_rows(table: &NativeTable) -> &[mant_ir::TableRow] {
        let mant_ir::Block::Table { rows, .. } = &table.block else {
            unreachable!("native table projection always constructs a table block");
        };
        rows
    }

    fn cell_inlines(cell: &mant_ir::TableCell) -> &[mant_ir::Inline] {
        let [mant_ir::Block::Paragraph { children, .. }] = cell.blocks.as_slice() else {
            panic!("printable staged table cell must contain one paragraph");
        };
        children
    }

    fn assert_run_descends_from_heading(
        report: &NativeExecutionReport,
        projection: &NativeProjection,
        heading_label: &str,
        body_text: &str,
        expected_font: Option<ExecutionFont>,
    ) {
        let heading = projection
            .headings
            .iter()
            .find(|heading| heading.label == heading_label)
            .expect("projected heading");
        let run = projection
            .runs
            .iter()
            .find(|run| run.text == body_text)
            .expect("heading body run");
        if let Some(expected_font) = expected_font {
            assert_eq!(run.font, expected_font, "{body_text}");
        }
        let mut node = run.node;
        while node != heading.body {
            node = report.nodes()[node.0 as usize]
                .parent
                .expect("body run descends from its section body");
        }
    }

    fn assert_mdoc_heading_projection(
        report: &NativeExecutionReport,
        projection: &NativeProjection,
    ) {
        assert_eq!(
            projection
                .headings
                .iter()
                .map(|heading| {
                    (
                        heading.kind,
                        heading.authored_phrase.as_deref(),
                        heading.label.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (NativeHeadingKind::MdocSection, Some("NAME"), "NAME"),
                (NativeHeadingKind::MdocSection, Some("Alice"), "-Alice"),
                (NativeHeadingKind::MdocSection, Some("$ Alice"), "$Alice"),
                (NativeHeadingKind::MdocSection, Some(r"PARENT\fI"), "PARENT"),
                (
                    NativeHeadingKind::MdocSubsection,
                    Some(r"CHILD\fI"),
                    "CHILD",
                ),
                (
                    NativeHeadingKind::MdocSection,
                    Some("NEXT SECTION"),
                    "NEXT SECTION",
                ),
                (
                    NativeHeadingKind::MdocSection,
                    Some("NEXTSECTION"),
                    "NEXTSECTION",
                ),
                (NativeHeadingKind::MdocSection, Some("AUTHORS"), "AUTHORS"),
                (NativeHeadingKind::MdocSection, Some("Alice"), "“ Alice”"),
                (NativeHeadingKind::MdocSection, Some("SPACING"), "SPACING"),
                (NativeHeadingKind::MdocSection, Some("NEXT"), "NEXT"),
                (NativeHeadingKind::MdocSection, Some("SEE ALSO"), "SEEALSO"),
            ]
        );
        let multiline = projection
            .headings
            .iter()
            .find(|heading| heading.label == "“ Alice”")
            .expect("multiline author heading");
        assert_eq!(multiline.display_lines, ["“", "Alice”"]);
        assert!(
            projection
                .visible_lines
                .iter()
                .any(|line| line.contains("threefour"))
        );

        for (heading, body, font) in [
            ("-Alice", "FLAGBODY", None),
            ("$Alice", "PREFIXBODY", None),
            ("PARENT", "SECTION-BODY", Some(ExecutionFont::Bold)),
            ("CHILD", "SUB-BODY", Some(ExecutionFont::Bold)),
        ] {
            assert_run_descends_from_heading(report, projection, heading, body, font);
        }

        let sx = projection
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::SameDocumentSection)
            .expect("projected Sx reference");
        assert_eq!(sx.primary, b"NEXT SECTION");
        assert_eq!(
            projection
                .runs
                .iter()
                .filter(|run| run.reference == Some(sx.key))
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "NEXTSECTION"
        );
    }

    fn assert_man_heading_projection(projection: &NativeProjection) {
        assert_eq!(
            projection
                .headings
                .iter()
                .map(|heading| (heading.kind, heading.label.as_str()))
                .collect::<Vec<_>>(),
            [
                (NativeHeadingKind::ManSection, "NAME"),
                (NativeHeadingKind::ManSection, "NEXT"),
                (NativeHeadingKind::ManSubsection, "SUB"),
            ]
        );
        for text in ["TAIL", "TEXT"] {
            let run = projection
                .runs
                .iter()
                .find(|run| run.text == text)
                .expect("man heading body run");
            assert_eq!(run.font, ExecutionFont::Roman, "{text}");
        }
    }

    fn assert_matrix_execution(table: &NativeTable) {
        assert_eq!(
            table.source,
            std::path::Path::new("native-execution-table-matrix.1")
        );
        assert_eq!(table.logical_columns, 3);
        assert_eq!(table.rows.len(), 5);
        assert!(table.rows.iter().all(|row| {
            row.source == std::path::Path::new("native-execution-table-matrix.1")
                && row.line >= 8
                && row.column == 1
        }));
        assert!(matches!(
            table.rows[3].kind,
            mant_ir::TableRowKind::HorizontalRule
        ));
        let first = &table.rows[0];
        assert_eq!(first.cells.len(), 3);
        assert_eq!(first.cells[0].font, ExecutionFont::Bold);
        assert_eq!(first.cells[0].logical_column, 0);
        assert_eq!(first.cells[1].logical_column, 1);
        assert_eq!(first.cells[1].alignment, ExecutionTableAlignment::Numeric);
        assert_eq!(first.cells[2].logical_column, 2);
        assert!(first.cells.iter().all(|cell| {
            cell.fragments
                .iter()
                .all(|fragment| fragment.buffer_generation == cell.buffer_generation)
                && cell
                    .flushes
                    .iter()
                    .all(|flush| Some(flush.buffer_generation) == cell.buffer_generation)
        }));
        assert_eq!(
            first.cells[0].content,
            [mant_ir::Inline::Strong {
                children: vec![mant_ir::Inline::Text {
                    value: "alpha".to_owned(),
                }],
            }]
        );
        assert_eq!(
            first.cells[1].content,
            [mant_ir::Inline::Text {
                value: "12.34".to_owned(),
            }]
        );
        assert_eq!(table.rows[1].cells[0].column_span, 2);
        assert_eq!(table.rows[1].cells[1].logical_column, 2);
        assert!(table.rows[2].cells[1].content.is_empty());
        assert!(table.rows[2].cells[1].buffer_generation.is_none());
    }

    fn assert_matrix_ir(table: &NativeTable) {
        let rows = table_rows(table);
        assert!(rows[3].cells.is_empty());
        assert_eq!(rows[1].cells[0].column_span, 2);
        assert!(rows[2].cells[1].blocks.is_empty());
        assert_eq!(
            cell_inlines(&rows[0].cells[0]),
            table.rows[0].cells[0].content
        );
        assert_eq!(
            cell_inlines(&rows[0].cells[1]),
            [mant_ir::Inline::Text {
                value: "12.34".to_owned(),
            }],
            "numeric alignment padding must not enter semantic content"
        );
        assert_eq!(
            cell_inlines(&rows[4].cells[0]),
            [
                mant_ir::Inline::Text {
                    value: "block one".to_owned(),
                },
                mant_ir::Inline::LineBreak,
                mant_ir::Inline::Text {
                    value: "block two with additional words that wrap softly inside the table cell"
                        .to_owned(),
                },
            ],
            "authored word-end break is the only semantic line break"
        );
        let grid = mant_ir::TableGrid::new(rows);
        assert_eq!(grid.column_count, 3);
        assert_eq!(grid.rows[1][0].column, 0);
        assert_eq!(grid.rows[1][1].column, 2);
    }

    #[test]
    fn consumes_owned_man_and_mdoc_execution_facts() {
        let cases = [
            (
                "native-projection-man.1",
                InputFormat::Man,
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/plain-man.1")
                    .as_slice(),
            ),
            (
                "native-projection-mdoc.1",
                InputFormat::Mdoc,
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/plain-mdoc.1")
                    .as_slice(),
            ),
        ];
        for (path, format, source) in cases {
            let report = Parser::new(ParseOptions::default())
                .with_input_format(format)
                .with_mdoc_operating_system("ManT")
                .unwrap()
                .execute_bytes(path, source, ExecutionLimits::default())
                .unwrap();
            let projection = project(&report.document, &report.execution);
            assert!(!projection.runs.is_empty());
            assert!(
                projection
                    .runs
                    .iter()
                    .all(|run| run.source == std::path::Path::new(path))
            );
            assert_eq!(
                projection.glyph_geometries,
                report.execution.fragments().len()
            );
            assert!(projection.runs.iter().all(|run| run.end_bu >= run.start_bu));
            if format == InputFormat::Man {
                assert_eq!(projection.implicit_spaces, 1);
                assert_eq!(projection.hard_boundaries, 21);
                assert_eq!(
                    projection.visible_lines,
                    [
                        "NAME",
                        "probe - execution report",
                        "DESCRIPTION",
                        "Ordinary words preserve source order and deterministic wrapping across",
                        "the native report boundary.",
                    ]
                );
            } else {
                assert_eq!(projection.implicit_spaces, 4);
                assert_eq!(projection.hard_boundaries, 19);
                assert_eq!(
                    projection.visible_lines,
                    [
                        "NAME",
                        "probe – execution report",
                        "DESCRIPTION",
                        "Plain emphasized text.",
                    ]
                );
                let emphasized = projection
                    .runs
                    .iter()
                    .find(|run| run.text == "emphasized")
                    .expect("native emphasized run");
                assert_eq!(emphasized.font, ExecutionFont::Underline);
                assert_eq!(report.execution.nodes()[emphasized.node.0 as usize].line, 9);
            }
        }
    }

    fn assert_mdoc_list_topology(report: &NativeExecutionReport, projection: &NativeProjection) {
        assert_eq!(
            projection
                .mdoc_lists
                .iter()
                .map(|list| (list.kind, list.compact, list.items.len()))
                .collect::<Vec<_>>(),
            [
                (ExecutionMdocListKind::Bullet, true, 2),
                (ExecutionMdocListKind::Dash, false, 1),
                (ExecutionMdocListKind::Hyphen, false, 1),
                (ExecutionMdocListKind::Enum, false, 2),
                (ExecutionMdocListKind::Item, false, 1),
                (ExecutionMdocListKind::Tag, false, 2),
                (ExecutionMdocListKind::Hang, false, 1),
                (ExecutionMdocListKind::Overhang, false, 1),
                (ExecutionMdocListKind::Inset, false, 1),
                (ExecutionMdocListKind::Diagnostic, false, 1),
                (ExecutionMdocListKind::Column, false, 2),
            ]
        );

        assert!(projection.mdoc_lists.iter().all(|list| {
            list.items.iter().all(|item| {
                item.list == list.owner
                    && item.kind == list.kind
                    && item.compact == list.compact
                    && item.head.role == NativeMdocListRole::Head
                    && item.bodies.iter().enumerate().all(|(ordinal, body)| {
                        body.role == NativeMdocListRole::Body
                            && body.ordinal == u32::try_from(ordinal).expect("bounded body ordinal")
                    })
            })
        }));
        for item in projection.mdoc_lists.iter().flat_map(|list| &list.items) {
            let wrapper = &report.wrappers()[item.wrapper as usize];
            let head_wrapper = report
                .wrappers()
                .iter()
                .find(|candidate| {
                    candidate.kind == ExecutionWrapperKind::Node
                        && candidate.node == Some(item.head.node)
                })
                .expect("native mdoc head wrapper");
            assert!(item.boundaries.iter().any(|boundary| {
                boundary.request == libmandoc_rs::BoundaryRequest::Newline
                    && wrapper.enter_sequence < boundary.enter_sequence
                    && boundary.leave_sequence < head_wrapper.enter_sequence
            }));
            assert!(item.boundaries.iter().all(|boundary| {
                wrapper.enter_sequence < boundary.enter_sequence
                    && boundary.leave_sequence < wrapper.leave_sequence
            }));
            assert!(item.flushes.iter().all(|flush| {
                wrapper.enter_sequence < flush.sequence
                    && flush.outcome_sequence < wrapper.leave_sequence
            }));
            assert!(item.runs.iter().flat_map(|run| &run.atoms).all(|atom| {
                let sequence = report.atoms()[*atom as usize].sequence;
                wrapper.enter_sequence < sequence && sequence < wrapper.leave_sequence
            }));
        }

        let column = projection
            .mdoc_lists
            .iter()
            .find(|list| list.kind == ExecutionMdocListKind::Column)
            .unwrap();
        assert_eq!(column.items[0].bodies.len(), 2);
        assert_eq!(column.items[1].bodies.len(), 2);
        assert!(
            column.items[1]
                .bodies
                .iter()
                .all(|body| body.runs.is_empty())
        );

        let tag = projection
            .mdoc_lists
            .iter()
            .find(|list| list.kind == ExecutionMdocListKind::Tag)
            .unwrap();
        assert_eq!(
            tag.items[0]
                .head
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "-avalue"
        );
    }

    fn assert_mdoc_list_targets(report: &NativeExecutionReport, projection: &NativeProjection) {
        let target = |key: u32| {
            std::str::from_utf8(
                report
                    .pool_bytes(report.anchors()[key as usize].target)
                    .unwrap(),
            )
            .unwrap()
        };
        let item_targets = projection
            .mdoc_lists
            .iter()
            .flat_map(|list| &list.items)
            .map(|item| {
                item.anchors
                    .iter()
                    .map(|key| target(*key))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(item_targets[0], ["bullet-first", "bullet-empty"]);
        assert_eq!(item_targets[7], ["tag-target", "a"]);
        assert_eq!(
            item_targets,
            [
                vec!["bullet-first", "bullet-empty"],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec!["tag-target", "a"],
                vec!["empty"],
                vec!["hang"],
                vec!["over"],
                vec!["inset"],
                vec![],
                vec!["left", "right"],
                vec![],
            ]
        );
        assert!(projection.anchors.iter().any(|anchor| {
            anchor.target == b"before-bullet"
                && !item_targets
                    .iter()
                    .flatten()
                    .any(|target| target.as_bytes() == anchor.target)
        }));
        assert!(projection.anchors.iter().any(|anchor| {
            anchor.target == b"after-lists"
                && !item_targets
                    .iter()
                    .flatten()
                    .any(|target| target.as_bytes() == anchor.target)
        }));
    }

    fn assert_mdoc_segment_includes(outer: &NativeMdocListSegment, inner: &NativeMdocListSegment) {
        assert!(inner.runs.iter().all(|run| outer.runs.contains(run)));
        assert!(
            inner
                .flushes
                .iter()
                .all(|flush| outer.flushes.contains(flush))
        );
        assert!(
            inner
                .anchors
                .iter()
                .all(|anchor| outer.anchors.contains(anchor))
        );
    }

    fn is_output_region(kind: ExecutionRegionKind) -> bool {
        matches!(
            kind,
            ExecutionRegionKind::ManLiteralBegin
                | ExecutionRegionKind::ManLiteralEnd
                | ExecutionRegionKind::MdocDisplayFilled
                | ExecutionRegionKind::MdocDisplayUnfilled
                | ExecutionRegionKind::MdocDisplayLiteral
                | ExecutionRegionKind::MdocDisplayRagged
                | ExecutionRegionKind::MdocDisplayCentered
                | ExecutionRegionKind::MdocDisplayOneLine
                | ExecutionRegionKind::MdocDisplayOneLineLiteral
                | ExecutionRegionKind::CenteredLines
                | ExecutionRegionKind::RightJustifiedLines
        )
    }

    fn restores_terminal_flags(kind: ExecutionRegionKind) -> bool {
        matches!(
            kind,
            ExecutionRegionKind::MdocDisplayFilled
                | ExecutionRegionKind::MdocDisplayUnfilled
                | ExecutionRegionKind::MdocDisplayLiteral
                | ExecutionRegionKind::MdocDisplayRagged
                | ExecutionRegionKind::MdocDisplayCentered
                | ExecutionRegionKind::MdocDisplayOneLine
                | ExecutionRegionKind::MdocDisplayOneLineLiteral
                | ExecutionRegionKind::CenteredLines
                | ExecutionRegionKind::RightJustifiedLines
        )
    }

    fn assert_native_region_facts(
        report: &NativeExecutionReport,
        projection: &NativeProjection,
        path: &str,
    ) {
        for region in &projection.regions {
            let wrapper = &report.wrappers()[region.wrapper as usize];
            assert_eq!(wrapper.node, Some(region.owner), "{path}");
            assert_eq!(
                wrapper.enter_atom..wrapper.leave_atom,
                region.atoms,
                "{path}"
            );
            assert!(
                region.runs.iter().all(|run| region.atoms.start
                    <= *run.atoms.first().unwrap_or(&region.atoms.start)
                    && run.atoms.last().is_none_or(|atom| *atom < region.atoms.end)),
                "{path}: {:?}",
                region.kind
            );
            if restores_terminal_flags(region.kind) {
                assert_eq!(
                    region.state_before, region.state_after,
                    "{path}: {:?} must restore its terminal flags",
                    region.kind
                );
            }
            if is_output_region(region.kind) {
                assert!(
                    !region.flushes.is_empty() || !region.boundaries.is_empty(),
                    "{path}: {:?} must retain its native output boundary",
                    region.kind
                );
            }
            if matches!(
                region.kind,
                ExecutionRegionKind::MdocDisplayUnfilled | ExecutionRegionKind::MdocDisplayLiteral
            ) {
                assert!(region.runs.iter().any(|run| {
                    report.nodes()[run.node.0 as usize]
                        .flags
                        .contains(libmandoc_rs::ExecutionNodeFlags::NO_FILL)
                }));
            }
        }
    }

    fn assert_captured_control_facts(
        report: &NativeExecutionReport,
        projection: &NativeProjection,
        path: &str,
    ) {
        for region in projection.regions.iter().filter(|region| {
            matches!(
                region.kind,
                ExecutionRegionKind::CenteredLines | ExecutionRegionKind::RightJustifiedLines
            )
        }) {
            let macro_name = match region.kind {
                ExecutionRegionKind::CenteredLines => "ce",
                ExecutionRegionKind::RightJustifiedLines => "rj",
                _ => unreachable!(),
            };
            assert!(
                region.controls.iter().any(|control| {
                    report.nodes()[control.node.0 as usize]
                        .macro_name
                        .as_deref()
                        == Some(macro_name)
                }),
                "{path}: {:?}",
                region.kind
            );
            assert!(!region.runs.is_empty(), "{path}: {:?}", region.kind);
        }
    }

    fn assert_literal_display_target(
        report: &NativeExecutionReport,
        projection: &NativeProjection,
    ) {
        let target = report
            .anchors()
            .iter()
            .find(|anchor| {
                report.pool_bytes(anchor.target) == Some(b"literal-display-target".as_slice())
            })
            .expect("literal display target");
        let literal = projection
            .regions
            .iter()
            .find(|region| region.kind == ExecutionRegionKind::MdocDisplayLiteral)
            .expect("literal display region");
        assert!(literal.anchors.contains(&target.key));
    }

    #[test]
    fn projects_exact_native_mdoc_list_roles() {
        // Expected list lifecycles and target ownership were established with
        // the pinned CVS `print_mdoc_node()`/`termp_it_pre()`/`termp_it_post()`
        // execution path before this assertion was added.  In particular,
        // Xo/Xc remains part of the It head, column rows retain every BODY,
        // and targets are owned by their actual AST location rather than a
        // rendered device line.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "mdoc-list-lifecycle.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/mdoc-list-lifecycle.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        assert_mdoc_list_topology(&report.execution, &projection);
        assert_mdoc_list_targets(&report.execution, &projection);
    }

    #[test]
    fn projects_exact_native_man_block_roles() {
        // This exact fixture was first checked with the pinned CVS terminal,
        // tree, and lint renderers.  Fixed `print_man_node()` wraps the whole
        // block handler lifecycle; `.PD 0` only changes paragraph distance,
        // `TQ` remains its own owner, and an `RS` lifecycle inclusively owns
        // the nested `TP` while the closest definition retains its flushes.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "man-definition-lifecycle.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/man-definition-lifecycle.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        assert_eq!(
            projection
                .man_blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            [
                ExecutionManBlockKind::TaggedParagraph,
                ExecutionManBlockKind::AdditionalTag,
                ExecutionManBlockKind::TaggedParagraph,
                ExecutionManBlockKind::Paragraph,
                ExecutionManBlockKind::TaggedParagraph,
                ExecutionManBlockKind::TaggedParagraph,
                ExecutionManBlockKind::IndentedParagraph,
                ExecutionManBlockKind::HangingParagraph,
                ExecutionManBlockKind::RelativeIndent,
                ExecutionManBlockKind::TaggedParagraph,
                ExecutionManBlockKind::Paragraph,
                ExecutionManBlockKind::ParagraphP,
                ExecutionManBlockKind::ParagraphLp,
            ]
        );
        let text =
            |runs: &[NativeTextRun]| runs.iter().map(|run| run.text.as_str()).collect::<String>();
        let blocks = &projection.man_blocks;
        assert_eq!(text(&blocks[0].head.runs), "-a");
        assert_eq!(text(&blocks[1].head.runs), "--alpha");
        assert_eq!(text(&blocks[2].head.runs), "--beta");
        assert!(blocks[0].body.runs.is_empty());
        assert!(blocks[1].body.runs.is_empty());
        let shared_body = text(&blocks[2].body.runs);
        assert_eq!(shared_body, "Sharedbody.");
        assert_eq!(text(&blocks[4].head.runs), "-c");
        assert_eq!(text(&blocks[5].head.runs), "--charlie");
        assert!(blocks[4].body.runs.is_empty());
        assert_eq!(text(&blocks[5].body.runs), "Separatebody.");
        assert!(text(&blocks[6].head.runs).starts_with("1."));

        let relative = &blocks[8];
        let nested = &blocks[9];
        assert!(nested.runs.iter().all(|run| relative.runs.contains(run)));
        let nested_definition = projection
            .definitions
            .iter()
            .find(|definition| definition.owner == nested.owner)
            .expect("nested TP definition");
        assert!(
            nested
                .body
                .flushes
                .iter()
                .all(|flush| { nested_definition.body_flushes.contains(flush) })
        );
        assert_eq!(
            projection
                .definitions
                .iter()
                .filter(|definition| matches!(definition.macro_name.as_str(), "TP" | "TQ"))
                .count(),
            6
        );
        assert_eq!(projection.definitions.len(), 7);
        assert!(projection.definitions.iter().all(|definition| {
            definition.responsive.layout.placement == mant_ir::DefinitionPlacement::Fit
                && definition.responsive.layout.min_term_gap_columns == 1
        }));
        let numbered = projection
            .definitions
            .iter()
            .find(|definition| {
                definition.kind
                    == NativeDefinitionKind::Man(ExecutionManBlockKind::IndentedParagraph)
            })
            .expect("native IP definition");
        assert_eq!(numbered.responsive.layout.body_indent_columns, 4);
        assert!(
            projection
                .definitions
                .iter()
                .filter(|definition| {
                    matches!(
                        definition.kind,
                        NativeDefinitionKind::Man(
                            ExecutionManBlockKind::TaggedParagraph
                                | ExecutionManBlockKind::AdditionalTag
                        )
                    )
                })
                .all(|definition| definition.responsive.layout.body_indent_columns == 7)
        );
    }

    #[test]
    fn projects_native_display_synopsis_literal_and_capture_regions() {
        // These exact fixtures were first checked with the pinned CVS
        // `-Tlint`, `-Ttree`, and `-Tutf8` renderers.  The region boundaries
        // come from fixed CVS handler entry/exit in man_term.c, mdoc_term.c,
        // and roff_term.c; the projection only transfers their inclusive
        // native ownership to an immutable Rust fact graph.
        for (path, format, source, expected) in [
            (
                "display-control-man.1",
                InputFormat::Man,
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/display-control-man.1"
                )
                .as_slice(),
                vec![
                    ExecutionRegionKind::ManSynopsisSection,
                    ExecutionRegionKind::ManSynopsisCommand,
                    ExecutionRegionKind::ManLiteralBegin,
                    ExecutionRegionKind::ManLiteralEnd,
                    ExecutionRegionKind::CenteredLines,
                    ExecutionRegionKind::RightJustifiedLines,
                ],
            ),
            (
                "display-control-mdoc.1",
                InputFormat::Mdoc,
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/display-control-mdoc.1"
                )
                .as_slice(),
                vec![
                    ExecutionRegionKind::MdocSynopsisSection,
                    ExecutionRegionKind::MdocSynopsisItem,
                    ExecutionRegionKind::MdocSynopsisItem,
                    ExecutionRegionKind::MdocSynopsisItem,
                    ExecutionRegionKind::MdocSynopsisItem,
                    ExecutionRegionKind::MdocDisplayFilled,
                    ExecutionRegionKind::MdocDisplayUnfilled,
                    ExecutionRegionKind::MdocDisplayLiteral,
                    ExecutionRegionKind::MdocDisplayRagged,
                    ExecutionRegionKind::MdocDisplayCentered,
                    ExecutionRegionKind::MdocDisplayOneLine,
                    ExecutionRegionKind::MdocDisplayOneLineLiteral,
                    ExecutionRegionKind::CenteredLines,
                    ExecutionRegionKind::RightJustifiedLines,
                ],
            ),
        ] {
            let report = Parser::new(ParseOptions::default())
                .with_input_format(format)
                .with_mdoc_operating_system("ManT")
                .unwrap()
                .execute_bytes(path, source, ExecutionLimits::default())
                .unwrap();
            let projection = project(&report.document, &report.execution);
            assert_eq!(
                projection
                    .regions
                    .iter()
                    .map(|region| region.kind)
                    .collect::<Vec<_>>(),
                expected,
                "{path}"
            );
            assert_native_region_facts(&report.execution, &projection, path);
            assert_captured_control_facts(&report.execution, &projection, path);

            if format == InputFormat::Mdoc {
                assert_literal_display_target(&report.execution, &projection);
            }
        }
    }

    #[test]
    fn projects_overlapping_regions_with_inclusive_native_ownership() {
        // The exact source was checked with pinned CVS lint/tree/UTF-8 before
        // this assertion.  A ce/rj node allocated while mdoc_state.c keeps
        // SYNOPSIS is both one synopsis item and one captured-control region.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "display-control-mdoc-synopsis-overlap.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/display-control-mdoc-synopsis-overlap.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        assert_eq!(
            projection
                .regions
                .iter()
                .map(|region| region.kind)
                .collect::<Vec<_>>(),
            [
                ExecutionRegionKind::MdocSynopsisSection,
                ExecutionRegionKind::MdocSynopsisItem,
                ExecutionRegionKind::CenteredLines,
                ExecutionRegionKind::MdocSynopsisItem,
                ExecutionRegionKind::RightJustifiedLines,
                ExecutionRegionKind::MdocSynopsisItem,
                ExecutionRegionKind::MdocSynopsisItem,
                ExecutionRegionKind::MdocSynopsisItem,
            ]
        );
        for pair in [
            projection.regions[1..3].as_ref(),
            projection.regions[3..5].as_ref(),
        ] {
            let item = &pair[0];
            let capture = &pair[1];
            assert_eq!(item.owner, capture.owner);
            assert!(capture.runs.iter().all(|run| item.runs.contains(run)));
            assert!(
                capture
                    .flushes
                    .iter()
                    .all(|flush| item.flushes.contains(flush))
            );
            assert!(
                capture
                    .boundaries
                    .iter()
                    .all(|boundary| item.boundaries.contains(boundary))
            );
            assert!(
                capture
                    .controls
                    .iter()
                    .all(|control| item.controls.contains(control))
            );
            assert!(!capture.runs.is_empty());
            assert!(!capture.flushes.is_empty());
            assert!(!capture.boundaries.is_empty());
            assert!(!capture.controls.is_empty());
        }
        let section = &projection.regions[0];
        assert!(
            projection.regions[1..]
                .iter()
                .flat_map(|region| &region.runs)
                .all(|run| section.runs.contains(run))
        );
        let item_region = projection
            .regions
            .iter()
            .find(|region| {
                region.kind == ExecutionRegionKind::MdocSynopsisItem
                    && report.execution.nodes()[region.owner.0 as usize]
                        .macro_name
                        .as_deref()
                        == Some("It")
            })
            .expect("synopsis list item region");
        let parent = report.execution.wrappers()[report.execution.wrappers()
            [item_region.wrapper as usize]
            .parent
            .unwrap() as usize]
            .kind;
        assert_eq!(parent, ExecutionWrapperKind::MdocListItem);
    }

    #[test]
    fn projects_nested_mdoc_list_intervals_without_changing_definition_ownership() {
        // The pinned CVS `print_mdoc_node()` recursion keeps both nested Bl
        // blocks inside the outer It BODY lifecycle.  Consequently native
        // list intervals are inclusive, while the definition consumer keeps
        // using the closest definition owner for each executed node.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "mdoc-list-nested.1",
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/mdoc-list-nested.1"),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let item_named = |name: &str| {
            projection
                .mdoc_lists
                .iter()
                .flat_map(|list| &list.items)
                .find(|item| {
                    item.head
                        .runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>()
                        == name
                })
                .expect("named native list item")
        };
        let outer = item_named("outer");
        let inner = item_named("inner");
        let bullet = projection
            .mdoc_lists
            .iter()
            .find(|list| list.kind == ExecutionMdocListKind::Bullet)
            .and_then(|list| list.items.first())
            .expect("nested bullet item");
        let outer_body = outer.bodies.first().expect("outer tag body");
        let inner_body = inner.bodies.first().expect("inner tag body");
        let bullet_body = bullet.bodies.first().expect("nested bullet body");

        assert_mdoc_segment_includes(outer_body, bullet_body);
        assert_mdoc_segment_includes(outer_body, inner_body);

        let outer_definition = projection
            .definitions
            .iter()
            .find(|definition| definition.owner == outer.owner)
            .expect("outer native definition");
        let inner_definition = projection
            .definitions
            .iter()
            .find(|definition| definition.owner == inner.owner)
            .expect("inner native definition");
        assert!(
            bullet_body
                .flushes
                .iter()
                .all(|flush| outer_definition.body_flushes.contains(flush))
        );
        assert!(
            inner_body
                .flushes
                .iter()
                .all(|flush| inner_definition.body_flushes.contains(flush))
        );
        assert!(
            inner_body
                .flushes
                .iter()
                .all(|flush| !outer_definition.body_flushes.contains(flush))
        );
    }

    #[test]
    fn projects_large_mdoc_lists_without_scanning_wrappers_per_item() {
        // The repeated It grammar was first checked with the pinned CVS
        // reference.  This scale regression protects the pre-indexed
        // node-wrapper lookup used by native list projection.
        const ITEM_COUNT: usize = 4_096;
        let mut source =
            String::from(".Dd September 19, 2026\n.Dt K14S 1\n.Os\n.Sh LISTS\n.Bl -bullet\n");
        for item in 0..ITEM_COUNT {
            source.push_str(".It\nitem-");
            source.push_str(&item.to_string());
            source.push('\n');
        }
        source.push_str(".El\n");
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "mdoc-list-scale.1",
                source.as_bytes(),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let list = projection
            .mdoc_lists
            .iter()
            .find(|list| list.kind == ExecutionMdocListKind::Bullet)
            .expect("native bullet list");
        assert_eq!(list.items.len(), ITEM_COUNT);
    }

    fn assert_control_origin_matrix(
        path: &str,
        format: InputFormat,
        projection: &NativeProjection,
    ) {
        let lines = if format == InputFormat::Man {
            [5, 6, 7, 8, 9, 10, 13, 15, 16, 18, 20, 22, 24]
        } else {
            [8, 9, 10, 11, 12, 13, 16, 18, 19, 21, 23, 25, 27]
        };
        let requests = [
            libmandoc_rs::ExecutionControlRequest::MarginCharacter,
            libmandoc_rs::ExecutionControlRequest::MarginCharacter,
            libmandoc_rs::ExecutionControlRequest::VerticalSpace,
            libmandoc_rs::ExecutionControlRequest::TemporaryIndent,
            libmandoc_rs::ExecutionControlRequest::NoFill,
            libmandoc_rs::ExecutionControlRequest::Fill,
            libmandoc_rs::ExecutionControlRequest::MarginCharacter,
            libmandoc_rs::ExecutionControlRequest::Break,
            libmandoc_rs::ExecutionControlRequest::MarginCharacter,
            libmandoc_rs::ExecutionControlRequest::VerticalSpace,
            libmandoc_rs::ExecutionControlRequest::TemporaryIndent,
            libmandoc_rs::ExecutionControlRequest::NoFill,
            libmandoc_rs::ExecutionControlRequest::Fill,
        ];
        assert_eq!(
            projection
                .controls
                .iter()
                .map(|fact| (fact.line, fact.control.request, fact.provenance))
                .collect::<Vec<_>>(),
            lines
                .into_iter()
                .zip(requests)
                .map(|(line, request)| (line, request, NativeControlProvenance::Authored))
                .collect::<Vec<_>>()
        );
        assert!(projection.controls.iter().all(|fact| {
            fact.source == std::path::Path::new(path)
                && fact.control.boundaries.end - fact.control.boundaries.start
                    == u32::try_from(fact.boundaries.len()).expect("bounded boundary count")
        }));
    }

    fn assert_control_effect_matrix(projection: &NativeProjection) {
        assert_eq!(
            projection
                .controls
                .iter()
                .map(|fact| fact.control.line_after - fact.control.line_before)
                .collect::<Vec<_>>(),
            [0, 0, 2, 0, 0, 0, 0, 1, 0, 2, 1, 1, 0]
        );
        assert_eq!(
            projection
                .controls
                .iter()
                .map(|fact| fact.control.temporary_indent_after)
                .collect::<Vec<_>>(),
            [0, 0, 0, 72, 72, 72, 0, 0, 0, 0, 72, 0, 0]
        );
        assert_eq!(
            projection
                .controls
                .iter()
                .map(|fact| {
                    let ended = fact
                        .boundaries
                        .iter()
                        .filter(|boundary| boundary.effect == BoundaryEffect::EndedLine)
                        .count();
                    let spaced = fact
                        .boundaries
                        .iter()
                        .filter(|boundary| boundary.effect == BoundaryEffect::AddedVerticalSpace)
                        .count();
                    (fact.boundaries.len(), ended, spaced)
                })
                .collect::<Vec<_>>(),
            [
                (0, 0, 0),
                (0, 0, 0),
                (7, 2, 2),
                (1, 0, 0),
                (1, 0, 0),
                (1, 0, 0),
                (0, 0, 0),
                (3, 3, 0),
                (0, 0, 0),
                (6, 4, 1),
                (3, 3, 0),
                (3, 3, 0),
                (1, 0, 0),
            ]
        );
    }

    fn assert_control_visible_lines(format: InputFormat, projection: &NativeProjection) {
        let description = if format == InputFormat::Man {
            "k11-controls - native control execution probe"
        } else {
            "k11-controls – native control execution probe"
        };
        assert_eq!(
            projection.visible_lines,
            [
                "NAME",
                description,
                "EMPTY REQUESTS",
                "SUCCESSORS",
                "ALPHA BETA",
                "GAMMA",
                "DELTA",
                "EPSILON",
                "ZETA",
                "ETA",
            ],
            "the staged projection must preserve the explicit fixed-CVS visible-line oracle"
        );
    }

    fn assert_nested_control_projection() {
        let nested = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "control-nested-man.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/control-nested-man.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let nested_projection = project(&nested.document, &nested.execution);
        let direct_boundary_keys = nested_projection
            .controls
            .iter()
            .flat_map(|fact| fact.boundaries.iter().map(|boundary| boundary.key))
            .collect::<Vec<_>>();
        let mut projected_boundary_keys = direct_boundary_keys;
        projected_boundary_keys.sort_unstable();
        let mut report_boundary_keys = nested
            .execution
            .boundaries()
            .iter()
            .filter(|boundary| boundary.control.is_some())
            .map(|boundary| boundary.key)
            .collect::<Vec<_>>();
        report_boundary_keys.sort_unstable();
        assert_eq!(projected_boundary_keys, report_boundary_keys);
        assert!(nested_projection.controls.iter().any(|fact| {
            fact.control.request == libmandoc_rs::ExecutionControlRequest::Break
                && fact.control.parent.is_some()
                && fact
                    .boundaries
                    .iter()
                    .all(|boundary| boundary.control == Some(fact.control.key))
        }));
        assert!(
            nested_projection
                .controls
                .iter()
                .filter(|fact| fact.control.parent.is_some())
                .all(|fact| !fact.boundaries.is_empty())
        );
    }

    #[test]
    fn direct_control_boundaries_are_partitioned_once_across_many_controls() {
        // This exact generated input was rendered by the pinned CVS binary
        // before the assertion was written.  While a live `.ce` causes the
        // next `.ce` to be reattached as a sibling in fixed CVS `roff_onearg`,
        // all 32 centering and 32 vertical-space controls remain observable.
        // The counting iterator pins the projection contract: every report
        // boundary is visited exactly once, independent of control ancestry.
        let mut source = String::from(".TH PROBE 1\n.SH DESCRIPTION\n");
        source.push_str(&".ce 2\n".repeat(32));
        source.push_str(&".sp 1\n".repeat(32));
        source.push_str("VISIBLE\n");
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "control-deep-boundaries-man.1",
                source.as_bytes(),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);

        let visits = std::cell::Cell::new(0usize);
        let partitioned = partition_control_boundaries(
            report.execution.controls().len(),
            report.execution.boundaries().iter().inspect(|_| {
                visits.set(visits.get() + 1);
            }),
        );
        assert_eq!(visits.get(), report.execution.boundaries().len());
        assert_eq!(
            partitioned.iter().map(Vec::len).sum::<usize>(),
            report
                .execution
                .boundaries()
                .iter()
                .filter(|boundary| boundary.control.is_some())
                .count()
        );

        assert!(projection.controls.len() >= 64);
        let mut projected = projection
            .controls
            .iter()
            .flat_map(|fact| fact.boundaries.iter())
            .map(|boundary| (boundary.key, boundary.control))
            .collect::<Vec<_>>();
        let mut expected = report
            .execution
            .boundaries()
            .iter()
            .filter(|boundary| boundary.control.is_some())
            .map(|boundary| (boundary.key, boundary.control))
            .collect::<Vec<_>>();
        projected.sort_unstable();
        expected.sort_unstable();
        assert_eq!(projected, expected);
        assert_eq!(
            projection
                .controls
                .iter()
                .map(|fact| fact.boundaries.len())
                .sum::<usize>(),
            expected.len()
        );
    }

    #[test]
    fn projects_native_heading_identity_content_and_body_handoff() {
        // Both fixtures were rendered with the pinned CVS terminal before
        // these expectations were written.  The mdoc fixture also used the
        // pinned HTML renderer to confirm `NEXT SECTION` resolves to
        // `#NEXT_SECTION` even though `.Sm off` displays `NEXTSECTION`.
        let mdoc = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "heading-execution-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/heading-execution-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projected = project(&mdoc.document, &mdoc.execution);
        assert_mdoc_heading_projection(&mdoc.execution, &projected);

        let man = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "heading-execution-man.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/heading-execution-man.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projected = project(&man.document, &man.execution);
        assert_man_heading_projection(&projected);
    }

    #[test]
    fn projects_authored_controls_with_native_boundary_effects() {
        for (path, format, source) in [
            (
                "control-effects-man.1",
                InputFormat::Man,
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/control-effects-man.1"
                )
                .as_slice(),
            ),
            (
                "control-effects-mdoc.1",
                InputFormat::Mdoc,
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/control-effects-mdoc.1"
                )
                .as_slice(),
            ),
        ] {
            let report = Parser::new(ParseOptions::default())
                .with_input_format(format)
                .with_mdoc_operating_system("ManT")
                .unwrap()
                .execute_bytes(path, source, ExecutionLimits::default())
                .unwrap();
            let projection = project(&report.document, &report.execution);
            assert_eq!(projection.controls.len(), report.execution.controls().len());
            assert_control_origin_matrix(path, format, &projection);
            assert_control_effect_matrix(&projection);
            assert_control_visible_lines(format, &projection);
        }

        assert_nested_control_projection();

        assert_generated_control_provenance();
    }

    fn assert_generated_control_provenance() {
        let generated = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "control-generated-man.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/control-generated-man.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        assert!(
            project(&generated.document, &generated.execution)
                .controls
                .iter()
                .any(|fact| fact.provenance == NativeControlProvenance::Generated)
        );
    }

    #[test]
    fn projects_zero_output_formatter_words_without_inventing_ir_content() {
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "empty-word-man.1",
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/empty-word-man.1"),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let empty = projection
            .words
            .iter()
            .find(|word| word.operand.is_empty())
            .expect("private projection retains the explicit empty formatter word");
        assert_eq!(empty.source, std::path::Path::new("empty-word-man.1"));
        assert!(empty.atoms.is_empty());
        assert_eq!(empty.role, AtomRole::Authored);
        assert!(
            projection
                .visible_lines
                .iter()
                .any(|line| line.trim() == "AB"),
            "the zero-output word must not invent visible IR content"
        );
    }

    #[test]
    fn projects_definition_roles_and_semantic_annotations_without_c_borrows() {
        for (path, format, source, definition_macro) in [
            (
                "annotated-man.1",
                InputFormat::Man,
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/annotated-man.1")
                    .as_slice(),
                "TP",
            ),
            (
                "annotated-mdoc.1",
                InputFormat::Mdoc,
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/annotated-mdoc.1")
                    .as_slice(),
                "It",
            ),
        ] {
            let report = Parser::new(ParseOptions::default())
                .with_input_format(format)
                .with_mdoc_operating_system("ManT")
                .unwrap()
                .execute_bytes(path, source, ExecutionLimits::default())
                .unwrap();
            let projection = project(&report.document, &report.execution);
            assert!(projection.runs.iter().any(|run| {
                run.reference.is_some()
                    && (run.text.contains("linked") || run.text.contains("label"))
            }));
            assert!(!projection.runs.iter().any(|run| {
                run.reference.is_some() && run.text.contains("https://example.org/manual")
            }));
            let definition = projection
                .definitions
                .iter()
                .find(|definition| definition.macro_name == definition_macro)
                .expect("native definition structure");
            assert!(!definition.head_flushes.is_empty());
            assert!(!definition.body_flushes.is_empty());
            assert!(
                definition
                    .head_flushes
                    .iter()
                    .chain(&definition.body_flushes)
                    .all(|flush| flush.accepted.end <= flush.scanned.end)
            );
            let reference = projection
                .references
                .iter()
                .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
                .expect("owned URI reference");
            assert_eq!(reference.primary, b"https://example.org/manual");
            assert!(!reference.atoms.is_empty());
            if format == InputFormat::Mdoc {
                let anchor = projection
                    .anchors
                    .iter()
                    .find(|anchor| anchor.target == b"custom-target")
                    .expect("owned native anchor");
                assert!(anchor.device_line > 0);
                assert!(
                    usize::try_from(anchor.atom_cursor)
                        .is_ok_and(|cursor| cursor <= report.execution.atoms().len())
                );
                assert!(
                    usize::try_from(anchor.fragment_cursor)
                        .is_ok_and(|cursor| cursor <= report.execution.fragments().len())
                );
            }
            drop(report);
            assert!(
                !projection.runs.is_empty(),
                "projection must be fully owned"
            );
            assert_eq!(
                projection.references[0].primary,
                b"https://example.org/manual"
            );
            assert_eq!(
                projection.origins[projection.references[0].owner_node.0 as usize]
                    .macro_name
                    .as_deref(),
                Some(if format == InputFormat::Man {
                    "UR"
                } else {
                    "Lk"
                })
            );
        }
    }

    #[test]
    fn mdoc_definition_projection_uses_the_native_list_subtype() {
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "list-roles-mdoc.1",
                include_bytes!("../../../libmandoc-rs/tests/fixtures/execution/list-roles-mdoc.1"),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        assert_eq!(projection.definitions.len(), 1);
        let definition = &projection.definitions[0];
        assert_eq!(definition.macro_name, "It");
        assert!(!definition.head_flushes.is_empty());
        assert!(!definition.body_flushes.is_empty());
        assert!(
            projection
                .visible_lines
                .iter()
                .any(|line| line.contains("Bullet body."))
        );
    }

    fn responsive_definition_item(layout: mant_ir::DefinitionLayout) -> mant_ir::DefinitionItem {
        mant_ir::DefinitionItem {
            source: None,
            entry: None,
            terms: vec![vec![mant_ir::Inline::Text {
                value: "alpha".to_owned(),
            }]],
            description: vec![mant_ir::Block::Paragraph {
                children: vec![mant_ir::Inline::Text {
                    value: "description".to_owned(),
                }],
                layout: mant_ir::LayoutHint::default(),
                source: None,
            }],
            layout,
        }
    }

    fn is_within(
        report: &NativeExecutionReport,
        mut node: ExecutionNodeKey,
        ancestor: ExecutionNodeKey,
    ) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            let Some(parent) = report.nodes()[node.0 as usize].parent else {
                return false;
            };
            node = parent;
        }
    }

    #[test]
    fn projects_native_definition_contracts_into_responsive_ir_policy() {
        // The exact fixture was rendered with the pinned CVS terminal at
        // 32/78/120 columns before this assertion was written.  Fixed CVS
        // `termp_it_pre()` supplies field origins and continuation flags;
        // only this codec layer maps typed list styles to IR placement.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "mdoc-list-lifecycle.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/mdoc-list-lifecycle.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let definition = |kind| {
            projection
                .definitions
                .iter()
                .find(|definition| definition.kind == NativeDefinitionKind::Mdoc(kind))
                .expect("responsive mdoc definition")
        };
        let tag = definition(ExecutionMdocListKind::Tag);
        assert_eq!(
            tag.responsive.layout.placement,
            mant_ir::DefinitionPlacement::Fit
        );
        assert_eq!(tag.responsive.layout.body_indent_columns, 10);
        assert_eq!(tag.responsive.layout.min_term_gap_columns, 2);
        assert_eq!(
            definition(ExecutionMdocListKind::Hang)
                .responsive
                .layout
                .placement,
            mant_ir::DefinitionPlacement::RunIn
        );
        assert_eq!(
            definition(ExecutionMdocListKind::Overhang)
                .responsive
                .layout,
            mant_ir::DefinitionLayout {
                placement: mant_ir::DefinitionPlacement::Stacked,
                body_indent_columns: 0,
                min_term_gap_columns: 0,
                term_continuation_indent_columns: 0,
                fit_constraint: None,
                spacing_before_lines: None,
            }
        );
        assert_eq!(
            definition(ExecutionMdocListKind::Inset)
                .responsive
                .layout
                .min_term_gap_columns,
            1
        );
        assert_eq!(
            definition(ExecutionMdocListKind::Diagnostic)
                .responsive
                .layout
                .min_term_gap_columns,
            2
        );

        let item = responsive_definition_item(tag.responsive.layout);
        let plan = mant_ir::geometry::definition_placement_plan(&item, 0);
        let narrow = plan.resolve(Some(6));
        let wide = plan.resolve(Some(40));
        assert!(!narrow.run_in);
        assert!(wide.run_in);
        assert_eq!(wide.body_origin_columns, 10);
        let translated = plan.translated(7).resolve(Some(40));
        assert!(translated.run_in);
        assert_eq!(translated.body_origin_columns, wide.body_origin_columns + 7);
        assert_eq!(
            translated.first_description_origin_columns,
            wide.first_description_origin_columns + 7
        );
    }

    #[test]
    fn projects_fractional_absolute_origins_before_computing_relative_indent() {
        // This exact fixture was rendered with the pinned CVS `-Tutf8` and
        // `-Tlint` frontends before this assertion was written.  Fixed CVS
        // `ascii_advance()` rounds the absolute 132-BU and 192-BU destinations
        // to columns 5 and 8; rounding their 60-BU delta would incorrectly
        // produce only two columns.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-responsive-fractional.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-responsive-fractional.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one fractional responsive definition");
        };
        assert_eq!(definition.responsive.label_origin_columns, 5);
        assert_eq!(definition.responsive.layout.body_indent_columns, 3);
        assert_eq!(
            definition
                .responsive
                .layout
                .fit_constraint
                .expect("tie-phase fit constraint")
                .origin_phase_basic_units,
            12
        );

        let mut item = responsive_definition_item(definition.responsive.layout);
        item.terms = vec![vec![mant_ir::Inline::Text {
            value: "A".to_owned(),
        }]];
        let plan = mant_ir::geometry::definition_placement_plan(&item, 0);
        let wide = plan.resolve(Some(40));
        assert!(wide.run_in);
        assert_eq!(wide.body_origin_columns, 3);
        let translated = plan.translated(7).resolve(Some(40));
        assert_eq!(translated.body_origin_columns, 10);
        assert_eq!(translated.first_description_origin_columns, 10);

        let above_half = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-fractional-fit-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-fractional-fit-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&above_half.document, &above_half.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one above-half responsive definition");
        };
        assert_eq!(
            definition
                .responsive
                .layout
                .fit_constraint
                .expect("above-half fit constraint")
                .origin_phase_basic_units,
            13
        );
    }

    #[test]
    fn projects_the_executed_head_field_after_a_line_length_change() {
        // The pristine pinned CVS reference ran this exact fixture before the
        // assertion was written. `roff_term_pre_ll()` updates maxrmargin, but
        // `term_flushln()` still observes the tag handler's ten-cell field at
        // its five-cell origin; projection must use that executed flush rather
        // than reconstructing either value from the phase-entry snapshot.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-head-line-length-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-head-line-length-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one line-length definition");
        };
        let constraint = definition
            .responsive
            .layout
            .fit_constraint
            .expect("executed head fit constraint");
        assert_eq!(constraint.field_basic_units, 10 * 24);
        assert_eq!(constraint.origin_phase_basic_units, 0);
        assert_eq!(constraint.cell_basic_units.get(), 24);
    }

    #[test]
    fn projects_native_term_continuations_and_fit_only_trailing_space() {
        // Both exact fixtures were rendered with pinned CVS `-Tutf8` at the
        // asserted widths and `-Tlint` before this test was written.  In
        // fixed CVS `term_flushln()`, BRIND restarts wrapped tag text at the
        // field end, while BRTRSP counts discarded authored tail whitespace
        // only for the Fit decision.
        let boundaries = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-responsive-boundaries.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-responsive-boundaries.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&boundaries.document, &boundaries.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one responsive definition");
        };
        assert_eq!(
            definition
                .responsive
                .layout
                .term_continuation_indent_columns,
            definition.responsive.layout.body_indent_columns
        );

        let trailing = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-trailing-fit-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-trailing-fit-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&trailing.document, &trailing.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one trailing-fit definition");
        };
        let constraint = definition
            .responsive
            .layout
            .fit_constraint
            .expect("native Fit constraint");
        assert_eq!(constraint.fit_content_basic_units, 9 * 24);
        assert_eq!(constraint.field_basic_units, 10 * 24);
        assert_eq!(constraint.cell_basic_units.get(), 24);
        let mut with_tail = responsive_definition_item(definition.responsive.layout);
        with_tail.terms = vec![vec![mant_ir::Inline::Text {
            value: "12345678".to_owned(),
        }]];
        let mut without_tail = with_tail.clone();
        without_tail.layout.fit_constraint = Some(mant_ir::DefinitionFitConstraint {
            fit_content_basic_units: 8 * 24,
            ..constraint
        });
        assert!(!mant_ir::geometry::definition_placement(&with_tail, 5, Some(15)).run_in);
        assert!(mant_ir::geometry::definition_placement(&without_tail, 5, Some(15)).run_in);
    }

    #[test]
    fn materializes_custom_native_tabs_before_portable_geometry() {
        // This exact fixture was rendered with the pristine pinned CVS
        // reference at width 30 before the assertion was written.  Its active
        // `.ta 4n T 4n` stop renders `12345<TAB>X` as `12345   X`, while a
        // generic absolute eight-column reader would place the tab differently
        // because the label itself begins at column five.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-custom-tab-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-custom-tab-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one custom-tab definition");
        };
        let [field] = definition.responsive.term_tab_fields.as_slice() else {
            panic!("one native tab field");
        };
        assert_eq!(
            field.rows,
            [layout::NativeTermTabRow {
                row_epoch: 0,
                destinations_columns: vec![8],
            }]
        );

        let term = vec![mant_ir::Inline::Text {
            value: "12345\tX".to_owned(),
        }];
        let projected = definition.project_field_term(field.buffer_generation, &term);
        assert_eq!(
            projected,
            vec![mant_ir::Inline::Text {
                value: "12345   X".to_owned(),
            }]
        );
        let mut item = responsive_definition_item(definition.responsive.layout);
        item.terms = vec![projected];
        let resolved = mant_ir::geometry::definition_placement(&item, 0, Some(30));
        assert!(resolved.run_in);
        assert_eq!(resolved.final_label_width_columns, Some(9));
    }

    #[test]
    fn materializes_tabs_on_their_executed_word_end_break_rows() {
        // Both exact fixtures were rendered with the pristine pinned CVS
        // reference at width 30 and checked with `-Tlint` before these
        // assertions were written.  `term_flushln()` advances the carried tab
        // offset after each `\p` row (term.c:692-696), so the first continuation
        // reaches column five and the second reaches column three relative to
        // their respective logical row origins. Style, link and anchor splits
        // do not create formatter rows or new tab identities.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "logical-tabs-multi-row-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/logical-tabs-multi-row-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one multi-row tab definition");
        };
        let [field] = definition.responsive.term_tab_fields.as_slice() else {
            panic!("one native tab field");
        };
        let rows = field
            .rows
            .iter()
            .map(|row| (row.row_epoch, row.destinations_columns.as_slice()))
            .collect::<Vec<_>>();
        assert_eq!(rows, [(1, &[5][..]), (2, &[3][..])]);

        let target = mant_ir::LinkTarget::External {
            uri: "https://example.invalid/".to_owned(),
        };
        let term = vec![
            mant_ir::Inline::Text {
                value: "A".to_owned(),
            },
            mant_ir::Inline::LineBreak,
            mant_ir::Inline::Strong {
                children: vec![mant_ir::Inline::Text {
                    value: "B".to_owned(),
                }],
            },
            mant_ir::Inline::Anchor {
                id: "row-one".into(),
                fragment_aliases: Vec::new(),
                owner_source: None,
            },
            mant_ir::Inline::Emphasis {
                children: vec![mant_ir::Inline::Text {
                    value: "\tC".to_owned(),
                }],
            },
            mant_ir::Inline::LineBreak,
            mant_ir::Inline::Link {
                target: target.clone(),
                title: None,
                children: vec![mant_ir::Inline::Text {
                    value: "D\tE".to_owned(),
                }],
            },
        ];
        let projected = definition.project_field_term(field.buffer_generation, &term);
        assert_eq!(
            projected,
            vec![
                mant_ir::Inline::Text {
                    value: "A".to_owned(),
                },
                mant_ir::Inline::LineBreak,
                mant_ir::Inline::Strong {
                    children: vec![mant_ir::Inline::Text {
                        value: "B".to_owned(),
                    }],
                },
                mant_ir::Inline::Anchor {
                    id: "row-one".into(),
                    fragment_aliases: Vec::new(),
                    owner_source: None,
                },
                mant_ir::Inline::Emphasis {
                    children: vec![mant_ir::Inline::Text {
                        value: "    C".to_owned(),
                    }],
                },
                mant_ir::Inline::LineBreak,
                mant_ir::Inline::Link {
                    target,
                    title: None,
                    children: vec![mant_ir::Inline::Text {
                        value: "D  E".to_owned(),
                    }],
                },
            ]
        );

        let mut mismatched = term.clone();
        mismatched.push(mant_ir::Inline::Text {
            value: "\textra".to_owned(),
        });
        assert_eq!(
            definition.project_field_term(field.buffer_generation, &mismatched),
            mismatched
        );
    }

    #[test]
    fn binds_tabs_to_each_native_logical_field_before_materialization() {
        // The pristine pinned CVS reference rendered this exact fixture at
        // width 40 before this assertion was written.  Its `.sp` executes a
        // real field boundary: the two `term_flushln()` invocations therefore
        // own distinct buffer generations and independently render
        // `12345   X` and `B   C` (term.c:589-821).  Public IR terms do not
        // retain that execution identity, so the projection API requires the
        // materializer to present the matching generation explicitly.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "logical-tabs-hard-fields-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/logical-tabs-hard-fields-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one hard-boundary definition");
        };
        let [first, second] = definition.responsive.term_tab_fields.as_slice() else {
            panic!("two independently identified native tab fields");
        };
        assert_ne!(first.buffer_generation, second.buffer_generation);
        assert_eq!(
            first.rows,
            [layout::NativeTermTabRow {
                row_epoch: 0,
                destinations_columns: vec![8],
            }]
        );
        assert_eq!(
            second.rows,
            [layout::NativeTermTabRow {
                row_epoch: 0,
                destinations_columns: vec![4],
            }]
        );
        assert_eq!(
            definition.project_field_term(
                first.buffer_generation,
                &[mant_ir::Inline::Text {
                    value: "12345\tX".to_owned(),
                }],
            ),
            [mant_ir::Inline::Text {
                value: "12345   X".to_owned(),
            }]
        );
        assert_eq!(
            definition.project_field_term(
                second.buffer_generation,
                &[mant_ir::Inline::Text {
                    value: "B\tC".to_owned(),
                }],
            ),
            [mant_ir::Inline::Text {
                value: "B   C".to_owned(),
            }]
        );
        assert_eq!(
            definition.project_field_term(
                u32::MAX,
                &[mant_ir::Inline::Text {
                    value: "B\tC".to_owned(),
                }],
            ),
            [mant_ir::Inline::Text {
                value: "B\tC".to_owned(),
            }]
        );
    }

    #[test]
    fn rounds_hard_row_tabs_at_their_fractional_continuation_origin() {
        // The pristine pinned CVS reference was run before this assertion.
        // Native execution reports origin 133 BU, BRIND continuation origin
        // 361 BU, tab destination 60 BU and cell width 24 BU. The fixed CVS
        // half-down conversion must therefore use the continuation origin:
        // round(361 + 60) - round(361) = 18 - 15 = 3 columns. Reusing the
        // first-row origin would incorrectly produce two columns.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "logical-tabs-fractional-row-origin-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/logical-tabs-fractional-row-origin-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one fractional-row tab definition");
        };
        let [field] = definition.responsive.term_tab_fields.as_slice() else {
            panic!("one native tab field");
        };
        assert_eq!(
            field.rows,
            [layout::NativeTermTabRow {
                row_epoch: 1,
                destinations_columns: vec![3],
            }]
        );
        assert_eq!(
            definition.project_field_term(
                field.buffer_generation,
                &[
                    mant_ir::Inline::Text {
                        value: "A".to_owned(),
                    },
                    mant_ir::Inline::LineBreak,
                    mant_ir::Inline::Text {
                        value: "B\tC".to_owned(),
                    },
                ]
            ),
            vec![
                mant_ir::Inline::Text {
                    value: "A".to_owned(),
                },
                mant_ir::Inline::LineBreak,
                mant_ir::Inline::Text {
                    value: "B  C".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn responsive_resolution_preserves_native_source_anchor_and_link_identity() {
        // This exact fixture was rendered with pinned CVS `-Ttree`, `-Tlint`,
        // and `-Tutf8` at 32/78/120 columns before this assertion was written.
        // The `.Tg` stays attached to the It head and the `.Lk` stays attached
        // to its body; changing reader width is therefore only geometry.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-responsive-metadata.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-responsive-metadata.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one metadata definition");
        };
        let owner = projection
            .origins
            .iter()
            .find(|origin| origin.key == definition.owner)
            .expect("definition source origin");
        assert_eq!(owner.line, 10);
        assert_eq!(
            owner.source,
            std::path::Path::new("definition-responsive-metadata.1")
        );

        let anchor = projection
            .anchors
            .iter()
            .find(|anchor| anchor.target == b"term-anchor")
            .expect("term anchor");
        assert!(is_within(&report.execution, anchor.node, definition.head));
        let link = projection
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
            .expect("body URI reference");
        assert_eq!(link.primary, b"https://example.org");
        assert!(is_within(
            &report.execution,
            link.owner_node,
            definition.body
        ));

        let source = mant_ir::SourceSpan {
            byte_range: None,
            line: owner.line,
            column: owner.column,
            end_line: None,
            end_column: None,
        };
        let item = mant_ir::DefinitionItem {
            source: Some(source),
            entry: None,
            terms: vec![vec![
                mant_ir::Inline::anchor_at("term-anchor", Some(source)),
                mant_ir::Inline::Text {
                    value: "-alpha".to_owned(),
                },
            ]],
            description: vec![mant_ir::Block::Paragraph {
                children: vec![mant_ir::Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: String::from_utf8(link.primary.clone()).unwrap(),
                    },
                    title: None,
                    children: vec![mant_ir::Inline::Text {
                        value: "label".to_owned(),
                    }],
                }],
                layout: mant_ir::LayoutHint::default(),
                source: Some(source),
            }],
            layout: definition.responsive.layout,
        };
        let original = item.clone();
        let plan = mant_ir::geometry::definition_placement_plan(
            &item,
            definition.responsive.label_origin_columns,
        );
        let narrow = plan.resolve(Some(6));
        let wide = plan.resolve(Some(40));
        let translated = plan.translated(7).resolve(Some(40));
        assert!(!narrow.run_in);
        assert!(wide.run_in);
        assert_eq!(translated.body_origin_columns, wide.body_origin_columns + 7);
        assert_eq!(item, original);
    }

    #[test]
    fn target_only_empty_definition_keeps_its_anchor_without_inventing_a_visible_row() {
        // Pinned CVS `-Ttree` retains `target-only` on the empty It head and
        // `-Tutf8` emits no definition row. `-Tlint` reports the expected
        // empty-head warning. The native handler still owns a field contract,
        // but an empty IR item must remain ineligible for run-in placement.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-target-only-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-target-only-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one empty native definition lifecycle");
        };
        assert!(
            projection
                .anchors
                .iter()
                .any(|anchor| anchor.target == b"target-only")
        );
        let empty = mant_ir::DefinitionItem {
            source: None,
            entry: None,
            terms: Vec::new(),
            description: Vec::new(),
            layout: definition.responsive.layout,
        };
        let resolved = mant_ir::geometry::definition_placement(
            &empty,
            definition.responsive.label_origin_columns,
            Some(120),
        );
        assert!(!resolved.run_in);
        assert_eq!(resolved.final_label_width_columns, None);
    }

    #[test]
    fn whitespace_only_native_head_does_not_invent_a_visible_term_row() {
        // The pristine pinned CVS reference renders this exact all-space HEAD
        // as no term row and starts BODY at the definition body origin.
        // `term.c::term_fill()` leaves ordinary spaces pending and returns an
        // empty field, while tabs and NBSP remain graph atoms.
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("definition-whitespace-head-mdoc.1"),
            include_bytes!(
                "../../../libmandoc-rs/tests/fixtures/execution/definition-whitespace-head-mdoc.1"
            ),
        )
        .unwrap();
        let item = document
            .sections
            .iter()
            .flat_map(|section| &section.blocks)
            .find_map(|block| match block {
                mant_ir::Block::DefinitionList { items, .. } => items.first(),
                _ => None,
            })
            .expect("whitespace definition item");
        assert_eq!(
            mant_ir::geometry::definition_run_in_width(&item.terms),
            None
        );
        assert!(item.terms.iter().flatten().all(|inline| {
            !matches!(inline, mant_ir::Inline::Text { value } if value.chars().all(|character| character == ' '))
        }));
    }

    #[test]
    fn keeps_native_wraps_soft_and_effective_control_breaks_hard() {
        // This exact source was first run with the pinned CVS `-Tutf8` and
        // `-Tlint` frontends.  `term_flushln()` may wrap the long head at the
        // fixed reference width, while the body `.br` owns an ended-line
        // boundary.  Only the latter is an IR hard boundary.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-responsive-boundaries.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-responsive-boundaries.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one projected responsive definition");
        };
        assert!(!definition.responsive.soft_flushes.is_empty());
        assert!(!definition.responsive.hard_boundaries.is_empty());
        assert!(definition.responsive.soft_flushes.iter().all(|key| {
            matches!(
                report.execution.flushes()[*key as usize].outcome,
                FlushOutcome::Wrapped | FlushOutcome::DeferredColumn
            )
        }));
        assert!(definition.responsive.hard_boundaries.iter().all(|key| {
            let boundary = &report.execution.boundaries()[*key as usize];
            matches!(
                boundary.effect,
                BoundaryEffect::EndedLine | BoundaryEffect::AddedVerticalSpace
            ) && boundary.control.is_some()
        }));
        let hard_macros = definition
            .responsive
            .hard_boundaries
            .iter()
            .map(|key| {
                let boundary = &report.execution.boundaries()[*key as usize];
                let control = &report.execution.controls()[boundary.control.unwrap() as usize];
                report.execution.nodes()[control.node.0 as usize]
                    .macro_name
                    .as_deref()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(!hard_macros.is_empty());
        assert!(hard_macros.iter().all(|macro_name| *macro_name == "br"));
    }

    #[test]
    fn keeps_structural_hard_boundaries_and_assigns_nested_breaks_once() {
        // These exact fixtures were rendered with pinned CVS `-Tutf8` and
        // `-Tlint` before this assertion was written.  `termp_pp_pre()` owns
        // its `term_vspace()` directly (without a raw control wrapper), while
        // a nested `.br` belongs only to the closest definition lifecycle.
        let pp = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-pp-head-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-pp-head-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&pp.document, &pp.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one Pp definition");
        };
        assert!(definition.responsive.hard_boundaries.iter().any(|key| {
            let boundary = &pp.execution.boundaries()[*key as usize];
            boundary.control.is_none()
                && boundary.request == libmandoc_rs::BoundaryRequest::VerticalSpace
                && boundary.effect == BoundaryEffect::AddedVerticalSpace
                && boundary.node.is_some_and(|node| {
                    pp.execution.nodes()[node.0 as usize].macro_name.as_deref() == Some("Pp")
                })
        }));
        let mut item = responsive_definition_item(definition.responsive.layout);
        item.terms = vec![vec![mant_ir::Inline::Text {
            value: "beta".to_owned(),
        }]];
        assert!(
            mant_ir::geometry::definition_placement(&item, 0, Some(120)).run_in,
            "an earlier Pp closes alpha, but must not close the final beta field"
        );

        let nested = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-nested-boundary-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-nested-boundary-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&nested.document, &nested.execution);
        let br_control = nested
            .execution
            .controls()
            .iter()
            .find(|control| {
                nested.execution.nodes()[control.node.0 as usize]
                    .macro_name
                    .as_deref()
                    == Some("br")
            })
            .expect("nested br control");
        let br_boundary = nested
            .execution
            .boundaries()
            .iter()
            .find(|boundary| boundary.control == Some(br_control.key))
            .expect("nested br boundary")
            .key;
        let owners = projection
            .definitions
            .iter()
            .filter(|definition| definition.responsive.hard_boundaries.contains(&br_boundary))
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 1);
        assert_eq!(
            nested.execution.nodes()[owners[0].owner.0 as usize].line,
            11
        );
    }

    #[test]
    fn excludes_native_definition_phase_finalization_from_hard_boundaries() {
        // Pinned CVS `termp_it_post()` unconditionally calls `term_newln()`
        // for this ordinary short `Bl -tag` item.  The exact fixture was run
        // through the fixed reference `-Tutf8` and `-Tlint`: it has no
        // authored content break, so responsive IR must not preserve that
        // fixed-device HEAD/BODY finalization as a hard boundary.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-responsive-metadata.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-responsive-metadata.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one ordinary native definition lifecycle");
        };
        assert!(definition.responsive.hard_boundaries.is_empty());
    }

    #[test]
    fn keeps_head_finalization_hard_after_control_clears_native_nobreak() {
        // This exact fixture was rendered with pinned CVS `-Tascii -O
        // width=30` and `-Tlint` before this assertion was written.  In
        // `roff_term.c::roff_term_pre_mc()`, `.mc` flushes buffered text and
        // clears NOBREAK; the later `mdoc_term.c::termp_it_post()` newline
        // therefore structurally separates BODY instead of merely finalizing
        // the handler's ordinary conditional tag field.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "definition-margin-control-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/definition-margin-control-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let [definition] = projection.definitions.as_slice() else {
            panic!("one margin-control definition");
        };
        assert!(!definition.responsive.hard_boundaries.is_empty());
        let constraint = definition
            .responsive
            .layout
            .fit_constraint
            .expect("native fit constraint");
        assert!(constraint.forced_separation);

        let mut item = responsive_definition_item(definition.responsive.layout);
        item.terms = vec![vec![mant_ir::Inline::Text {
            value: "A B".to_owned(),
        }]];
        assert!(
            !mant_ir::geometry::definition_placement(&item, 0, Some(120)).run_in,
            "a wide reader must not erase the executed HEAD boundary"
        );
    }

    #[test]
    fn excludes_man_hanging_paragraphs_from_definition_layout() {
        // The pinned CVS `man_term.c` reference establishes `.HP` as a
        // hanging paragraph, not a label/body definition lifecycle.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "man-definition-lifecycle.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/man-definition-lifecycle.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        assert!(projection.definitions.iter().all(|definition| {
            definition.kind != NativeDefinitionKind::Man(ExecutionManBlockKind::HangingParagraph)
        }));
        assert!(projection.man_blocks.iter().any(|block| {
            block.kind == ExecutionManBlockKind::HangingParagraph
                && report.execution.wrappers()[block.wrapper as usize]
                    .definition
                    .is_none()
        }));
    }

    #[test]
    fn reference_projection_owns_wrapped_labels_and_nested_relationships() {
        let wrapped = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes(
                "wrapped-reference-mdoc.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/wrapped-reference-mdoc.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let wrapped_projection = project(&wrapped.document, &wrapped.execution);
        let wrapped_reference = wrapped_projection
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
            .expect("native wrapped URI reference");
        let wrapped_key = wrapped_reference.key;
        assert_eq!(wrapped_reference.primary, b"https://example.org/wrapped");
        let label_runs = wrapped_projection
            .runs
            .iter()
            .filter(|run| run.reference == Some(wrapped_key))
            .collect::<Vec<_>>();
        assert!(
            label_runs
                .iter()
                .map(|run| run.device_line)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                >= 2,
            "the fixed native width must wrap the semantic label"
        );
        assert!(
            label_runs
                .iter()
                .all(|run| run.font == ExecutionFont::Underline)
        );
        assert!(label_runs.iter().all(|run| {
            run.role == AtomRole::Authored
                && run.word.is_some()
                && run.wrapper.is_some()
                && !run.atoms.is_empty()
        }));
        assert!(
            wrapped_projection.runs.iter().any(|run| {
                run.reference.is_none() && run.text == "https://example.org/wrapped"
            })
        );
        drop(wrapped);
        assert_eq!(
            wrapped_projection
                .references
                .iter()
                .find(|reference| reference.key == wrapped_key)
                .expect("owned wrapped reference")
                .primary,
            b"https://example.org/wrapped"
        );

        let nested = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "nested-reference-man.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/nested-reference-man.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let nested_projection = project(&nested.document, &nested.execution);
        let outer = nested_projection
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
            .expect("outer URI reference");
        let inner = nested_projection
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::Manual)
            .expect("nested manual reference");
        assert_eq!(inner.parent, Some(outer.key));
        assert!(outer.execution_atoms.start <= inner.execution_atoms.start);
        assert!(inner.execution_atoms.end <= outer.execution_atoms.end);
        assert!(
            nested_projection
                .runs
                .iter()
                .filter(|run| run.reference == Some(inner.key))
                .any(|run| run.text.contains("printf") || run.text.contains('3'))
        );
        drop(nested);
        assert_eq!(inner.primary, b"printf");
        assert_eq!(inner.secondary.as_deref(), Some(b"3".as_slice()));
    }

    #[test]
    fn inline_projection_preserves_native_word_and_annotation_boundaries() {
        // These complete inputs were rendered with the pinned CVS binary
        // before the assertions were written.  Fixed CVS emits Fn/OP/Bx
        // punctuation as separate formatter words, and opens Lk/Mt/UR
        // references only around the label chosen by the native handler.
        for (path, format, source) in [
            (
                "inline-annotations-mdoc.1",
                InputFormat::Mdoc,
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/inline-annotations-mdoc.1"
                )
                .as_slice(),
            ),
            (
                "inline-annotations-man.1",
                InputFormat::Man,
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/inline-annotations-man.1"
                )
                .as_slice(),
            ),
        ] {
            let report = Parser::new(ParseOptions::default())
                .with_input_format(format)
                .with_mdoc_operating_system("ManT")
                .unwrap()
                .execute_bytes(path, source, ExecutionLimits::default())
                .unwrap();
            let projection = project(&report.document, &report.execution);

            for run in &projection.runs {
                assert!(!run.atoms.is_empty(), "{path}");
                for &atom_key in &run.atoms {
                    let atom = &report.execution.atoms()[atom_key as usize];
                    assert_eq!(atom.font, run.font, "{path}");
                    assert_eq!(atom.role, run.role, "{path}");
                    assert_eq!(atom.wrapper, run.wrapper, "{path}");
                    assert_eq!(
                        projection
                            .words
                            .iter()
                            .find(|word| word.atoms.contains(&atom_key))
                            .map(|word| word.key),
                        run.word,
                        "{path}"
                    );
                }
            }

            for spelling in ["[", "]"] {
                assert!(
                    projection.runs.iter().any(|run| {
                        run.text == spelling && run.role == AtomRole::MacroGenerated
                    }),
                    "{path}: missing generated {spelling}"
                );
            }
            if format == InputFormat::Mdoc {
                for spelling in ["(", ",", ")", ";", "BSD"] {
                    assert!(
                        projection.runs.iter().any(|run| {
                            run.text == spelling && run.role == AtomRole::MacroGenerated
                        }),
                        "{path}: missing generated {spelling}"
                    );
                }
            }

            for reference in &projection.references {
                let label = projection
                    .runs
                    .iter()
                    .filter(|run| run.reference == Some(reference.key))
                    .map(|run| run.text.as_str())
                    .collect::<String>();
                assert!(!label.is_empty(), "{path}: empty native reference label");
                if projection.origins[reference.owner_node.0 as usize]
                    .macro_name
                    .as_deref()
                    != Some("Mt")
                {
                    assert!(
                        !label
                            .as_bytes()
                            .windows(reference.primary.len())
                            .any(|part| part == reference.primary),
                        "{path}: hidden target leaked into labelled reference"
                    );
                }
            }
        }
    }

    #[test]
    fn definition_projection_owns_each_partial_native_flush() {
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "partial-definition-man.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/partial-definition-man.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        let definition = projection
            .definitions
            .first()
            .expect("native TP definition");
        assert!(!definition.head_flushes.is_empty());
        assert!(
            definition
                .body_flushes
                .iter()
                .any(|flush| flush.outcome == libmandoc_rs::FlushOutcome::Wrapped)
        );
        assert!(
            definition
                .head_flushes
                .iter()
                .chain(&definition.body_flushes)
                .all(|flush| {
                    flush.accepted.start == flush.scanned.start
                        && flush.accepted.end == flush.consumed.end
                        && flush.accepted.end == flush.tail_discarded.start
                        && flush.tail_discarded.end == flush.remaining.start
                        && flush.accepted.end <= flush.scanned.end
                        && flush.fragments.end >= flush.fragments.start
                })
        );
    }

    fn assert_contracted_slot_is_not_reassigned(
        report: &libmandoc_rs::ExecutionReport,
        projection: &NativeProjection,
    ) {
        let shrunk = projection
            .fields
            .iter()
            .find(|field| {
                field.flush.node.is_some_and(|node| {
                    let owner = &report.execution.nodes()[node.0 as usize];
                    owner.source == 0
                        && owner.line == 33
                        && owner.kind == NodeKind::Body
                        && owner.macro_name.as_deref() == Some("SH")
                })
            })
            .expect("SHRUNK BUFFER field");
        let projected = shrunk
            .emitted_atoms
            .iter()
            .chain(&shrunk.consumed_atoms)
            .chain(&shrunk.replaced_atoms)
            .chain(&shrunk.trailing_discarded_atoms)
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let cleared_then_reused = report
            .execution
            .atoms()
            .iter()
            .filter(|atom| {
                atom.buffer_generation == Some(shrunk.flush.buffer_generation)
                    && atom.disposition == AtomDisposition::TrailingDiscard
                    && atom
                        .slot
                        .is_some_and(|slot| shrunk.flush.accepted.contains(&slot))
            })
            .find(|cleared| {
                report.execution.atoms().iter().any(|later| {
                    later.buffer_generation == cleared.buffer_generation
                        && later.slot == cleared.slot
                        && later.sequence > cleared.sequence
                        && later.disposition != AtomDisposition::Replaced
                })
            })
            .expect("cleared overstrike slot reused before the field flush");
        assert!(
            !projected.contains(&cleared_then_reused.key.0),
            "a contraction-era discarded atom is not part of the later field"
        );
    }

    fn expected_field_atom_keys(
        report: &libmandoc_rs::ExecutionReport,
        field: &NativeFieldFact,
    ) -> std::collections::BTreeSet<u32> {
        let mut expected = std::collections::BTreeSet::new();
        for range in [
            field.flush.accepted.clone(),
            field.flush.tail_discarded.clone(),
        ] {
            for slot in range {
                let current = report
                    .execution
                    .atoms()
                    .iter()
                    .rev()
                    .find(|atom| {
                        atom.buffer_generation == Some(field.flush.buffer_generation)
                            && atom.slot == Some(slot)
                            && atom.disposition != AtomDisposition::Replaced
                    })
                    .expect("validated field slot has a current occupant");
                let mut current_key = current.key.0;
                expected.insert(current_key);
                while let Some(predecessor) = report.execution.atoms().iter().find(|atom| {
                    atom.buffer_generation == Some(field.flush.buffer_generation)
                        && atom.slot == Some(slot)
                        && atom.replaced_by == Some(libmandoc_rs::AtomKey(current_key))
                }) {
                    current_key = predecessor.key.0;
                    assert!(
                        expected.insert(current_key),
                        "replacement ancestry is acyclic"
                    );
                }
            }
        }
        expected
    }

    #[test]
    fn field_projection_consumes_native_ranges_without_reconstructing_layout() {
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "field-consumption-man.1",
                include_bytes!(
                    "../../../libmandoc-rs/tests/fixtures/execution/field-consumption-man.1"
                ),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        assert_eq!(projection.fields.len(), report.execution.flushes().len());
        assert!(projection.fields.iter().all(|field| {
            let projected = field
                .emitted_atoms
                .iter()
                .chain(&field.consumed_atoms)
                .chain(&field.replaced_atoms)
                .chain(&field.trailing_discarded_atoms)
                .copied()
                .collect::<Vec<_>>();
            let projected_set = projected
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            field.emitted_atoms.iter().all(|key| {
                report.execution.atoms()[*key as usize].disposition == AtomDisposition::Emitted
            }) && field.consumed_atoms.iter().all(|key| {
                report.execution.atoms()[*key as usize].disposition == AtomDisposition::Consumed
            }) && field.replaced_atoms.iter().all(|key| {
                report.execution.atoms()[*key as usize].disposition == AtomDisposition::Replaced
            }) && field.trailing_discarded_atoms.iter().all(|key| {
                report.execution.atoms()[*key as usize].disposition
                    == AtomDisposition::TrailingDiscard
            }) && projected.len() == projected_set.len()
                && projected_set == expected_field_atom_keys(&report, field)
        }));

        assert_contracted_slot_is_not_reassigned(&report, &projection);

        let no_content = projection
            .fields
            .iter()
            .find(|field| field.flush.outcome == libmandoc_rs::FlushOutcome::NoContent)
            .expect("whitespace-only native field");
        assert!(no_content.emitted_atoms.is_empty());
        assert!(no_content.consumed_atoms.is_empty());
        assert_eq!(no_content.trailing_discarded_atoms.len(), 4);
        assert!(
            no_content.trailing_discarded_atoms.iter().any(|key| {
                report.execution.atoms()[*key as usize].kind == AtomKind::TabReference
            })
        );

        let deferred = projection
            .fields
            .iter()
            .filter(|field| field.flush.outcome == libmandoc_rs::FlushOutcome::DeferredColumn)
            .collect::<Vec<_>>();
        assert_eq!(deferred.len(), 2);
        assert!(deferred.iter().all(|field| {
            !field.emitted_atoms.is_empty()
                && !field.flush.remaining.is_empty()
                && field.flush.tail_discarded.end == field.flush.remaining.start
                && field.consumed_atoms.iter().any(|key| {
                    report.execution.atoms()[*key as usize]
                        .slot
                        .is_some_and(|slot| field.flush.tail_discarded.contains(&slot))
                })
        }));
    }

    #[test]
    fn table_projection_keeps_owned_topology_and_filters_device_layout() {
        // Pinned CVS `tbl_term.c` separates authored cell words from the
        // padding and rule glyphs that `term_tbl()` generates for the device.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "native-execution-table-matrix.1",
                include_bytes!("fixtures/native-execution-table-matrix.1"),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        drop(report);

        let [table] = projection.tables.as_slice() else {
            panic!("one native table");
        };
        assert_matrix_execution(table);
        assert_matrix_ir(table);
        let document = mant_ir::Document {
            parser: None,
            source: mant_ir::DocumentSource {
                format: mant_ir::SourceFormat::Man,
                path: Some("native-execution-table-matrix.1".to_owned()),
            },
            meta: mant_ir::DocumentMeta::default(),
            heading: None,
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: vec![table.block.clone()],
            sections: Vec::new(),
        };
        assert!(mant_ir::validate_document(&document).is_empty());
    }

    #[test]
    fn table_projection_preserves_hard_breaks_without_device_row_breaks() {
        // Pinned CVS `tbl_term.c` repeatedly calls `term_flushln()` over the
        // column buffers. Those alternating device rows are distinct from the
        // authored `\p` word-end break recorded in each cell payload.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "native-execution-table-interleaving.1",
                include_bytes!("fixtures/native-execution-table-interleaving.1"),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        drop(report);

        let [table] = projection.tables.as_slice() else {
            panic!("one native table");
        };
        let first = &table.rows[0];
        assert_eq!(first.cells.len(), 2);
        assert!(first.cells.iter().all(|cell| {
            cell.fragments
                .iter()
                .map(|fragment| fragment.device_line)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == 2
        }));
        assert_eq!(
            first.cells[0].content,
            [
                mant_ir::Inline::Text {
                    value: "left alpha".to_owned(),
                },
                mant_ir::Inline::LineBreak,
                mant_ir::Inline::Text {
                    value: "left beta".to_owned(),
                },
            ]
        );
        assert_eq!(
            first.cells[1].content,
            [
                mant_ir::Inline::Text {
                    value: "right one".to_owned(),
                },
                mant_ir::Inline::LineBreak,
                mant_ir::Inline::Text {
                    value: "right two".to_owned(),
                },
            ]
        );

        let mut flush_order = first
            .cells
            .iter()
            .flat_map(|cell| {
                cell.flushes
                    .iter()
                    .map(move |flush| (flush.sequence, cell.data_ordinal))
            })
            .collect::<Vec<_>>();
        flush_order.sort_unstable();
        assert_eq!(
            flush_order
                .iter()
                .map(|(_, data_ordinal)| *data_ordinal)
                .collect::<Vec<_>>(),
            [0, 1, 0, 1],
            "native device output alternates column slices"
        );

        let rows = table_rows(table);
        assert_eq!(rows.len(), 2);
        assert_eq!(cell_inlines(&rows[0].cells[0]), first.cells[0].content,);
        assert_eq!(cell_inlines(&rows[0].cells[1]), first.cells[1].content,);
        assert_eq!(
            cell_inlines(&rows[1].cells[0]),
            [mant_ir::Inline::Text {
                value: "left tail".to_owned(),
            }]
        );
        assert_eq!(
            cell_inlines(&rows[1].cells[1]),
            [mant_ir::Inline::Text {
                value: "right tail".to_owned(),
            }]
        );
    }

    #[test]
    fn table_projection_agrees_with_ast_vertical_continuation_spelling() {
        // Fixed CVS `tbl_data()` recognizes a literal `\^` data cell as a
        // vertical continuation, and the complete fixture renders only the
        // preceding `first` row. The execution report and owned AST must use
        // that same parser fact rather than independent layout-only tests.
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "native-execution-table-vertical-continuation.1",
                include_bytes!("fixtures/native-execution-table-vertical-continuation.1"),
                ExecutionLimits::default(),
            )
            .unwrap();
        let projection = project(&report.document, &report.execution);
        drop(report);

        let [table] = projection.tables.as_slice() else {
            panic!("one native table");
        };
        assert_eq!(table.rows.len(), 2);
        let continuation = &table.rows[1].cells[0];
        assert!(
            continuation
                .flags
                .contains(ExecutionTableCellFlags::VERTICAL_CONTINUATION)
        );
        assert!(continuation.content.is_empty());
        assert!(table_rows(table)[1].cells[0].blocks.is_empty());
    }

    #[cfg(feature = "staged-native-audit")]
    #[test]
    fn staged_audit_accepts_only_one_atomic_execution_result() {
        let report = Parser::new(ParseOptions::default())
            .with_input_format(InputFormat::Man)
            .execute_bytes(
                "staged-pair-first.1",
                b".TH FIRST 1\n.SH NAME\nfirst \\- probe\n",
                ExecutionLimits::default(),
            )
            .unwrap();
        let document = lower_staged_semantic_document(&report);
        assert_eq!(document.meta.title.as_deref(), Some("FIRST"));
    }
}
