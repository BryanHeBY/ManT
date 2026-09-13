#![cfg(feature = "execute")]

use libmandoc_rs::{
    AtomRole, ExecutionErrorKind, ExecutionFont, ExecutionLimits, ExecutionReferenceKind,
    ExecutionTableAlignment, ExecutionTableDataKind, ExecutionTableLayoutKind,
    ExecutionTableRowKind, FlushOutcome, FragmentRole, InputFormat, NativeExecutionReport, Node,
    ParseOptions, Parser,
};

const MAN: &[u8] = include_bytes!("fixtures/execution/plain-man.1");
const MDOC: &[u8] = include_bytes!("fixtures/execution/plain-mdoc.1");
const TABLE: &[u8] = include_bytes!("fixtures/execution/unsupported-table.1");
const TABLE_VERTICAL_CONTINUATION: &[u8] =
    include_bytes!("fixtures/execution/table-vertical-continuation.1");
const TABLE_WITH_EMPTY_CELL: &[u8] = br#".TH PROBE 1 "September 14, 2026" "ManT" "Manual"
.SH DESCRIPTION
.TS
l l l.
left		third
.TE
"#;
const TABLE_WITH_WORD_SPACING: &[u8] = br#".TH PROBE 1 "September 14, 2026" "ManT" "Manual"
.SH DESCRIPTION
.TS
l.
left alpha
.TE
.B outside
.I words
"#;
const EQUATION: &[u8] = include_bytes!("fixtures/execution/unsupported-equation.1");
const EXECUTED_SO: &[u8] = include_bytes!("fixtures/execution/executed-so.1");
const INACTIVE_SO: &[u8] = include_bytes!("fixtures/execution/inactive-so.1");
const DEVICE_ROLES: &[u8] = include_bytes!("fixtures/execution/device-roles.1");
const ANNOTATED_MAN: &[u8] = include_bytes!("fixtures/execution/annotated-man.1");
const ANNOTATED_MDOC: &[u8] = include_bytes!("fixtures/execution/annotated-mdoc.1");
const REFERENCES_MAN: &[u8] = include_bytes!("fixtures/execution/references-man.1");
const REFERENCES_MDOC: &[u8] = include_bytes!("fixtures/execution/references-mdoc.1");
const WRAPPED_REFERENCE_MDOC: &[u8] = include_bytes!("fixtures/execution/wrapped-reference-mdoc.1");
const NESTED_REFERENCE_MAN: &[u8] = include_bytes!("fixtures/execution/nested-reference-man.1");

fn execute(name: &str, input_format: InputFormat, source: &[u8]) -> libmandoc_rs::ExecutionReport {
    Parser::new(ParseOptions::default())
        .with_input_format(input_format)
        .with_mdoc_operating_system("ManT")
        .unwrap()
        .execute_bytes(name, source, ExecutionLimits::default())
        .unwrap()
}

fn execution_keys(node: &Node, keys: &mut Vec<u32>) {
    keys.push(
        node.execution_node_key
            .expect("executed AST node must expose its report-local key"),
    );
    for child in &node.children {
        execution_keys(child, keys);
    }
}

fn assert_ast_report_identity(node: &Node, report: &libmandoc_rs::NativeExecutionReport) {
    let key = node
        .execution_node_key
        .expect("executed AST node must expose its report-local key");
    let execution = &report.nodes[key as usize];
    assert_eq!(execution.key.0, key);
    assert_eq!(execution.line, node.line, "line mismatch for node {key}");
    assert_eq!(
        execution.column, node.column,
        "column mismatch for node {key}"
    );
    assert_eq!(
        execution.macro_name.as_deref(),
        node.macro_name.as_deref(),
        "macro mismatch for node {key}"
    );
    for child in &node.children {
        assert_ast_report_identity(child, report);
    }
}

