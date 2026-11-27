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

#[test]
fn backbefore_keeps_the_authored_link_prefix_until_a_word_rearms_it() {
    // CVS term.c::encode1() retreats over the leading blank before the
    // graph. With a single pending glyph the same visible blank can keep
    // its source ownership, as in man_html.c::man_UR_pre(). A second \z
    // rearms the retreat at A, which L then overstrikes.
    for (operand, label, row) in [
        (" ALPHA", " ALPHA", "PREX ALPHA <https://outer.test> TAIL"),
        ("\\z ALPHA", "LPHA", "PREX LPHA <https://outer.test> TAIL"),
    ] {
        let case = serde_json::json!({
            "id": operand,
            "source": format!(
                ".TH REVIEW 1 \"November 7, 2026\"\n.SH NAME\nreview \\- demonstrate delayed link ownership\n.SH DESCRIPTION\nPRE\\zX\n.UR https://outer.test\n.B \"{operand}\"\n.UE\nTAIL\n"
            ),
        });
        let bundle: mant_protocol::QueryBundle =
            serde_json::from_slice(&run(&case, "json", "never").stdout).unwrap();
        let content: mant_ir::ResolvedContent = bundle.into();
        let mut links = Links::default();
        links.visit_document(content.document.as_ref().unwrap());
        assert_eq!(
            links.0,
            [(
                mant_ir::LinkTarget::External {
                    uri: "https://outer.test".into()
                },
                label.into()
            )],
            "{operand}"
        );
        let plain = String::from_utf8(run(&case, "text", "never").stdout).unwrap();
        assert_eq!(
            plain
                .split_once("DESCRIPTION\n")
                .unwrap()
                .1
                .trim_end_matches('\n'),
            row
        );
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

const LINK_COLON_URI: &str = "https://z.test";
// Earlier source, movement, output writer, inset context, native row, label.
type LinkColonCase<'a> = (&'a str, &'a str, &'a str, bool, &'a str, Option<&'a str>);
const LINK_COLON_CASES: [LinkColonCase<'_>; 9] = [
    (
        "Y",
        "\\h'|0n'R",
        ".Lk https://z.test Z",
        false,
        "REZ:Yhttps://z.test END",
        Some("Z"),
    ),
    (
        "Z",
        "\\h'|0n'R",
        ".Lk https://z.test Z",
        false,
        "REZ:Zhttps://z.test END",
        Some("Z"),
    ),
    (
        ".Sy Y",
        "\\h'|0n'R",
        ".Lk https://z.test Z",
        false,
        "REZ:Yhttps://z.test END",
        Some("Z"),
    ),
    (
        "Y",
        "\\h'|0n'R",
        ".Lk https://z.test ZX",
        false,
        "REZX: https://z.test END",
        Some("ZX"),
    ),
    (
        "Y",
        "\\h'|0n'R",
        ".Lk https://z.test Z",
        true,
        "REZ:Yhttps://z.test END",
        Some("Z"),
    ),
    (
        "Y",
        "\\h'-7n'R",
        ".Lk https://z.test Z",
        false,
        "HERDZ: https://z.test END",
        Some("Z"),
    ),
    (
        "Y",
        "R",
        ".Lk https://z.test Z",
        false,
        "HEAD Y R Z: https://z.test END",
        Some("Z"),
    ),
    (
        "Y",
        "\\h'|0n'R",
        ".Lk https://z.test",
        false,
        "REhttps://z.test END",
        Some(LINK_COLON_URI),
    ),
    (
        "Y",
        "\\h'|0n'R",
        ".No Z : https://z.test",
        false,
        "REZ:https://z.test END",
        None,
    ),
];

#[test]
fn generated_link_colon_writes_through_the_live_cursor_without_owning_old_ink() {
    // CVS mdoc_term.c::termp_lk_pre() executes the label, an unspaced
    // term_word(":"), then the address. term.c::bufferc() advances col
    // through the existing lastcol, rather than appending at lastcol.
    // The pinned ASCII and UTF-8 devices agree on these exact body rows.
    const HEAD: &str = ".Dd November 27, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd annotations\n.Sh DESCRIPTION\n";
    for (previous, motion, link, inset, expected, label) in LINK_COLON_CASES {
        let (open, close) = if inset {
            (".Bl -inset\n.It\n", ".El\n")
        } else {
            ("", "")
        };
        let source =
            format!("{HEAD}{open}.No HEAD\n{previous}\n.No \"{motion}\"\n{link}\nEND\n{close}");
        let case = serde_json::json!({ "id": &source, "source": &source });
        let plain = String::from_utf8(run(&case, "text", "never").stdout).unwrap();
        let (_, body) = plain.split_once("DESCRIPTION\n").unwrap();
        assert_eq!(body.trim(), expected, "{source}");

        let bundle: mant_protocol::QueryBundle =
            serde_json::from_slice(&run(&case, "json", "never").stdout).unwrap();
        let content: mant_ir::ResolvedContent = bundle.into();
        let mut links = Links::default();
        links.visit_document(content.document.as_ref().unwrap());
        let expected_link = label.map(|label| {
            (
                mant_ir::LinkTarget::External {
                    uri: LINK_COLON_URI.into(),
                },
                label.to_owned(),
            )
        });
        assert_eq!(
            links.0,
            expected_link.into_iter().collect::<Vec<_>>(),
            "{source}"
        );

        let markdown = String::from_utf8(run(&case, "markdown", "never").stdout).unwrap();
        if let Some(label) = label.filter(|label| *label != LINK_COLON_URI) {
            assert!(
                markdown.contains(&format!("[*{label}*]({LINK_COLON_URI})\\:")),
                "{source}\n{markdown}"
            );
            assert_eq!(markdown.matches(&format!("]({LINK_COLON_URI})")).count(), 1);
        }
        if previous == ".Sy Y" {
            assert!(markdown.contains(r"\:**Y**https"), "{markdown}");
        }
    }
}
