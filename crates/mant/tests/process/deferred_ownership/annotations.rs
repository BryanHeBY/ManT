//! New source-instance cases through the actual CLI and Unix terminal route.
use super::run_text_input;
use mant_ir::visit::{self, Visit};
use serde_json::Value;

fn cases() -> Vec<Value> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../mant-codec/src/mandoc/tests/annotation_ownership/boundaries.json"
    ))
    .unwrap();
    fixture["cases"].as_array().unwrap().clone()
}

#[derive(Default)]
struct Links(Vec<(mant_ir::LinkTarget, String)>);
impl<'a> Visit<'a> for Links {
    fn visit_inline(&mut self, node: &'a mant_ir::Inline) {
        if let mant_ir::Inline::Link {
            target, children, ..
        } = node
        {
            self.0
                .push((target.clone(), mant_ir::inline_plain_text(children)));
        }
        visit::walk_inline(self, node);
    }
}

fn run(case: &Value, format: &str, color: &str) -> std::process::Output {
    let result = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            format,
            "--display",
            "direct",
            "--color",
            color,
        ],
        case["source"].as_str().unwrap(),
    );
    assert!(result.status.success(), "{} {result:?}", case["id"]);
    result
}

#[test]
fn source_occurrences_and_native_body_members_survive_real_json_and_color_outputs() {
    for case in cases() {
        let result = run(&case, "json", "never");
        let bundle: mant_protocol::QueryBundle = serde_json::from_slice(&result.stdout).unwrap();
        let content: mant_ir::ResolvedContent = bundle.into();
        let mut links = Links::default();
        links.visit_document(content.document.as_ref().unwrap());
        let expected = case["expected"]["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|link| {
                (
                    serde_json::from_value::<mant_ir::LinkTarget>(link["target"].clone()).unwrap(),
                    link["label"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(links.0, expected, "{} actual CLI JSON", case["id"]);
        let plain = String::from_utf8(run(&case, "text", "never").stdout).unwrap();
        let ansi = String::from_utf8(run(&case, "text", "always").stdout).unwrap();
        assert_eq!(
            plain,
            super::super::execution_scopes::without_sgr(&ansi),
            "{} color",
            case["id"]
        );
        if let Some(body) = case["expected"]["body_text"].as_str() {
            assert!(
                plain.contains(body),
                "{} exact CLI body {body:?}: {plain}",
                case["id"]
            );
        }
        for word in case["expected"]["outside_link_literals"]
            .as_array()
            .unwrap()
        {
            assert!(
                plain.contains(word.as_str().unwrap()),
                "{} outside source",
                case["id"]
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn original_born_members_reach_both_actual_terminal_widths() {
    let selected = cases()
        .into_iter()
        .filter(|case| case["source"].as_str().unwrap().contains("\nAFTER\n"))
        .step_by(4)
        .map(|mut case| {
            let fragments = if let Some(text) = case["expected"]["body_text"].as_str() {
                vec![text.to_owned()]
            } else {
                case["expected"]["links"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|link| {
                        link["label"]
                            .as_str()
                            .filter(|label| !label.is_empty())
                            .map(str::to_owned)
                    })
                    .collect()
            };
            case["required_fragments"] = fragments
                .into_iter()
                .map(|text| {
                    // Named-0010 has a source Sx NEXT and the distinct NEXT
                    // heading. All other selected complete fragments are unique.
                    let count = if text == "NEXT" {
                        assert!(case["source"].as_str().unwrap().contains(".Sh NEXT\n"));
                        2
                    } else {
                        1
                    };
                    serde_json::json!({"text": text, "count": count})
                })
                .collect::<Vec<_>>()
                .into();
            case
        })
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 15);
    super::check_pty(&selected);
}
