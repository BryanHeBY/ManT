//! Observe actual SGR runs, not only text after stripping decoration.
use serde_json::Value;

pub(super) fn assert_recovery_styles(ansi: &str, case: &Value) {
    let mut chars = ansi.chars();
    let mut cells = Vec::new();
    let mut bold = false;
    let mut italic = false;
    while let Some(value) = chars.next() {
        if value != '\u{1b}' {
            cells.push((value, bold, italic));
            continue;
        }
        assert_eq!(chars.next(), Some('['));
        let sequence = chars.by_ref().take_while(|c| *c != 'm').collect::<String>();
        let mut codes = sequence.split(';').map(|part| part.parse::<u16>().unwrap());
        while let Some(code) = codes.next() {
            match code {
                0 => {
                    bold = false;
                    italic = false;
                }
                1 => bold = true,
                3 => italic = true,
                22 => bold = false,
                23 => italic = false,
                38 | 48 | 58 => match codes.next() {
                    Some(5) => {
                        codes.next().unwrap();
                    }
                    Some(2) => {
                        for _ in 0..3 {
                            codes.next().unwrap();
                        }
                    }
                    other => panic!("invalid extended color {other:?}"),
                },
                _ => {}
            }
        }
    }
    let text = cells.iter().map(|(value, _, _)| value).collect::<String>();
    for recovery in case["recoveries"].as_array().unwrap() {
        let word = recovery["text"].as_str().unwrap();
        let ordinal = usize::try_from(recovery["ordinal"].as_u64().unwrap()).unwrap();
        let at = text.match_indices(word).nth(ordinal).unwrap().0;
        let start = text[..at].chars().count();
        for (_, bold, italic) in &cells[start..start + word.chars().count()] {
            assert_eq!(
                (*bold, *italic),
                (
                    recovery["style"]["strong"].as_bool().unwrap(),
                    recovery["style"]["emphasis"].as_bool().unwrap()
                ),
                "{} actual SGR source member",
                case["id"]
            );
        }
    }
}

#[test]
fn annotated_traditional_manual_links_keep_actual_cli_modifiers() {
    use super::super::process_support::run_text_input;

    // Pinned CVS man_term.c::pre_alternate leaves the second operand roman.
    // Only the first operand receives the original alternating modifier.
    for (macro_name, strong, emphasis) in [("BR", true, false), ("IR", false, true)] {
        let source = format!(
            ".TH NAVPROBE 1 \"November 17, 2026\"\n.SH DESCRIPTION\n.{macro_name} printf \"(3) ordinary\"\nBodyWord.\n"
        );
        let output = run_text_input(
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
                "always",
            ],
            &source,
        );
        assert!(output.status.success(), "{output:?}");
        let ansi = String::from_utf8(output.stdout).unwrap();
        assert!(ansi.contains("\u{1b}["));
        assert!(super::super::execution_scopes::without_sgr(&ansi).contains("printf(3) ordinary"));
        assert_recovery_styles(
            &ansi,
            &serde_json::json!({
                "id": macro_name,
                "recoveries": [
                    {"text": "printf", "ordinal": 0, "style": {"strong": strong, "emphasis": emphasis}},
                    {"text": "(3) ordinary", "ordinal": 0, "style": {"strong": false, "emphasis": false}},
                ],
            }),
        );
    }
}
