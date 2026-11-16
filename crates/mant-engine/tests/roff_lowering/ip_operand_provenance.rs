//! Native label execution and its lowered definition/list ownership.

use super::{json_roundtrip, oracle_receipt};
use libmandoc_rs::{MacroToken, ManMacro, Node, NodeKind, Parser};
use mant_ir::{Block, EntryOwner, ListKind, ResolvedContent};
use mant_protocol::{ContentSelector, ExplanationOptions, ExplanationQuery};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
struct Label<'a> {
    text: &'a str,
    counter: bool,
}

#[derive(Deserialize)]
struct Matrix<'a> {
    #[serde(borrow)]
    cases: Vec<Case<'a>>,
}

#[derive(Deserialize)]
struct Case<'a> {
    id: String,
    group: String,
    source: String,
    source_sha256: String,
    #[serde(borrow)]
    labels: Vec<Label<'a>>,
    runs: BTreeMap<String, RecordedRun>,
    #[serde(default)]
    bodies: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct RecordedRun {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
    diagnostics: String,
}

fn native_labels<'a>(node: &'a Node, labels: &mut Vec<Label<'a>>) {
    if node.kind == NodeKind::Head
        && node.macro_token == Some(MacroToken::Man(ManMacro::Ip))
        && let Some(label) = node.children.first()
    {
        labels.push(Label {
            text: label.text.as_deref().unwrap(),
            counter: label.flags.ip_incremented_register,
        });
    }
    for child in &node.children {
        native_labels(child, labels);
    }
}

fn explained_bodies(query: &ResolvedContent, name: &str) -> Vec<String> {
    let response = mant_query::explain_query(
        query,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    response
        .evidence
        .iter()
        .filter(|entry| entry.class == mant_protocol::EvidenceClass::DirectEntry)
        .map(|entry| {
            let excerpt =
                mant_query::select_excerpt(query, &[ContentSelector::path(entry.outline.path())])
                    .unwrap();
            mant_render::render_excerpt_text(&excerpt)
        })
        .collect()
}

fn load_and_check(source: &str, expected: &[Label<'_>]) -> ResolvedContent {
    let ast = Parser::default()
        .parse_bytes("probe.1", source.as_bytes())
        .unwrap();
    let mut native = Vec::new();
    native_labels(&ast.document.root, &mut native);
    assert_eq!(native, expected, "{source}");
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let restored = json_roundtrip::json_string_roundtrip(&original);
    let document = restored.document.as_ref().unwrap();
    assert_eq!(mant_ir::validate_document(document), [], "{source}");
    let mut projected = Vec::new();
    for block in &document.sections[0].blocks {
        match block {
            Block::DefinitionList { items, .. } => {
                for item in items {
                    let label = mant_ir::inline_plain_text(&item.terms[0]);
                    if matches!(label.as_str(), "7" | "8") {
                        let owner = EntryOwner::Definition(item);
                        assert!(
                            owner
                                .facts()
                                .unwrap()
                                .id
                                .as_str()
                                .starts_with(&format!("term-{label}"))
                        );
                        let forms = owner.forms().unwrap();
                        assert_eq!(
                            mant_ir::inline_plain_text(forms.iter().next().unwrap()),
                            label,
                            "{source}"
                        );
                    }
                    projected.push((label, false));
                }
            }
            Block::List {
                kind: ListKind::Ordered { start: Some(first) },
                items,
                ..
            } => projected
                .extend((0..items.len()).map(|offset| ((first + offset as u64).to_string(), true))),
            _ => panic!("unexpected content {block:?}: {source}"),
        }
    }
    assert_eq!(
        projected
            .iter()
            .map(|(text, counter)| Label {
                text,
                counter: *counter
            })
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|label| Label {
                counter: label.counter && !label.text.starts_with('-'),
                ..*label
            })
            .collect::<Vec<_>>(),
        "{source}"
    );
    restored
}

fn check_cases(group: &str) {
    let matrix: Matrix<'_> =
        serde_json::from_str(include_str!("ip_operand_provenance/cases.json")).unwrap();
    let cases: Vec<_> = matrix
        .cases
        .iter()
        .filter(|case| case.group == group)
        .collect();
    assert!(!cases.is_empty(), "{group}");
    for case in cases {
        oracle_receipt::assert_receipts(
            &case.id,
            &case.source_sha256,
            case.runs.iter().map(|(profile, run)| {
                (
                    profile.as_str(),
                    oracle_receipt::OracleReceipt {
                        status: run.status,
                        stdout_sha256: &run.stdout_sha256,
                        stderr_sha256: Some(&run.stderr_sha256),
                        diagnostics: &run.diagnostics,
                    },
                )
            }),
        );
        let query = load_and_check(&case.source, &case.labels);
        let markdown = mant_codec::encode::render_markdown(&query);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for (name, expected) in &case.bodies {
            let bodies = explained_bodies(&query, name);
            assert_eq!(bodies.len(), expected.len(), "{} {name}", case.id);
            for (body, expected) in bodies.iter().zip(expected) {
                assert!(body.contains(expected), "{} {body}", case.id);
                for output in [
                    mant_render::render_query_man(&query),
                    mant_render::render_query_man(&readback),
                ] {
                    assert!(
                        output.find(name).unwrap() < output.find(expected).unwrap(),
                        "{} {output}",
                        case.id
                    );
                }
            }
        }
    }
}

#[test]
fn discarded_macro_arguments_leave_the_same_definition_owner() {
    // roff_userdef() places the replacement on the invocation's line.
    // The actual SavedIP label is literal in every ignored-argument control.
    check_cases("ignored");
}

#[test]
fn executed_increment_expansions_follow_real_macro_operands() {
    // roff_getregn() executes the increment; blk_imp() assigns the resulting
    // label only after roff_getarg() has consumed the first IP operand.
    check_cases("increment");
}

#[test]
fn unrelated_expansions_and_fresh_loop_lines_cannot_claim_literal_labels() {
    // read.c restores separate raw lines from the .while lookaside. Both IP
    // labels occupy the same offset and the replay lines share coordinates.
    check_cases("unrelated");
}
