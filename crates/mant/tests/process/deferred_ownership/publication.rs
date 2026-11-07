//! Execute every publication source and explicit query through the shipped CLI.
use crate::process_support::{executable, run_text_command};
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

#[path = "publication/entries.rs"]
mod entries;
#[path = "publication/members.rs"]
mod members;
#[path = "publication/mutations.rs"]
mod mutations;

const CASES: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/annotation_publication/cases.jsonl");

#[derive(Deserialize)]
struct Case<'a> {
    id: &'a str,
    family: &'a str,
    axes: Vec<&'a str>,
    source: String,
    queries: Vec<&'a str>,
}

impl Case<'_> {
    fn output(&self, binary: &str, format: &str, arguments: &[&str]) -> Vec<u8> {
        let mut command = Command::new(binary);
        command
            .args([
                "--input",
                "-",
                "--input-format",
                "roff",
                "--display",
                "direct",
                "--color",
                "never",
                "--format",
                format,
            ])
            .args(arguments);
        let result = run_text_command(command, &self.source);
        assert!(
            result.status.success(),
            "{} {arguments:?}: {result:?}",
            self.id
        );
        result.stdout
    }

    fn json(&self, binary: &str, arguments: &[&str]) -> Value {
        serde_json::from_slice(&self.output(binary, "json", arguments)).unwrap()
    }

    fn check(&self, binary: &str) {
        let transport = self.json(binary, &["--compact"]);
        let document = &transport["document"];
        let bundle: mant_protocol::QueryBundle = serde_json::from_value(transport.clone()).unwrap();
        let roundtripped: mant_ir::Document = bundle.document.unwrap().into();
        let section = &document["sections"][1];
        let blocks = array(&section["blocks"]);
        assert!(!blocks.is_empty(), "{} body missing", self.id);

        let outline = self.json(
            binary,
            &[
                "--compact",
                "--outline",
                "--outline-references",
                "all",
                "--reference-types",
                "external,email,manual",
            ],
        );
        let parsed_outline: mant_protocol::QueryOutline =
            serde_json::from_value(outline.clone()).unwrap();
        let inventory = &outline["references"];
        let links = descendants(&section["blocks"], "link");
        let records = array(&inventory["records"]);
        assert_eq!(inventory["occurrences"]["kind"], "exact", "{}", self.id);
        assert_eq!(records.len(), links.len(), "{} occurrence count", self.id);
        for (index, (record, link)) in records.iter().zip(&links).enumerate() {
            assert_eq!(record["target"], link["target"], "{} target order", self.id);
            let occurrence = &parsed_outline.references.records[index];
            let Some(mant_ir::Inline::Link {
                target, children, ..
            }) = occurrence.origin.resolve_link(&roundtripped)
            else {
                panic!("{} outline origin did not resolve a link", self.id);
            };
            let label = inline_text(&link["children"]);
            assert_eq!(target, &occurrence.target, "{} origin target", self.id);
            assert_eq!(
                mant_ir::inline_plain_text(children),
                label,
                "{} origin label",
                self.id
            );
            assert_eq!(occurrence.label, label, "{} outline label", self.id);
            assert!(
                !occurrence.label_truncated,
                "{} unexpected truncation",
                self.id
            );
            assert!(
                parsed_outline.references.records[..index]
                    .iter()
                    .all(|previous| previous.origin != occurrence.origin),
                "{} duplicate outline origin",
                self.id,
            );
        }

        match self.family {
            "N1a" => entries::head(self, blocks),
            "N1b" => entries::group(self, blocks),
            "N1c" => entries::hanging(self, blocks),
            "N2" | "X-prefix-gap" => members::prefix(self, &links),
            "N3" => members::gap(self, binary, blocks, &links),
            "N4" => members::row(self, binary, &links),
            "E12" | "E12extra" | "X-head-marker" => members::manual(self, section, &links),
            "E3" | "X-line-eqn" => members::equation(self, document, section, &links),
            _ => panic!("unknown case {}", self.id),
        }
        if self.family == "X-prefix-gap" {
            members::gap(self, binary, blocks, &links);
        }

        for query in &self.queries {
            let flag = format!("--explain={query}");
            let response = self.json(binary, &["--compact", &flag]);
            assert_eq!(
                response["query"]["entry"], *query,
                "{} explicit query",
                self.id
            );
            let explanation: mant_protocol::QueryExplanation =
                serde_json::from_value(response.clone()).unwrap();
            assert!(
                explanation.validate_references().is_ok(),
                "{} {query}",
                self.id
            );
            match self.family {
                "N1a" | "N1b" | "N1c" | "X-head-marker" => {
                    entries::query(self, binary, query, blocks, &response, &explanation);
                }
                "E3" | "X-line-eqn" => {
                    members::explanation_coverage(self, &explanation);
                }
                _ => {}
            }
        }
    }
}

fn array(value: &Value) -> &[Value] {
    value.as_array().expect("array")
}

fn descendants<'a>(root: &'a Value, kind: &str) -> Vec<&'a Value> {
    fn visit<'a>(root: &'a Value, kind: &str, found: &mut Vec<&'a Value>) {
        match root {
            Value::Array(values) => {
                for value in values {
                    visit(value, kind, found);
                }
            }
            Value::Object(fields) => {
                if root["type"] == kind {
                    found.push(root);
                }
                for key in [
                    "terms",
                    "description",
                    "blocks",
                    "children",
                    "content",
                    "items",
                    "rows",
                    "cells",
                ] {
                    if let Some(value) = fields.get(key) {
                        visit(value, kind, found);
                    }
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    visit(root, kind, &mut found);
    found
}

fn inline_text(value: &Value) -> String {
    match value {
        Value::Array(values) => values.iter().map(inline_text).collect(),
        Value::Object(_) if value["type"] == "line-break" => "\n".into(),
        Value::Object(_) if value["value"].is_string() => value["value"].as_str().unwrap().into(),
        Value::Object(_) => inline_text(&value["children"]),
        _ => String::new(),
    }
}

fn cases() -> impl Iterator<Item = Case<'static>> {
    CASES
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
}

fn check_families(families: &[&str], expected: usize) {
    let mut count = 0;
    for case in cases().filter(|case| families.contains(&case.family)) {
        count += 1;
        case.check(executable());
    }
    assert_eq!(count, expected);
}

#[test]
fn publication_definition_owners_and_queries() {
    check_families(&["N1a", "N1b", "N1c", "X-head-marker"], 96);
}

#[test]
fn publication_author_label_and_native_rows() {
    check_families(&["N2", "N3", "N4", "X-prefix-gap"], 308);
}

#[test]
fn publication_manual_references_and_equation_coverage() {
    check_families(&["E12", "E12extra", "E3", "X-line-eqn"], 100);
}

#[test]
#[ignore = "replay one exact case against a frozen audit binary"]
fn publication_frozen_case() {
    let binary = std::env::var("MANT_PUBLICATION_REPLAY").unwrap();
    let case_id = std::env::var("MANT_PUBLICATION_CASE").unwrap();
    let case = cases().find(|case| case.id == case_id).unwrap();
    case.check(&binary);
}
