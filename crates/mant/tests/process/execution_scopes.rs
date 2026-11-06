//! Native scope receipts also reach the public plain/ANSI CLI path.

use super::process_support::run_text_input;
use serde_json::Value;

fn without_sgr(text: &str) -> String {
    let mut output = String::new();
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character != '\u{1b}' {
            output.push(character);
            continue;
        }
        assert_eq!(chars.next(), Some('['), "only trusted SGR is generated");
        let mut ended = false;
        for control in chars.by_ref() {
            if control.is_ascii_alphabetic() {
                assert_eq!(control, 'm');
                ended = true;
                break;
            }
            assert!(control.is_ascii_digit() || control == ';');
        }
        assert!(ended, "complete SGR control");
    }
    output
}

#[test]
fn source_scope_core_keeps_native_rows_and_glyphs_through_cli_color_policy() {
    // All 236 complete core sources have five sealed pristine profiles.
    // man_term.c::post_SY, first-span term_tbl entry, and mdoc It/display
    // pre/post supply the independent expected rows in this manifest.
    let matrix: Value = serde_json::from_str(include_str!(
        "../../../mant-engine/tests/roff_lowering/execution_scopes/cases.json"
    ))
    .unwrap();
    let mut checked = 0;
    for case in matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| matches!(case["family"].as_str(), Some("M1-core" | "M2" | "M3")))
    {
        checked += 1;
        let mut outputs = Vec::new();
        for color in ["never", "always"] {
            let result = run_text_input(
                &[
                    "--input",
                    "-",
                    "--input-format",
                    "roff",
                    "--format",
                    "text",
                    "--display",
                    "direct",
                    "--color",
                    color,
                ],
                case["source"].as_str().unwrap(),
            );
            assert!(result.status.success(), "{}: {result:?}", case["id"]);
            outputs.push(String::from_utf8(result.stdout).unwrap());
        }
        assert!(
            outputs[1].contains("\u{1b}["),
            "{}: color is exercised",
            case["id"]
        );
        assert_eq!(without_sgr(&outputs[1]), outputs[0], "{}", case["id"]);
        if let Some(rows) = case["expected"]["prose_rows"].as_array() {
            for row in rows {
                let text = row["text"].as_str().unwrap();
                let matching: Vec<_> = outputs[0]
                    .lines()
                    .filter(|line| line.contains(text))
                    .collect();
                assert_eq!(
                    matching.len(),
                    1,
                    "{}: original native hard row",
                    case["id"]
                );
                assert_eq!(
                    mant_ir::geometry::text_width(&matching[0][..matching[0].find(text).unwrap()]),
                    usize::try_from(row["column"].as_u64().unwrap()).unwrap()
                );
            }
        }
        if let Some(glyphs) = case["expected"]["glyphs"].as_object() {
            for (label, column) in glyphs {
                let positions: Vec<_> = outputs[0]
                    .lines()
                    .filter_map(|line| {
                        line.find(label)
                            .map(|start| mant_ir::geometry::text_width(&line[..start]))
                    })
                    .collect();
                assert_eq!(
                    positions,
                    [usize::try_from(column.as_u64().unwrap()).unwrap()],
                    "{}: {label}",
                    case["id"]
                );
            }
        }
    }
    assert_eq!(checked, 236);
}
