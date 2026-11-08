//! Explicit mathematical output through the native process entry point.
use super::process_support::run_text_input;

// Exact inputs were run with the pinned reference in tree, HTML and UTF-8
// modes before these assertions. CVS eqn.c EQN_TOK_OVER/SUP/LEFT/RIGHT and
// eqn_html.c retain the fraction/script operands and literal ASCII fences.
const DISPLAY: &str = ".TH MATH 1\n.SH DESCRIPTION\n.EQ\nx over y\n.EN\n";
const INLINE: &str = ".TH MATH 1\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\nBefore $x sup 2$ after.\n";
const FENCE: &str = ".TH MATH 1\n.SH DESCRIPTION\n.EQ\nleft \"<\" { x over y } right \">\"\n.EN\n";

#[test]
fn formulas_remain_readable_by_default_and_latex_requires_an_explicit_choice() {
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
    let readable = String::from_utf8(portable.stdout).unwrap();
    // The shared readable projection adds spaces around the fraction operator.
    assert!(readable.contains("x / y"), "{readable}");
    assert!(!readable.contains("```math") && !readable.contains("\\frac"));

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
    assert_eq!(String::from_utf8(explicit.stdout).unwrap(), readable);

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
