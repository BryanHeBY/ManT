#![cfg(feature = "execute")]

use libmandoc_rs::{
    AtomRole, ExecutionErrorKind, ExecutionLimits, FlushOutcome, FragmentRole, InputFormat, Node,
    ParseOptions, Parser,
};

const MAN: &[u8] = include_bytes!("fixtures/execution/plain-man.1");
const MDOC: &[u8] = include_bytes!("fixtures/execution/plain-mdoc.1");
const TABLE: &[u8] = include_bytes!("fixtures/execution/unsupported-table.1");
const EQUATION: &[u8] = include_bytes!("fixtures/execution/unsupported-equation.1");
const EXECUTED_SO: &[u8] = include_bytes!("fixtures/execution/executed-so.1");
const INACTIVE_SO: &[u8] = include_bytes!("fixtures/execution/inactive-so.1");
const DEVICE_ROLES: &[u8] = include_bytes!("fixtures/execution/device-roles.1");

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
fn unsupported_execution_shapes_fail_before_returning_a_partial_report() {
    for (name, source) in [("table.1", TABLE), ("equation.1", EQUATION)] {
        let error = Parser::default()
            .with_input_format(InputFormat::Man)
            .execute_bytes(name, source, ExecutionLimits::default())
            .unwrap_err();
        assert_eq!(error.kind, ExecutionErrorKind::Unsupported);
    }

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