fn pool(report: &NativeExecutionReport, range: libmandoc_rs::PoolRange) -> &[u8] {
    report.pool_bytes(range).expect("valid report pool range")
}

fn has_ancestor_macro(
    report: &NativeExecutionReport,
    mut node: libmandoc_rs::ExecutionNodeKey,
    expected: &str,
) -> bool {
    loop {
        let current = &report.nodes[node.0 as usize];
        if current.macro_name.as_deref() == Some(expected) {
            return true;
        }
        let Some(parent) = current.parent else {
            return false;
        };
        node = parent;
    }
}

fn ast_node_by_execution_key(node: &Node, key: u32) -> Option<&Node> {
    if node.execution_node_key == Some(key) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| ast_node_by_execution_key(child, key))
}

#[test]
fn one_native_session_owns_matching_ast_and_execution_nodes() {
    for (name, format, source) in [
        ("plain-man.1", InputFormat::Man, MAN),
        ("plain-mdoc.1", InputFormat::Mdoc, MDOC),
    ] {
        let report = execute(name, format, source);
        assert!(!report.execution.nodes.is_empty());
        assert!(!report.execution.atoms.is_empty());
        assert!(!report.execution.fragments.is_empty());
        assert_eq!(report.execution.sources.len(), 1);
        assert_eq!(report.execution.sources[0].path.to_string_lossy(), name);
        assert_eq!(report.execution.nodes[0].key.0, 0);
        assert_ast_report_identity(&report.document.root, &report.execution);
        let mut ast_keys = Vec::new();
        execution_keys(&report.document.root, &mut ast_keys);
        assert_eq!(
            ast_keys,
            report
                .execution
                .nodes
                .iter()
                .map(|node| node.key.0)
                .collect::<Vec<_>>()
        );
        assert!(
            report
                .execution
                .nodes
                .iter()
                .enumerate()
                .all(|(index, node)| { usize::try_from(node.key.0).ok() == Some(index) })
        );
        assert!(
            report
                .execution
                .atoms
                .windows(2)
                .all(|atoms| { atoms[0].sequence < atoms[1].sequence })
        );
        assert!(!report.execution.buffer_generations.is_empty());
        assert!(
            report
                .execution
                .buffer_generations
                .iter()
                .all(|generation| {
                    generation.extent <= generation.capacity
                        && generation.open_sequence < generation.close_sequence
                })
        );
        assert!(report.execution.flushes.iter().all(|flush| {
            flush.scanned.start == flush.accepted.start
                && flush.accepted == flush.consumed
                && flush.accepted.end == flush.remaining.start
                && flush.remaining.end == flush.scanned.end
                && flush.sequence < flush.outcome_sequence
        }));
    }

    let report = execute("plain-mdoc.1", InputFormat::Mdoc, MDOC);
    let plain = report
        .execution
        .atoms
        .iter()
        .find(|atom| {
            atom.role == AtomRole::Authored
                && atom.operand.is_some_and(|range| {
                    report.execution.pool_bytes(range) == Some(b"Plain".as_slice())
                })
        })
        .expect("authored Plain operand");
    let origin = &report.execution.nodes[plain.node.expect("authored word node").0 as usize];
    assert_eq!(origin.macro_name, None);
}

