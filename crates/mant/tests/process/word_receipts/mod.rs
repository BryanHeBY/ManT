//! Source word ownership survives the public text and Markdown entry points.

use mant_ir::visit::{self, Visit};
use mant_ir::{Inline, LinkTarget, inline_plain_text};

use super::execution_scopes::without_sgr;
use super::process_support::run_text_input;

mod marker_replay;
mod motion;
mod overprint;
mod publication;
mod rows;

const FIELD_PROBE: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample manual\n.Sh DESCRIPTION\n";

const EDITABLE_FIELD: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";

#[derive(Default)]
struct Links(Vec<(LinkTarget, String)>);

impl<'a> Visit<'a> for Links {
    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Link {
            target, children, ..
        } = inline
        {
            self.0.push((target.clone(), inline_plain_text(children)));
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

fn document_links(source: &str, input: &str) -> Vec<(LinkTarget, String)> {
    let json = render(source, input, "json", "never", false);
    let bundle: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let mut links = Links::default();
    for section in &bundle.document.unwrap().sections {
        links.visit_section(section);
    }
    links.0
}

fn assert_link(source: &str, input: &str, label: &str) {
    assert_eq!(
        document_links(source, input),
        [(
            LinkTarget::External {
                uri: "https://x.io".into(),
            },
            label.into(),
        )],
    );
}

fn assert_readbacks(source: &str, expected: &str, label: Option<&str>) {
    assert_readback_modes(source, expected, label, &[false, true]);
}

fn assert_readback_modes(source: &str, expected: &str, label: Option<&str>, modes: &[bool]) {
    let plain = render(source, "roff", "text", "never", false);
    let ansi = render(source, "roff", "text", "always", false);
    assert_eq!(description(&plain), expected, "{source}");
    assert_eq!(without_sgr(&ansi), plain, "{source}");
    if let Some(label) = label {
        assert_link(source, "roff", label);
    }
    for &addressable in modes {
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

fn search_visible_word(source: &str, word: &str) -> u64 {
    search_visible_word_with_format(source, "roff", word)
}

fn search_visible_word_with_format(source: &str, format: &str, word: &str) -> u64 {
    let result = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            format,
            "--format",
            "json",
            "--display",
            "direct",
            "--word",
            "--search",
            word,
        ],
        source,
    );
    assert!(result.status.success(), "{result:?}");
    let response: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    response["total"].as_u64().unwrap()
}
