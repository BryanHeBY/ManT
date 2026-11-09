//! Explicit mathematical output through the native process entry point.
use super::process_support::run_text_input;

// Exact inputs were run with the pinned reference in tree, HTML and UTF-8
// modes before these assertions. CVS eqn.c EQN_TOK_OVER/SUP/LEFT/RIGHT and
// eqn_html.c retain the fraction/script operands and literal ASCII fences.
const DISPLAY: &str = ".TH MATH 1\n.SH DESCRIPTION\n.EQ\nx over y\n.EN\n";
const INLINE: &str = ".TH MATH 1\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\nBefore $x sup 2$ after.\n";
const FENCE: &str = ".TH MATH 1\n.SH DESCRIPTION\n.EQ\nleft \"<\" { x over y } right \">\"\n.EN\n";

#[test]
fn formulas_default_to_latex_and_readable_is_an_explicit_choice() {
    let portable = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            "markdown",
        ],
        DISPLAY,
    );
    assert!(portable.status.success(), "{portable:?}");
    let default = String::from_utf8(portable.stdout).unwrap();
    assert!(default.contains("\\frac{"), "{default}");

    let explicit = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--markdown-math",
            "readable",
        ],
        DISPLAY,
    );
    assert!(explicit.status.success(), "{explicit:?}");
    let readable = String::from_utf8(explicit.stdout).unwrap();
    assert!(readable.contains("x / y"), "{readable}");
    assert!(!readable.contains("\\frac"));

    let mathematical = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--markdown-math",
            "latex",
        ],
        DISPLAY,
    );
    assert!(mathematical.status.success(), "{mathematical:?}");
    let text = String::from_utf8(mathematical.stdout).unwrap();
    assert_eq!(text, default);
    assert_eq!(text.matches("$$").count(), 2, "{text}");
    assert!(text.contains("\\frac{"), "{text}");
    assert!(mathematical.stderr.is_empty(), "{:?}", mathematical.stderr);
}

#[test]
fn inline_mathematics_preserves_its_surrounding_words() {
    let output = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--markdown-math",
            "latex",
        ],
        INLINE,
    );
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Before $") && text.contains("$  after."),
        "{text}"
    );
    assert!(text.contains("^{"), "{text}");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

#[test]
fn selected_sections_honor_the_same_math_mode_as_complete_documents() {
    for mode in ["latex", "readable"] {
        let output = run_text_input(
            &[
                "--input",
                "-",
                "--input-format",
                "roff",
                "--node",
                "1",
                "--markdown-math",
                mode,
            ],
            DISPLAY,
        );
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.contains("\\frac{"), mode == "latex", "{text}");
        assert_eq!(text.contains("x / y"), mode == "readable", "{text}");
    }
}

#[test]
fn target_losses_reach_stderr_without_changing_literal_fences() {
    let output = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--markdown-math",
            "latex",
        ],
        FENCE,
    );
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("\\frac{"), "{text}");
    assert!(
        text.contains("\\textless{}") && text.contains("\\textgreater{}"),
        "{text}"
    );
    assert!(!text.contains("\\langle") && !text.contains("\\rangle"));
    let warnings = String::from_utf8(output.stderr).unwrap();
    assert!(
        warnings.contains("conversion issues") && warnings.contains("UnsupportedFence"),
        "{warnings}"
    );
}