#[test]
fn reports_native_flush_branches_and_device_decoration_roles() {
    let plain = execute("plain-man.1", InputFormat::Man, MAN);
    assert!(
        plain
            .execution
            .flushes
            .iter()
            .any(|flush| flush.outcome == FlushOutcome::Wrapped),
        "the fixed 78-column execution must expose its native wrap branch"
    );
    assert!(
        plain
            .execution
            .flushes
            .iter()
            .any(|flush| flush.outcome == FlushOutcome::Exhausted)
    );

    let decorated = execute("device-roles.1", InputFormat::Man, DEVICE_ROLES);
    assert!(
        decorated
            .execution
            .fragments
            .iter()
            .any(|fragment| fragment.role == FragmentRole::Content)
    );
    assert!(
        decorated
            .execution
            .fragments
            .iter()
            .any(|fragment| fragment.role == FragmentRole::MarginDecoration)
    );
    assert!(
        decorated
            .execution
            .fragments
            .iter()
            .any(|fragment| fragment.role == FragmentRole::PageDecoration)
    );
    let margin = decorated
        .execution
        .fragments
        .iter()
        .find(|fragment| fragment.role == FragmentRole::MarginDecoration)
        .expect("native .mc margin fragment");
    let margin_atom = &decorated.execution.atoms[margin.atoms[0].0 as usize];
    assert_eq!(margin_atom.role, AtomRole::Authored);
    assert_eq!(
        margin_atom
            .operand
            .and_then(|range| decorated.execution.pool_bytes(range)),
        Some(b"|".as_slice())
    );
    let margin_node =
        &decorated.execution.nodes[margin_atom.node.expect(".mc operand node").0 as usize];
    assert_eq!(margin_node.line, 3);
    assert_eq!(margin.node, margin_atom.node);
    assert_ne!(
        margin_node.line, 4,
        "margin must not inherit VISIBLE's owner"
    );
}

#[test]
fn reports_native_definition_fonts_references_and_actual_targets() {
    for (name, format, source, owner_macro) in [
        ("annotated-man.1", InputFormat::Man, ANNOTATED_MAN, "UR"),
        ("annotated-mdoc.1", InputFormat::Mdoc, ANNOTATED_MDOC, "Lk"),
    ] {
        let report = execute(name, format, source);
        let execution = &report.execution;
        let reference = execution
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
            .expect("native URI reference");
        assert_eq!(
            pool(execution, reference.primary),
            b"https://example.org/manual"
        );
        assert_eq!(reference.secondary, None);
        assert_eq!(
            execution.nodes[reference.owner_node.0 as usize]
                .macro_name
                .as_deref(),
            Some(owner_macro)
        );
        assert_eq!(
            ast_node_by_execution_key(&report.document.root, reference.target_node.0)
                .and_then(|node| node.text.as_deref()),
            Some("https://example.org/manual")
        );
        let label_operands = execution.atoms
            [reference.atoms.start as usize..reference.atoms.end as usize]
            .iter()
            .filter_map(|atom| atom.operand)
            .map(|range| pool(execution, range))
            .collect::<Vec<_>>();
        assert!(label_operands.iter().any(|operand| {
            operand
                .windows(b"linked".len())
                .any(|part| part == b"linked")
        }));
        assert!(
            label_operands
                .iter()
                .any(|operand| operand.windows(b"label".len()).any(|part| part == b"label"))
        );
        assert!(!label_operands.contains(&b"https://example.org/manual".as_slice()));
        assert!(execution.atoms.iter().any(|atom| {
            atom.font == ExecutionFont::Underline
                && atom.operand.is_some_and(|range| {
                    pool(execution, range)
                        .windows(b"styled".len())
                        .any(|part| part == b"styled")
                })
        }));
        assert!(execution.flushes.iter().any(|flush| {
            flush.node.is_some_and(|node| {
                has_ancestor_macro(
                    execution,
                    node,
                    if format == InputFormat::Man {
                        "TP"
                    } else {
                        "It"
                    },
                )
            }) && flush.outcome != FlushOutcome::NoContent
        }));
    }

    let mdoc = execute("annotated-mdoc.1", InputFormat::Mdoc, ANNOTATED_MDOC);
    let anchor = mdoc
        .execution
        .anchors
        .iter()
        .find(|anchor| pool(&mdoc.execution, anchor.target) == b"custom-target")
        .expect("moved native .Tg target");
    let owner = &mdoc.execution.nodes[anchor.node.0 as usize];
    assert_eq!(owner.macro_name.as_deref(), Some("It"));
    assert_eq!(owner.kind, 2, "target must attach to the actual It head");
    assert!(anchor.device_line > 0);
    assert!(
        usize::try_from(anchor.atom_cursor)
            .is_ok_and(|cursor| cursor <= mdoc.execution.atoms.len())
    );
    assert!(
        usize::try_from(anchor.fragment_cursor)
            .is_ok_and(|cursor| cursor <= mdoc.execution.fragments.len())
    );
}

