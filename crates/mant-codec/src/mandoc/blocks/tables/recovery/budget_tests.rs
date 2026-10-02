//! Exact complete sources ran pristine CVS before these assertions. The
//! high-level-macro T{} cases are the documented GNU enhancement, while
//! `tbl_data.c::tbl_cdata` retains the native operand payload on refusal.

use super::{CellPosition, TableEmbeddingPlan, TableTextBlock, lower_table_cell};
use crate::mandoc::table_recovery_budget::{Exhaustion, Limits, TableRecoveryBudget};
use crate::mandoc::{LoweringContext, formatter::FormatterState};
use libmandoc_rs::{MacroSet, Node, NodeKind, Parser};
use mant_ir::{Diagnostic, DiagnosticImpact, DiagnosticLevel, Inline};

const MAN_STYLE: &str = include_str!("budget_tests/fixtures/man-style.1");
const ROWS: &str = include_str!("budget_tests/fixtures/cumulative-rows.1");
const STATE: &str = include_str!("budget_tests/fixtures/state-candidate.1");
const MIXED: &str = include_str!("budget_tests/fixtures/adjacent-unsafe-safe.1");

fn tables(node: &Node, output: &mut Vec<Node>) {
    if node.kind == NodeKind::Table {
        output.push(node.clone());
    }
    for child in &node.children {
        tables(child, output);
    }
}

fn parsed_tables(source: &str) -> (MacroSet, Vec<Node>) {
    let report = Parser::default()
        .parse_bytes("table-budget.1", source.as_bytes())
        .unwrap();
    let mut output = Vec::new();
    tables(&report.document.root, &mut output);
    (report.document.macro_set, output)
}

fn decode_cell(
    row: &Node,
    index: usize,
    context: &LoweringContext<'_>,
    block: &TableTextBlock,
    formatter: &mut FormatterState,
) -> Vec<Inline> {
    lower_table_cell(
        &row.table_cells[index],
        CellPosition {
            index,
            row: &row.table_cells,
        },
        row,
        context,
        Some(block),
        formatter,
    )
    .unwrap_or_default()
}

#[test]
fn every_enhancement_limit_preserves_the_complete_native_cell() {
    // Fixed CVS returns SAFE for this complete native T{} operand. Font
    // enrichment is optional; rejecting it must not erase or truncate SAFE.
    let (dialect, rows) = parsed_tables(MAN_STYLE);
    for limits in [
        Limits {
            attempts: 0,
            ..Limits::default()
        },
        Limits {
            input: 0,
            ..Limits::default()
        },
        Limits {
            output_nodes: 1,
            ..Limits::default()
        },
        Limits {
            output_bytes: 3,
            ..Limits::default()
        },
    ] {
        let mut context = LoweringContext::new(None, None);
        context.macro_set = dialect;
        *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(limits);
        let block = TableTextBlock {
            source: ".B SAFE".to_owned(),
            escape: Some(b'\\'),
        };
        let mut formatter = FormatterState::default();
        let output = decode_cell(&rows[0], 0, &context, &block, &mut formatter);
        assert_eq!(mant_ir::inline_plain_text(&output), "SAFE");
        assert!(
            output
                .iter()
                .all(|node| matches!(node, Inline::Text { .. }))
        );
        assert!(
            context
                .table_recovery_budget
                .borrow()
                .exhaustion()
                .is_some()
        );
    }
}

