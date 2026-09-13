//! First private consumer of the owned native execution report.
//!
//! This is deliberately not the production switch. It establishes the
//! execution-to-projection boundary used by the staged migration without
//! reconstructing formatter state in Rust.

use libmandoc_rs::{
    AtomDisposition, AtomKind, AtomRole, BoundaryEffect, Document as NativeDocument,
    ExecutionAffinity, ExecutionBoundary, ExecutionControl, ExecutionFlush, ExecutionFont,
    ExecutionFragment, ExecutionNodeKey, ExecutionReferenceKind, ExecutionTableAlignment,
    ExecutionTableCell, ExecutionTableCellFlags, ExecutionTableCellKey, ExecutionTableDataKind,
    ExecutionTableKey, ExecutionTableLayoutKind, ExecutionTableRow, ExecutionTableRowKey,
    FragmentRole, GeometryKind, GeometryOriginKind, NativeExecutionReport, Node as NativeNode,
    NodeKind, NormalizedListKind, TableAlignment as NativeTableAlignment,
    TableCellKind as NativeTableCellKind, TableRowKind as NativeTableRowKind,
    TableRuleCellKind as NativeTableRuleCellKind,
};
use std::{collections::BTreeMap, ops::Range, path::PathBuf};

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeTextRun {
    pub(super) node: ExecutionNodeKey,
    pub(super) source: PathBuf,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) font: ExecutionFont,
    pub(super) text: String,
    pub(super) device_line: u32,
    pub(super) start_bu: i64,
    pub(super) end_bu: i64,
    pub(super) reference: Option<u32>,
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
    pub(super) head_flushes: Vec<ExecutionFlush>,
    pub(super) body_flushes: Vec<ExecutionFlush>,
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
    pub(super) fields: Vec<NativeFieldFact>,
    pub(super) controls: Vec<NativeControlFact>,
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

fn mark_definition_items(
    node: &NativeNode,
    inherited_list_kind: Option<NormalizedListKind>,
    definitions: &mut [bool],
) {
    let list_kind = if node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("Bl") {
        node.list_kind
    } else {
        inherited_list_kind
    };
    if node.kind == NodeKind::Block
        && node.macro_name.as_deref() == Some("It")
        && list_kind == Some(NormalizedListKind::Definition)
        && let Some(key) = node.execution_node_key
    {
        definitions[key as usize] = true;
    }
    for child in &node.children {
        mark_definition_items(child, list_kind, definitions);
    }
}

fn definition_facts(
    document: &NativeDocument,
    report: &NativeExecutionReport,
) -> Vec<NativeDefinitionFact> {
    #[derive(Default)]
    struct PendingDefinition {
        owner: Option<ExecutionNodeKey>,
        macro_name: Option<String>,
        head: Option<ExecutionNodeKey>,
        body: Option<ExecutionNodeKey>,
        head_flushes: Vec<ExecutionFlush>,
        body_flushes: Vec<ExecutionFlush>,
    }

    let mut definition_items = vec![false; report.nodes().len()];
    mark_definition_items(&document.root, None, &mut definition_items);
    let mut owner_definition = vec![None; report.nodes().len()];
    let mut definitions = Vec::new();
    for node in report.nodes() {
        let is_man_definition =
            matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "TQ" | "HP"));
        let is_mdoc_definition =
            node.macro_name.as_deref() == Some("It") && definition_items[node.key.0 as usize];
        if node.kind == NodeKind::Block && (is_man_definition || is_mdoc_definition) {
            owner_definition[node.key.0 as usize] = Some(definitions.len());
            definitions.push(PendingDefinition {
                owner: Some(node.key),
                macro_name: node.macro_name.clone(),
                ..PendingDefinition::default()
            });
        }
    }

    let mut direct_content_definition = vec![None; report.nodes().len()];
    for node in report.nodes() {
        let Some(parent) = node.parent else {
            continue;
        };
        let Some(definition) = owner_definition[parent.0 as usize] else {
            continue;
        };
        match node.kind {
            NodeKind::Head => {
                definitions[definition].head = Some(node.key);
                direct_content_definition[node.key.0 as usize] = Some((definition, true));
            }
            NodeKind::Body => {
                definitions[definition].body = Some(node.key);
                direct_content_definition[node.key.0 as usize] = Some((definition, false));
            }
            _ => {}
        }
    }

    let mut content_definition = vec![None; report.nodes().len()];
    for node in report.nodes() {
        content_definition[node.key.0 as usize] = direct_content_definition[node.key.0 as usize]
            .or_else(|| {
                node.parent
                    .and_then(|parent| content_definition[parent.0 as usize])
            });
    }

    for flush in report.flushes() {
        let Some(node) = flush.node else {
            continue;
        };
        if let Some((definition, is_head)) = content_definition[node.0 as usize] {
            if is_head {
                definitions[definition].head_flushes.push(flush.clone());
            } else {
                definitions[definition].body_flushes.push(flush.clone());
            }
        }
    }

    definitions
        .into_iter()
        .filter_map(|definition| {
            Some(NativeDefinitionFact {
                owner: definition.owner?,
                macro_name: definition.macro_name?,
                head: definition.head?,
                body: definition.body?,
                head_flushes: definition.head_flushes,
                body_flushes: definition.body_flushes,
            })
        })
        .collect()
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

fn text_projection(report: &NativeExecutionReport) -> (Vec<NativeTextRun>, Vec<String>) {
    let mut runs: Vec<NativeTextRun> = Vec::new();
    let atom_references = atom_reference_owners(report);
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
                && previous.device_line == fragment.device_line
                && previous.end_bu == fragment.start_bu
                && previous.reference == reference
            {
                previous.text.push(character);
                previous.end_bu = fragment.end_bu;
            } else {
                runs.push(NativeTextRun {
                    node,
                    source,
                    line: origin.line,
                    column: origin.column,
                    font: atom.font,
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

#[allow(dead_code)]
pub(super) fn project(
    document: &NativeDocument,
    report: &NativeExecutionReport,
) -> NativeProjection {
    let (runs, visible_lines) = text_projection(report);
    NativeProjection {
        origins: report
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
            .collect(),
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
        definitions: definition_facts(document, report),
        fields: field_facts(report),
        controls: control_facts(report),
        tables: table_projection(document, report),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libmandoc_rs::{ExecutionLimits, InputFormat, ParseOptions, Parser};

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
}