#[test]
fn reports_typed_reference_components_and_exact_label_intervals() {
    for (name, format, source, expected) in [
        (
            "references-man.1",
            InputFormat::Man,
            REFERENCES_MAN,
            vec![
                (
                    ExecutionReferenceKind::Manual,
                    b"printf".as_slice(),
                    Some(b"3".as_slice()),
                ),
                (
                    ExecutionReferenceKind::ExternalUri,
                    b"https://example.org/path".as_slice(),
                    None,
                ),
                (
                    ExecutionReferenceKind::Email,
                    b"one@example.org".as_slice(),
                    None,
                ),
            ],
        ),
        (
            "references-mdoc.1",
            InputFormat::Mdoc,
            REFERENCES_MDOC,
            vec![
                (
                    ExecutionReferenceKind::ExternalUri,
                    b"https://example.org/path".as_slice(),
                    None,
                ),
                (
                    ExecutionReferenceKind::Email,
                    b"one@example.org".as_slice(),
                    None,
                ),
                (
                    ExecutionReferenceKind::Email,
                    b"two@example.org".as_slice(),
                    None,
                ),
                (
                    ExecutionReferenceKind::Manual,
                    b"printf".as_slice(),
                    Some(b"3".as_slice()),
                ),
            ],
        ),
    ] {
        let report = execute(name, format, source);
        let actual = report
            .execution
            .references
            .iter()
            .map(|reference| {
                (
                    reference.kind,
                    pool(&report.execution, reference.primary),
                    reference
                        .secondary
                        .map(|range| pool(&report.execution, range)),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert!(report.execution.references.iter().all(|reference| {
            reference.atoms.start < reference.atoms.end
                && reference.atoms.end as usize <= report.execution.atoms.len()
                && report.execution.atoms[reference.atoms.start as usize].role
                    != AtomRole::ImplicitSpace
                && report.execution.atoms
                    [reference.execution_atoms.start as usize..reference.atoms.start as usize]
                    .iter()
                    .all(|atom| atom.role == AtomRole::ImplicitSpace)
        }));
        let manual = report
            .execution
            .references
            .iter()
            .find(|reference| reference.kind == ExecutionReferenceKind::Manual)
            .unwrap();
        let operands = report.execution.atoms
            [manual.atoms.start as usize..manual.atoms.end as usize]
            .iter()
            .filter_map(|atom| atom.operand)
            .map(|range| pool(&report.execution, range))
            .collect::<Vec<_>>();
        assert!(operands.contains(&b"printf".as_slice()));
        assert!(operands.contains(&b"3".as_slice()));
        assert!(!operands.contains(&b",".as_slice()));
    }
}

#[test]
fn nested_native_references_form_a_parent_linked_execution_stack() {
    let report = execute(
        "nested-reference-man.1",
        InputFormat::Man,
        NESTED_REFERENCE_MAN,
    );
    assert_eq!(report.execution.references.len(), 2);
    let outer = &report.execution.references[0];
    let inner = &report.execution.references[1];
    assert_eq!(outer.kind, ExecutionReferenceKind::ExternalUri);
    assert_eq!(inner.kind, ExecutionReferenceKind::Manual);
    assert_eq!(outer.parent, None);
    assert_eq!(inner.parent, Some(outer.key));
    assert!(outer.execution_atoms.start <= inner.execution_atoms.start);
    assert!(outer.execution_atoms.end >= inner.execution_atoms.end);
    assert!(outer.enter_sequence < inner.enter_sequence);
    assert!(outer.leave_sequence > inner.leave_sequence);
}

#[test]
fn one_native_reference_interval_survives_multiple_flushes() {
    let report = execute(
        "wrapped-reference-mdoc.1",
        InputFormat::Mdoc,
        WRAPPED_REFERENCE_MDOC,
    );
    let reference = report
        .execution
        .references
        .iter()
        .find(|reference| reference.kind == ExecutionReferenceKind::ExternalUri)
        .expect("wrapped URI reference");
    assert_eq!(
        pool(&report.execution, reference.primary),
        b"https://example.org/wrapped"
    );
    let mut device_lines = report
        .execution
        .fragments
        .iter()
        .filter(|fragment| {
            fragment
                .atoms
                .iter()
                .any(|atom| reference.atoms.start <= atom.0 && atom.0 < reference.atoms.end)
        })
        .map(|fragment| fragment.device_line)
        .collect::<Vec<_>>();
    device_lines.sort_unstable();
    device_lines.dedup();
    assert_eq!(device_lines.len(), 2);
}

#[test]
fn semantic_reference_and_anchor_callbacks_obey_report_budgets() {
    let baseline = execute("annotated-mdoc.1", InputFormat::Mdoc, ANNOTATED_MDOC);
    for limits in [
        ExecutionLimits {
            max_work: baseline.execution.work_units - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: baseline.execution.record_count - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_pool_bytes: baseline.execution.pool_len() as u64 - 1,
            ..ExecutionLimits::default()
        },
    ] {
        let error = Parser::default()
            .with_input_format(InputFormat::Mdoc)
            .with_mdoc_operating_system("ManT")
            .unwrap()
            .execute_bytes("annotated-mdoc.1", ANNOTATED_MDOC, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);
    }
}

#[cfg(feature = "serde")]
#[test]
fn report_local_execution_keys_do_not_escape_ast_serialization() {
    let report = execute("plain-man.1", InputFormat::Man, MAN);
    let json = serde_json::to_value(&report.document).unwrap();
    assert!(!json.to_string().contains("execution_node_key"));
}

#[test]
fn exhausted_budget_never_exposes_a_partial_report() {
    let baseline = execute("plain-man.1", InputFormat::Man, MAN);
    let max_depth = baseline
        .execution
        .nodes
        .iter()
        .map(|node| {
            let mut depth = 1_u64;
            let mut parent = node.parent;
            while let Some(key) = parent {
                depth += 1;
                parent = baseline.execution.nodes[key.0 as usize].parent;
            }
            depth
        })
        .max()
        .unwrap();
    let exact = [
        ExecutionLimits {
            max_nodes: baseline.execution.nodes.len() as u64,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_depth,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_work: baseline.execution.work_units,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: baseline.execution.record_count,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_pool_bytes: baseline.execution.pool_len() as u64,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_buffer_cells: baseline.execution.buffer_cells,
            ..ExecutionLimits::default()
        },
    ];
    for (index, limits) in exact.into_iter().enumerate() {
        Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes("plain-man.1", MAN, limits)
            .unwrap_or_else(|error| panic!("exact budget case {index} failed: {error:?}"));
    }
    let insufficient = [
        ExecutionLimits {
            max_nodes: baseline.execution.nodes.len() as u64 - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_depth: max_depth - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_work: baseline.execution.work_units - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: baseline.execution.record_count - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_pool_bytes: baseline.execution.pool_len() as u64 - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_buffer_cells: baseline.execution.buffer_cells - 1,
            ..ExecutionLimits::default()
        },
    ];
    for limits in insufficient {
        let error = Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes("plain-man.1", MAN, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);
        assert!(!error.message.is_empty());
    }
    assert!(
        !execute("plain-man.1", InputFormat::Man, MAN)
            .execution
            .fragments
            .is_empty()
    );
}

#[test]
fn invalid_execution_limits_are_rejected_as_budgets() {
    for limits in [
        ExecutionLimits {
            max_nodes: 0,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_depth: 0,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: u64::from(u32::MAX) + 1,
            ..ExecutionLimits::default()
        },
    ] {
        let error = Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes("plain-man.1", MAN, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);
    }
}

#[test]
fn table_execution_transfers_typed_rows_cells_and_payload_ownership() {
    // The pinned CVS renderer was run at widths 32, 78, and 120 before this
    // assertion was written.  In each case tbl_term.c executes one data row
    // with two left-aligned data cells and renders `left   right`.
    let report = execute("table.1", InputFormat::Man, TABLE);
    let execution = &report.execution;
    assert_eq!(execution.tables.len(), 1);
    assert_eq!(execution.table_rows.len(), 1);
    assert_eq!(execution.table_cells.len(), 2);

    let table = &execution.tables[0];
    let row = &execution.table_rows[0];
    assert_eq!(table.rows, 0..1);
    assert_eq!(table.cells, 0..2);
    assert_eq!(table.logical_columns, 2);
    assert_eq!(row.table, table.key);
    assert_eq!(row.kind, ExecutionTableRowKind::Data);
    assert_eq!(row.logical_columns, 2);
    assert_eq!(row.cells, 0..2);
    assert_eq!(row.node, table.first_row_node);

    for (index, cell) in execution.table_cells.iter().enumerate() {
        let index = u32::try_from(index).unwrap();
        assert_eq!(cell.row, row.key);
        assert_eq!(cell.ordinal, index);
        assert_eq!(cell.data_ordinal, index);
        assert_eq!(cell.logical_column, index);
        assert_eq!(cell.column_span, 1);
        assert_eq!(cell.row_span, 1);
        assert_eq!(cell.layout_kind, ExecutionTableLayoutKind::Left);
        assert_eq!(cell.data_kind, ExecutionTableDataKind::Text);
        assert_eq!(cell.alignment, ExecutionTableAlignment::Left);
        let buffer = cell.buffer.expect("non-empty table cell buffer");
        let generation_key = cell
            .buffer_generation
            .expect("non-empty table cell generation");
        let generation = &execution.buffer_generations[generation_key as usize];
        assert_eq!(generation.buffer, buffer);
        let payload_atoms = execution.atoms[cell.atoms.start as usize..cell.atoms.end as usize]
            .iter()
            .filter(|atom| atom.role == AtomRole::TableCellPayload)
            .collect::<Vec<_>>();
        assert!(!payload_atoms.is_empty());
        assert!(payload_atoms.iter().all(|atom| {
            atom.buffer == Some(buffer) && atom.buffer_generation == Some(generation_key)
        }));
        assert!(
            execution.flushes[row.flushes.start as usize..row.flushes.end as usize]
                .iter()
                .any(|flush| flush.buffer_generation == generation_key)
        );
    }

    let owned_table = table.clone();
    let owned_cells = execution.table_cells.clone();
    drop(report);
    assert_eq!(owned_table.logical_columns, 2);
    assert_eq!(owned_cells[0].data_ordinal, 0);
    assert_eq!(owned_cells[1].data_ordinal, 1);
}

#[test]
fn table_execution_budget_failure_is_atomic_and_reentrant() {
    let baseline = execute("table.1", InputFormat::Man, TABLE);
    for limits in [
        ExecutionLimits {
            max_work: baseline.execution.work_units - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_records: baseline.execution.record_count - 1,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_buffer_cells: baseline.execution.buffer_cells - 1,
            ..ExecutionLimits::default()
        },
    ] {
        let error = Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes("table.1", TABLE, limits)
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Budget);

        let next = execute("table.1", InputFormat::Man, TABLE);
        assert_eq!(next.execution.tables.len(), 1);
        assert_eq!(next.execution.table_cells.len(), 2);
    }
}

#[test]
fn empty_native_table_cell_has_no_buffer_identity() {
    // The pinned CVS renderer was run before this assertion was written and
    // emits `left` and `third`; tbl_term.c still executes the empty middle
    // logical data cell.
    let report = execute("empty-table.1", InputFormat::Man, TABLE_WITH_EMPTY_CELL);
    assert_eq!(report.execution.table_cells.len(), 3);
    assert!(report.execution.table_cells[0].buffer.is_some());
    assert!(report.execution.table_cells[0].buffer_generation.is_some());
    assert_eq!(report.execution.table_cells[1].buffer, None);
    assert_eq!(report.execution.table_cells[1].buffer_generation, None);
    assert!(report.execution.table_cells[1].atoms.is_empty());
    assert!(report.execution.table_cells[2].buffer.is_some());
    assert!(report.execution.table_cells[2].buffer_generation.is_some());
}

#[test]
fn explicit_table_vertical_continuation_matches_the_owned_ast() {
    // The complete fixture was accepted by the pinned CVS linter and rendered
    // as one visible `first` row before this assertion was written.  tbl_data.c
    // recognizes the data spelling `\^` as the same continuation fact as a
    // layout `^` cell.
    let report = execute(
        "table-vertical-continuation.1",
        InputFormat::Man,
        TABLE_VERTICAL_CONTINUATION,
    );
    assert_eq!(report.execution.table_rows.len(), 2);
    assert_eq!(report.execution.table_cells.len(), 2);
    let continuation = &report.execution.table_cells[1];
    assert!(
        continuation
            .flags
            .contains(libmandoc_rs::ExecutionTableCellFlags::VERTICAL_CONTINUATION)
    );
    let ast_row = ast_node_by_execution_key(&report.document.root, continuation.node.0)
        .expect("continuation row remains in the owned AST");
    assert!(ast_row.table_cells[0].vertical_continuation);
}

#[test]
fn table_payload_role_does_not_overwrite_implicit_spacing_provenance() {
    // The pinned CVS renderer was run before this assertion was written and
    // emits `left alpha` followed by the two native words `outside words`.
    let report = execute("spaced-table.1", InputFormat::Man, TABLE_WITH_WORD_SPACING);
    let cell = &report.execution.table_cells[0];
    let atoms = &report.execution.atoms[cell.atoms.start as usize..cell.atoms.end as usize];
    assert!(
        atoms
            .iter()
            .any(|atom| atom.role == AtomRole::TableCellPayload)
    );
    assert!(
        report
            .execution
            .atoms
            .iter()
            .any(|atom| atom.role == AtomRole::ImplicitSpace)
    );
    assert!(
        atoms
            .iter()
            .all(|atom| atom.role != AtomRole::ImplicitSpace)
    );
}

#[test]
fn unsupported_execution_shapes_fail_before_returning_a_partial_report() {
    let error = Parser::default()
        .with_input_format(InputFormat::Man)
        .execute_bytes("equation.1", EQUATION, ExecutionLimits::default())
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Unsupported);

    let parser = Parser::new(ParseOptions {
        includes: libmandoc_rs::IncludePolicy::SourceTree,
        compression: libmandoc_rs::Compression::Plain,
    });
    let error = parser
        .execute_bytes("plain-man.1", MAN, ExecutionLimits::default())
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Unsupported);

    let error = Parser::default()
        .with_input_format(InputFormat::Man)
        .execute_bytes("executed-so.1", EXECUTED_SO, ExecutionLimits::default())
        .unwrap_err();
    assert_eq!(error.kind, ExecutionErrorKind::Unsupported);
    let inactive = Parser::default()
        .with_input_format(InputFormat::Man)
        .execute_bytes("inactive-so.1", INACTIVE_SO, ExecutionLimits::default())
        .unwrap();
    assert!(!inactive.execution.fragments.is_empty());
}