#[test]
fn safe_cells_share_attempts_across_rows_and_new_sessions_start_fresh() {
    let (dialect, rows) = parsed_tables(ROWS);
    let mut context = LoweringContext::new(None, Some(ROWS));
    context.macro_set = dialect;
    *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(Limits {
        attempts: 2,
        ..Limits::default()
    });
    let plan = TableEmbeddingPlan::new(&rows, &context);
    let mut formatter = FormatterState::default();
    for (index, (row, word)) in rows.iter().zip(["FIRST", "SECOND", "THIRD"]).enumerate() {
        let block = plan.embedding(index).unwrap().blocks[0].as_ref().unwrap();
        let output = decode_cell(row, 0, &context, block, &mut formatter);
        assert_eq!(mant_ir::inline_plain_text(&output), word);
        assert_eq!(
            matches!(output.first(), Some(Inline::Emphasis { .. })),
            index < 2
        );
    }
    assert_eq!(
        context.table_recovery_budget.borrow().exhaustion(),
        Some(Exhaustion::Attempts)
    );
    let mut fresh = LoweringContext::new(None, None);
    fresh.macro_set = dialect;
    let output = decode_cell(
        &rows[2],
        0,
        &fresh,
        &TableTextBlock {
            source: ".Em THIRD".to_owned(),
            escape: Some(b'\\'),
        },
        &mut FormatterState::default(),
    );
    assert!(matches!(output.first(), Some(Inline::Emphasis { .. })));
}

#[test]
fn unsafe_cell_does_not_spend_an_attempt_or_disable_its_safe_neighbor() {
    // tbl_mark_source_unsafe records the no-output user Em invocation on
    // the first cell; tbl_cdata keeps the second Sy cell independently safe.
    let (dialect, rows) = parsed_tables(MIXED);
    assert!(!rows[0].table_cells[0].source_recovery_safe);
    assert!(rows[0].table_cells[1].source_recovery_safe);
    let mut context = LoweringContext::new(None, Some(MIXED));
    context.macro_set = dialect;
    let plan = TableEmbeddingPlan::new(&rows, &context);
    let mut formatter = FormatterState::default();
    for (index, expected) in ["UNSAFE", "SAFE"].into_iter().enumerate() {
        let block = plan.embedding(0).unwrap().blocks[index].as_ref();
        assert_eq!(block.is_some(), index == 1);
        let output = lower_table_cell(
            &rows[0].table_cells[index],
            CellPosition {
                index,
                row: &rows[0].table_cells,
            },
            &rows[0],
            &context,
            block,
            &mut formatter,
        )
        .unwrap();
        assert_eq!(mant_ir::inline_plain_text(&output), expected);
        assert_eq!(context.table_recovery_budget.borrow().attempts(), index);
        if index == 1 {
            assert!(matches!(output.first(), Some(Inline::Strong { .. })));
        }
    }
}

#[test]
fn source_scan_stops_at_its_allowance_and_retains_only_complete_blocks() {
    let (dialect, rows) = parsed_tables(ROWS);
    let mut context = LoweringContext::new(None, Some(ROWS));
    context.macro_set = dialect;
    // Charge exact physical scan units through the first T}, then refuse
    // the next T{. tbl's native finalized rows remain available in all cases.
    let start = usize::try_from(rows[0].line - 1).unwrap();
    let mut units = 0;
    for line in ROWS.lines().skip(start) {
        units += line.len() + 1;
        if line == "T}" {
            break;
        }
    }
    *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(Limits {
        scan: units,
        ..Limits::default()
    });
    let blocks =
        context.table_text_blocks(rows[0].line, [true; 3].into_iter(), rows[0].table_escape);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].as_ref().unwrap().source, ".Em FIRST");
    assert_eq!(
        context.table_recovery_budget.borrow().exhaustion(),
        Some(Exhaustion::Scan)
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].table_cells[0].text.as_deref(), Some("THIRD"));
}

