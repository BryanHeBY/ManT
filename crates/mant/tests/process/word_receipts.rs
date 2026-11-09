//! Source word ownership survives the public text and Markdown entry points.

use super::execution_scopes::without_sgr;
use super::process_support::run_text_input;

const WORD: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/accepted-word.1");
const HARD_ROW: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/late-hard-row.1");
const UNICODE_LINK: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/accepted-unicode-link.1");

fn render(source: &str, input: &str, format: &str, color: &str, addressable: bool) -> String {
    let mut args = vec![
        "--input",
        "-",
        "--input-format",
        input,
        "--format",
        format,
        "--display",
        "direct",
        "--color",
        color,
    ];
    if addressable {
        args.push("--preserve-anchors");
    }
    let result = run_text_input(&args, source);
    assert!(result.status.success(), "{result:?}");
    String::from_utf8(result.stdout).unwrap()
}

fn description(text: &str) -> &str {
    let (_, body) = text.split_once("\nDESCRIPTION\n").unwrap();
    body.strip_suffix('\n').unwrap_or(body)
}

#[test]
fn accepted_words_and_delayed_hard_rows_survive_two_cli_markdown_readbacks() {
    // All three exact .1 sources ran the five pristine profiles. The
    // formatter's accepted ranges and hard rows come from term.c::encode1(),
    // term_fill() and term_flushln(). The addressable subset here is the
    // DESCRIPTION body. Earlier raw HTML anchors have their own read contract.
    for (source, expected) in [
        (WORD, "ALPHAX  BETA\nOMEGA"),
        (HARD_ROW, "ALPHA\nXBETA\nOMEGA"),
        (UNICODE_LINK, "ALPHAX  界e\u{301}字: https://x.io\nOMEGA"),
    ] {
        let plain = render(source, "roff", "text", "never", false);
        let ansi = render(source, "roff", "text", "always", false);
        assert_eq!(description(&plain), expected, "{source}");
        assert_eq!(without_sgr(&ansi), plain, "{source}");
        for addressable in [false, true] {
            let mut input = source.to_owned();
            let mut input_format = "roff";
            for round in 1..=2 {
                input = render(&input, input_format, "markdown", "never", addressable);
                input_format = "markdown";
                let text = render(&input, input_format, "text", "never", false);
                assert_eq!(
                    description(&text),
                    expected,
                    "addressable={addressable}, round {round}: {input:?}"
                );
            }
        }
    }
}
