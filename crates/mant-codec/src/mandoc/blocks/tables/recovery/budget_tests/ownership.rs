//! Real source-order execution with private recovery refusals, never a mock
//! cell or a public CLI run mislabeled as budget exhaustion.
use super::*;
use mant_ir::{Block, LinkTarget, visit::Visit};

#[derive(Default)]
struct Output {
    roots: Vec<String>,
    links: Vec<(LinkTarget, String)>,
    cells: Vec<String>,
}
impl<'a> Visit<'a> for Output {
    fn visit_block(&mut self, block: &'a Block) {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                self.roots.push(mant_ir::inline_plain_text(children));
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        self.cells.push(
                            cell.blocks
                                .iter()
                                .map(|block| match block {
                                    Block::Paragraph { children, .. }
                                    | Block::Preformatted { children, .. } => {
                                        mant_ir::inline_plain_text(children)
                                    }
                                    _ => panic!("unexpected fallback owner"),
                                })
                                .collect::<String>(),
                        );
                    }
                }
            }
            _ => {}
        }
        mant_ir::visit::walk_block(self, block);
    }
    fn visit_inline(&mut self, node: &'a Inline) {
        if let Inline::Link {
            target, children, ..
        } = node
        {
            self.links
                .push((target.clone(), mant_ir::inline_plain_text(children)));
        }
        mant_ir::visit::walk_inline(self, node);
    }
}

#[test]
fn rejected_candidates_cannot_reassign_committed_members_or_later_receivers() {
    // All four complete sources ran pristine ASCII/UTF8/HTML/tree/lint first.
    // tbl_data.c::tbl_cdata provides the base operand stream on refusal;
    // term.c::encode1 records a graph before BACKBEFORE. Recovery spelling
    // remains ManT content, and never becomes an adjacent link's label.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/deferred-ownership.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        for variant in case["budget_variants"].as_array().unwrap() {
            let report = Parser::default()
                .parse_bytes("ownership-budget.1", source.as_bytes())
                .unwrap();
            let mut context = LoweringContext::new(Some("probe"), Some(source));
            context.macro_set = report.document.macro_set;
            context
                .scope_posts
                .index_structural_payload(&report.document.root);
            let limits = limits(variant);
            *context.table_recovery_budget.borrow_mut() = TableRecoveryBudget::with_limits(limits);
            let (mut blocks, mut sections) = crate::mandoc::blocks::lower_document_structure(
                &report.document.root,
                &mut context,
            );
            let expected = match variant["expected_exhaustion"].as_str() {
                Some("Attempts") => Some(Exhaustion::Attempts),
                Some("OutputNodes") => Some(Exhaustion::OutputNodes),
                Some("OutputBytes") => Some(Exhaustion::OutputBytes),
                None => None,
                _ => panic!("unregistered exhaustion"),
            };
            assert_eq!(
                context.table_recovery_budget.borrow().exhaustion(),
                expected,
                "{} {variant}",
                case["id"]
            );
            if let Some(count) = variant["expected_attempts"].as_u64() {
                assert_eq!(
                    context.table_recovery_budget.borrow().attempts(),
                    usize::try_from(count).unwrap()
                );
            }
            context.annotations.finish(
                &mut blocks,
                &mut sections,
                &mut context.native_heads.borrow_mut(),
            );
            let mut output = Output::default();
            for block in &blocks {
                output.visit_block(block);
            }
            for section in &sections {
                output.visit_section(section);
            }
            let labels = variant["expected_active_labels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                output
                    .links
                    .iter()
                    .map(|(_, text)| text.as_str())
                    .collect::<Vec<_>>(),
                labels,
                "{} {variant}",
                case["id"]
            );
            for (target, _) in &output.links {
                assert_eq!(
                    target,
                    &LinkTarget::External {
                        uri: "https://shared.test".into()
                    }
                );
            }
            assert_cells(&output, case, variant);
            let reading = output.roots.join("\n");
            for word in ["OLDLINK", "OLDACCEPTED", "AFTERLINK", "AFTERTEXT"] {
                assert_eq!(reading.matches(word).count(), 1, "{reading}");
            }
            let diagnostics = context.take_diagnostics();
            assert_eq!(
                diagnostics
                    .iter()
                    .filter(|d| d.code.as_deref() == Some("manual.table-recovery-budget"))
                    .count(),
                usize::from(expected.is_some())
            );
            assert!(!diagnostics.iter().any(|d| matches!(
                d.code.as_deref(),
                Some("manual.table-execution-incomplete" | "manual.escape-scan-truncated")
            )));
            let wire = serde_json::to_string(&(blocks, sections)).unwrap();
            assert!(!wire.contains("mant:deferred-origin:"));
        }
    }
}

fn limits(variant: &serde_json::Value) -> Limits {
    let mut limits = Limits::default();
    for (key, value) in variant["limits"].as_object().unwrap() {
        let value = usize::try_from(value.as_u64().unwrap()).unwrap();
        match key.as_str() {
            "attempts" => limits.attempts = value,
            "output_nodes" => limits.output_nodes = value,
            "output_bytes" => limits.output_bytes = value,
            _ => panic!("unregistered limit"),
        }
    }
    limits
}

fn assert_cells(output: &Output, case: &serde_json::Value, variant: &serde_json::Value) {
    if let Some(cells) = variant["expected_safe_cell_text"].as_array() {
        assert_eq!(
            output.cells.len(),
            cells.len() + 1,
            "{} final native bare-z column",
            case["id"]
        );
        assert_eq!(output.cells.last().unwrap(), "");
        for (cell, gold) in output.cells.iter().zip(cells) {
            assert_eq!(cell, gold.as_str().unwrap(), "{} {variant}", case["id"]);
        }
    } else {
        assert_eq!(output.cells.len(), 3);
        assert_eq!(output.cells[2], "");
        assert_eq!(
            output.cells[0],
            variant["expected_first_cell_text"].as_str().unwrap()
        );
        assert_eq!(
            output.cells[1],
            variant["expected_second_safe_cell_text"].as_str().unwrap()
        );
    }
}