#[test]
fn output_refusal_does_not_commit_spacing_fonts_scope_posts_or_candidate_diagnostics() {
    let (dialect, rows) = parsed_tables(STATE);
    let mut context = LoweringContext::new(None, None);
    context.macro_set = dialect;
    *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(Limits {
        output_bytes: 3,
        ..Limits::default()
    });
    context.diagnostics.borrow_mut().push(Diagnostic {
        impact: DiagnosticImpact::ContentCoverage,
        level: DiagnosticLevel::Warning,
        code: Some("existing-gap".to_owned()),
        message: "existing".to_owned(),
        source: None,
    });
    let mut formatter = FormatterState::default();
    formatter.execution.scope_posts.finish(u32::MAX);
    let initial_font = formatter.font.clone();
    let output = decode_cell(
        &rows[0],
        0,
        &context,
        &TableTextBlock {
            source: ".Sm off\n.Em SAFE".to_owned(),
            escape: Some(b'\\'),
        },
        &mut formatter,
    );
    assert_eq!(mant_ir::inline_plain_text(&output), "off SAFE");
    assert!(formatter.spacing_enabled());
    assert_eq!(formatter.font, initial_font);
    assert!(formatter.execution.scope_posts.ended(u32::MAX));
    assert!(!formatter.execution.escape_coverage.truncated());
    assert_eq!(context.diagnostics.borrow().len(), 1);
    assert_eq!(
        context.diagnostics.borrow()[0].code.as_deref(),
        Some("existing-gap")
    );
}

#[test]
fn raw_fallback_spends_a_second_attempt_after_semantic_decline() {
    let source = include_str!("budget_tests/fixtures/raw-native-request.1");
    let (dialect, mut rows) = parsed_tables(source);
    // Foreign callers may lack native text. The isolated raw replay still
    // needs its own allowance and cannot borrow a declined candidate's quota.
    rows[0].table_cells[0].text = None;
    let mut context = LoweringContext::new(None, None);
    context.macro_set = dialect;
    *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(Limits {
        attempts: 1,
        ..Limits::default()
    });
    let output = decode_cell(
        &rows[0],
        0,
        &context,
        &TableTextBlock {
            source: ".B FIRST\n.br\nLAST".to_owned(),
            escape: Some(b'\\'),
        },
        &mut FormatterState::default(),
    );
    assert_eq!(output.len(), 0);
    assert_eq!(context.table_recovery_budget.borrow().attempts(), 2);
    assert_eq!(
        context.table_recovery_budget.borrow().exhaustion(),
        Some(Exhaustion::Attempts)
    );
}

#[test]
fn output_nodes_and_bytes_accept_the_exact_limit_without_flattening_styles() {
    let (dialect, rows) = parsed_tables(MAN_STYLE);
    for (nodes, bytes) in [(2, 4), (3, 5)] {
        let mut context = LoweringContext::new(None, None);
        context.macro_set = dialect;
        *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(Limits {
            output_nodes: nodes,
            output_bytes: bytes,
            ..Limits::default()
        });
        let output = decode_cell(
            &rows[0],
            0,
            &context,
            &TableTextBlock {
                source: ".B SAFE".to_owned(),
                escape: Some(b'\\'),
            },
            &mut FormatterState::default(),
        );
        assert_eq!(mant_ir::inline_plain_text(&output), "SAFE");
        assert!(matches!(output.first(), Some(Inline::Strong { .. })));
        assert_eq!(context.table_recovery_budget.borrow().exhaustion(), None);
    }
}

#[test]
fn raw_fallback_can_commit_when_its_distinct_attempt_is_available() {
    let source = include_str!("budget_tests/fixtures/raw-native-request.1");
    let (dialect, mut rows) = parsed_tables(source);
    rows[0].table_cells[0].text = None;
    let mut context = LoweringContext::new(None, None);
    context.macro_set = dialect;
    *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(Limits {
        attempts: 2,
        ..Limits::default()
    });
    let output = decode_cell(
        &rows[0],
        0,
        &context,
        &TableTextBlock {
            source: ".B FIRST\n.br\nLAST".to_owned(),
            escape: Some(b'\\'),
        },
        &mut FormatterState::default(),
    );
    assert_eq!(mant_ir::inline_plain_text(&output), "FIRST LAST");
    assert_eq!(context.table_recovery_budget.borrow().attempts(), 2);
    assert_eq!(context.table_recovery_budget.borrow().exhaustion(), None);
}

