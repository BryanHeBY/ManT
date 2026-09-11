//! Optional native escape state stays deterministic on recursive and error paths.

use libmandoc_rs::Parser;

const CASES: &[(&str, Option<&str>, &str)] = &[
    (
        r"left\-middle \[em] \C'em' right",
        None,
        "left-middle — — right",
    ),
    (
        concat!(
            ".nr width 1\n.ds quote '\n.ds font B\n",
            r"left\h\*[quote]\n[width]n'right",
            "\n",
            r"\f\*[font]bold\fP",
        ),
        None,
        "left right bold",
    ),
    (
        r"before\h\~after",
        Some(r"invalid escape argument delimiter: \h\~"),
        "beforeafter",
    ),
    (
        r"before\C",
        Some(r"incomplete escape sequence: \C"),
        "before",
    ),
];

fn source(body: &str) -> String {
    format!(".TH ESCAPES 7 \"September 11, 2026\" \"ManT\"\n.SH BODY\n.nf\n{body}\n.fi\n")
}

#[test]
fn recursive_escape_state_preserves_parser_diagnostics() {
    for &(body, expected_error, _) in CASES {
        let report = Parser::default()
            .parse_bytes("escapes.7", source(body).as_bytes())
            .expect("invalid escapes remain recoverable native diagnostics");
        let errors = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.level == libmandoc_rs::DiagnosticLevel::Error)
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            errors,
            expected_error.into_iter().collect::<Vec<_>>(),
            "{body}"
        );
    }
}

#[cfg(feature = "render")]
#[test]
fn recursive_escape_state_preserves_reference_output() {
    // Independently checked against the unpatched, pinned CVS formatter.
    for &(body, _, expected_text) in CASES {
        let report = libmandoc_rs::Renderer::new(libmandoc_rs::RenderFormat::Utf8)
            .render_bytes("escapes.7", source(body).as_bytes())
            .expect("render valid or diagnosed escape sequences");
        let mut plain = String::new();
        for ch in report.output.chars() {
            if ch == '\u{8}' {
                plain.pop();
            } else {
                plain.push(ch);
            }
        }
        let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(plain.contains(expected_text), "{body}: {plain}");
        assert!(!plain.chars().any(char::is_control));
    }
}
