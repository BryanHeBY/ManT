//! Source word ownership survives the public text and Markdown entry points.

use mant_ir::visit::{self, Visit};
use mant_ir::{Inline, LinkTarget, inline_plain_text};

use super::execution_scopes::without_sgr;
use super::process_support::run_text_input;

const WORD: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/accepted-word.1");
const HARD_ROW: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/late-hard-row.1");
const UNICODE_LINK: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_receipts/accepted-unicode-link.1");
const BACKLINE: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_projection/backline-after-zero-width.1"
);
const UNPRINTED: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_projection/source-prefix-cross-pass.1"
);
const REARM: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/rearm-prefix-space.1");
const LINK_REARM: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/link-rearm.1");
const DEFERRED_MARKER: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/deferred-marker.1");
const REARMED_MARKER: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/rearmed-marker.1");
const LINKED_MARKER: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/linked-marker.1");
const SURVIVING_SEPARATOR: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/surviving-separator.1");
const PASS_LEADING_SPACE: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_passes/leading-space-after-pass-3.1"
);
const EMPTY_AT_EOF: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-at-eof.1");
const EMPTY_BEFORE_PP: &str =
    include_str!("../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-before-pp.1");
const EMPTY_LINK_BEFORE_PP: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-link-before-pp.1"
);
const EMPTY_BEFORE_SPACE: &str = include_str!(
    "../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-before-space.1"
);
const LINK_SPACES: [&str; 3] = [
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-1.1"),
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-2.1"),
    include_str!("../../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-3.1"),
];

#[derive(Default)]
struct Links(Vec<(String, String)>);

impl<'a> Visit<'a> for Links {
    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Link {
            target: LinkTarget::External { uri },
            children,
            ..
        } = inline
        {
            self.0.push((uri.clone(), inline_plain_text(children)));
        }
        visit::walk_inline(self, inline);
    }
}

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

fn assert_link(source: &str, input: &str, label: &str) {
    let json = render(source, input, "json", "never", false);
    let bundle: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let mut links = Links::default();
    for section in &bundle.document.unwrap().sections {
        links.visit_section(section);
    }
    assert_eq!(links.0, [("https://x.io".into(), label.into())]);
}

fn assert_readbacks(source: &str, expected: &str, label: Option<&str>) {
    let plain = render(source, "roff", "text", "never", false);
    let ansi = render(source, "roff", "text", "always", false);
    assert_eq!(description(&plain), expected, "{source}");
    assert_eq!(without_sgr(&ansi), plain, "{source}");
    if let Some(label) = label {
        assert_link(source, "roff", label);
    }
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
            if let Some(label) = label {
                assert_link(&input, input_format, label);
            }
        }
    }
}

#[test]
fn accepted_words_and_delayed_hard_rows_survive_two_cli_markdown_readbacks() {
    // All three exact .1 sources ran the five pristine profiles. The
    // formatter's accepted ranges and hard rows come from term.c::encode1(),
    // term_fill() and term_flushln(). The addressable subset here is the
    // DESCRIPTION body. Earlier raw HTML anchors have their own read contract.
    for (source, expected, label) in [
        (WORD, "ALPHAX  BETA\nOMEGA", None),
        (HARD_ROW, "ALPHA\nXBETA\nOMEGA", None),
        (
            UNICODE_LINK,
            "ALPHAX  界e\u{301}字: https://x.io\nOMEGA",
            Some(" 界e\u{301}字"),
        ),
    ] {
        assert_readbacks(source, expected, label);
    }
}

#[test]
fn overwritten_cells_and_printed_separators_survive_cli_markdown_readbacks() {
    // The five pristine profiles establish the exact rows. term_field()
    // advances only before a printable cell, including a backspace.
    for (source, expected) in [
        (BACKLINE, "ALPHAXBETA\nOMEGA"),
        (UNPRINTED, "ALPHAX\nBETA\nOMEGA"),
        (REARM, "ALPHAX  ETA\nOMEGA"),
    ] {
        assert_readbacks(source, expected, None);
    }
}

#[test]
fn marker_passes_keep_only_printed_separators_through_cli_readbacks() {
    // The pristine terminal profiles distinguish the authored post-break
    // space from the retired automatic space (term.c:205-220,389-427).
    // A real printed separator still survives the single-marker control.
    for (source, expected, label) in [
        (DEFERRED_MARKER, "ALPHAX\n BETA\nOMEGA", None),
        (REARMED_MARKER, "ALPHAX\nYBETA\nOMEGA", None),
        (
            LINKED_MARKER,
            "ALPHAX\nYBETA:\nhttps://x.io\nOMEGA",
            Some("\nYBETA"),
        ),
        (SURVIVING_SEPARATOR, "ALPHAX BETA\nOMEGA", None),
        (PASS_LEADING_SPACE, "ALPHAX\n\n YBETA", None),
    ] {
        assert_readbacks(source, expected, label);
    }
}

#[test]
fn rejected_plain_tail_and_positive_spacing_keep_independent_rows() {
    // In pinned term.c:143-146,205-220,250-253, the flush closes its
    // accepted pass and then its empty rejected tail. A following .Pp
    // independently runs term_vspace() via mdoc_term.c::termp_pp_pre().
    for (source, expected, label) in [
        (EMPTY_AT_EOF, "ALPHAX\n", None),
        (EMPTY_BEFORE_PP, "ALPHAX\n\n\nOMEGA", None),
        (EMPTY_LINK_BEFORE_PP, "ALPHAX\n\n\nOMEGA", Some("")),
    ] {
        assert_readbacks(source, expected, label);
    }

    // Markdown can simplify a positive .sp 2 to one layout row. The
    // preceding hard empty row must survive that simplification.
    assert_eq!(
        description(&render(EMPTY_BEFORE_SPACE, "roff", "text", "never", false)),
        "ALPHAX\n\n\n\nOMEGA"
    );
    let json = render(EMPTY_BEFORE_SPACE, "roff", "json", "never", false);
    let bundle: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let document = bundle.document.unwrap();
    let section = document.sections.last().unwrap();
    let [
        mant_ir::Block::Paragraph { children, .. },
        mant_ir::Block::VerticalSpace { lines: 2, .. },
        mant_ir::Block::Paragraph { .. },
    ] = section.blocks.as_slice()
    else {
        panic!("expected hard rows and independent .sp 2: {section:?}");
    };
    assert_eq!(inline_plain_text(children), "ALPHAX\n\n");
    for addressable in [false, true] {
        let mut input = render(EMPTY_BEFORE_SPACE, "roff", "markdown", "never", addressable);
        for _ in 0..2 {
            assert_eq!(
                description(&render(&input, "markdown", "text", "never", false)),
                "ALPHAX\n\n\nOMEGA"
            );
            input = render(&input, "markdown", "markdown", "never", addressable);
        }
    }
}

#[test]
fn printed_link_spaces_retain_their_owner_on_both_cli_markdown_readbacks() {
    // term_word() adds one separator before the label and encode1() may
    // retract a raw blank. For one raw blank the equivalent separator
    // remains in the label. Additional raw blanks retain their ownership.
    let word = "界e\u{301}字";
    for (index, source) in LINK_SPACES.into_iter().enumerate() {
        let expected = format!("ALPHAX{}{word}: https://x.io\nOMEGA", " ".repeat(index + 1));
        let label = format!("{}{word}", " ".repeat(index.max(1)));
        assert_readbacks(source, &expected, Some(&label));
    }
    assert_readbacks(LINK_REARM, "ALPHAX ETA: https://x.io\nOMEGA", Some("ETA"));
}