#[test]
fn overlong_metadata_is_refused_before_name_scanning_and_native_payload_survives() {
    // Exact page, including this 65,537-byte name, ran pristine first.
    // Metadata is outside the T{} cell; it must not bypass the local bound.
    let name = "N".repeat(crate::mandoc::table_recovery_budget::MAX_FRAGMENT_BYTES + 1);
    let source = format!(
        ".Dd October 3, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm {name}\n.Nd probe\n.Sh DESCRIPTION\n.TS\nl.\nT{{\n.Em SAFE\nT}}\n.TE\n.No AFTER\n"
    );
    let (dialect, rows) = parsed_tables(&source);
    let mut context = LoweringContext::new(Some(&name), None);
    context.macro_set = dialect;
    let output = decode_cell(
        &rows[0],
        0,
        &context,
        &TableTextBlock {
            source: ".Em SAFE".to_owned(),
            escape: Some(b'\\'),
        },
        &mut FormatterState::default(),
    );
    assert_eq!(mant_ir::inline_plain_text(&output), "SAFE");
    assert!(
        output
            .iter()
            .all(|node| matches!(node, Inline::Text { .. }))
    );
    assert_eq!(context.table_recovery_budget.borrow().exhaustion(), None);
    assert_eq!(
        context.table_recovery_budget.borrow().refused_fragments(),
        1
    );
    // The refusal belongs to the synthetic metadata input, not the shared
    // page allowance. An independently bounded fragment can still use it.
    assert!(
        context
            .table_recovery_budget
            .borrow_mut()
            .begin_candidate(".B SAFE".len())
    );
    let recovered = crate::mandoc::inline::lower_source_fragment_with_formatter_state(
        ".B SAFE",
        Some(b'\\'),
        MacroSet::Man,
        None,
        false,
        FormatterState::default(),
        Some(&context.table_recovery_budget),
    )
    .unwrap();
    assert!(recovered.complete);
    assert!(matches!(
        recovered.inlines.first(),
        Some(Inline::Strong { .. })
    ));
}

#[test]
fn local_request_cap_preserves_every_operand_at_sixty_three_sixty_four_and_sixty_five() {
    for (count, source) in [
        (63, include_str!("budget_tests/fixtures/requests-63.1")),
        (64, include_str!("budget_tests/fixtures/requests-64.1")),
        (65, include_str!("budget_tests/fixtures/requests-65.1")),
    ] {
        let (dialect, rows) = parsed_tables(source);
        let mut context = LoweringContext::new(None, Some(source));
        context.macro_set = dialect;
        let plan = TableEmbeddingPlan::new(&rows, &context);
        let output = decode_cell(
            &rows[0],
            0,
            &context,
            plan.embedding(0).unwrap().blocks[0].as_ref().unwrap(),
            &mut FormatterState::default(),
        );
        let output_text = mant_ir::inline_plain_text(&output);
        assert_eq!(
            output_text.split_whitespace().collect::<Vec<_>>(),
            vec!["WORD"; count]
        );
        assert_eq!(
            output
                .iter()
                .any(|node| matches!(node, Inline::Strong { .. })),
            count <= 64
        );
        assert_eq!(context.table_recovery_budget.borrow().exhaustion(), None);
    }
}

#[test]
fn oversized_cells_preserve_ordinals_and_do_not_poison_safe_neighbors() {
    // Both complete generated pages ran pristine CVS first. tbl_cdata ends
    // the first cell at T} and getdata admits an adjacent T} TAB T{ cell;
    // a local enhancement size refusal cannot change either native payload.
    let long = "A".repeat(crate::mandoc::table_recovery_budget::MAX_FRAGMENT_BYTES + 1);
    for adjacent in [true, false] {
        let columns = if adjacent { "l l." } else { "l." };
        let boundary = if adjacent { "T}\tT{" } else { "T}\nT{" };
        let source = format!(
            ".TH TEST 1 \"2026-10-03\"\n.SH DESCRIPTION\n.TS\n{columns}\nT{{\n.B {long}\n{boundary}\n.B SAFE\nT}}\n.TE\nAFTER\n"
        );
        let (dialect, rows) = parsed_tables(&source);
        let mut context = LoweringContext::new(None, Some(&source));
        context.macro_set = dialect;
        let plan = TableEmbeddingPlan::new(&rows, &context);
        let first = plan.embedding(0).unwrap();
        assert!(first.blocks[0].is_none());
        let first_output = lower_table_cell(
            &rows[0].table_cells[0],
            CellPosition {
                index: 0,
                row: &rows[0].table_cells,
            },
            &rows[0],
            &context,
            None,
            &mut FormatterState::default(),
        )
        .unwrap();
        assert_eq!(mant_ir::inline_plain_text(&first_output), long);
        let (row_index, cell_index, block_index) = if adjacent { (0, 1, 1) } else { (1, 0, 0) };
        let block = plan.embedding(row_index).unwrap().blocks[block_index]
            .as_ref()
            .unwrap();
        assert_eq!(block.source, ".B SAFE");
        let output = decode_cell(
            &rows[row_index],
            cell_index,
            &context,
            block,
            &mut FormatterState::default(),
        );
        assert_eq!(mant_ir::inline_plain_text(&output), "SAFE");
        assert!(matches!(output.first(), Some(Inline::Strong { .. })));
        assert_eq!(
            context.table_recovery_budget.borrow().refused_fragments(),
            1
        );
        assert_eq!(context.table_recovery_budget.borrow().exhaustion(), None);
        assert_eq!(context.table_recovery_budget.borrow().attempts(), 1);
    }
}

#[test]
fn unavailable_source_does_not_attempt_optional_recovery_or_damage_native_payload() {
    let (dialect, rows) = parsed_tables(MAN_STYLE);
    let mut context = LoweringContext::new(None, None);
    context.macro_set = dialect;
    let plan = TableEmbeddingPlan::new(&rows, &context);
    assert!(plan.embedding(0).is_none());
    let output = lower_table_cell(
        &rows[0].table_cells[0],
        CellPosition {
            index: 0,
            row: &rows[0].table_cells,
        },
        &rows[0],
        &context,
        None,
        &mut FormatterState::default(),
    )
    .unwrap();
    assert_eq!(mant_ir::inline_plain_text(&output), "SAFE");
    assert_eq!(context.table_recovery_budget.borrow().attempts(), 0);
    assert_eq!(context.table_recovery_budget.borrow().exhaustion(), None);
}

#[test]
fn ownership_comparison_charges_both_inputs_before_visiting_native_payload() {
    // The exact native operand is SAFE in the pristine man-style page.
    // This isolates ownership admission from earlier synthetic parse charges.
    let (_, rows) = parsed_tables(MAN_STYLE);
    for input in [7, 8, 9] {
        let context = LoweringContext::new(None, None);
        *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(Limits {
            input,
            ..Limits::default()
        });
        let candidate = super::CellCandidate {
            inlines: vec![Inline::Text {
                value: "SAFE".to_owned(),
            }],
            formatter: FormatterState::default(),
            diagnostics: Vec::new(),
            output_text_bytes: 4,
        };
        assert_eq!(
            candidate.belongs_to(
                &rows[0].table_cells[0],
                CellPosition {
                    index: 0,
                    row: &rows[0].table_cells
                },
                "SAFE",
                true,
                &context
            ),
            input >= 8
        );
        assert_eq!(
            context.table_recovery_budget.borrow().exhaustion(),
            if input < 8 {
                Some(Exhaustion::Input)
            } else {
                None
            }
        );
    }
}
