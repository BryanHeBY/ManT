//! Development-only generator shared by the example and the drift test.

use std::fmt::Write as _;

use mant_engine::{TldrPageLocation, parse_tldr_page};
use mant_ir::TldrCommandPart;

pub fn generate(manual: &str) -> String {
    // Explicit build metadata, not inference from command substrings. Exact
    // command/count checks make manual edits require an intentional review of
    // capability requirements; the Markdown language gains no help-only syntax.
    generate_with_requirements(
        manual,
        &[
            ("mant git", &["roff", "tui"]),
            ("mant tar --explain=--exclude --format markdown", &["roff"]),
            (
                "mant git --search worktree --follow-links --max-depth 2 --max-documents 32 --context 1",
                &["roff"],
            ),
            (
                "mant --find '^git' --regex --kind manual --limit 20 --format json --compact",
                &[],
            ),
            (
                "mant --input ./tool.md --outline --outline-entries all --format json --compact",
                &[],
            ),
            ("mant --mcp", &["mcp"]),
        ],
    )
}

pub fn generate_with_requirements(manual: &str, requirements: &[(&str, &[&str])]) -> String {
    let preface = manual
        .strip_prefix("<!-- mant:tldr:start -->")
        .and_then(|text| text.split_once("<!-- mant:tldr:end -->"))
        .expect("self manual must begin with an embedded TLDR")
        .0;
    let page = parse_tldr_page(
        preface,
        TldrPageLocation {
            platform: "common".into(),
            language: "en".into(),
            source_path: "docs/manuals/mant.md".into(),
        },
    )
    .expect("valid self-manual TLDR");
    assert!(!page.examples.is_empty(), "self manual needs examples");
    assert_eq!(
        page.examples.len(),
        requirements.len(),
        "review the help capability metadata when examples change"
    );
    let mut output = String::from(
        "// Generated from docs/manuals/mant.md; do not edit.\n\
         // Regenerate: cargo run --locked -p mant --example generate_help_tldr\n\
         #[rustfmt::skip]\n\
         pub(super) const EXAMPLES: &[HelpExample] = &[\n",
    );
    for (example, (expected_command, features)) in page.examples.into_iter().zip(requirements) {
        let command: String = example
            .command_parts
            .iter()
            .map(|part| match part {
                TldrCommandPart::Text { value } | TldrCommandPart::Placeholder { value } => {
                    value.as_str()
                }
            })
            .collect();
        assert_eq!(
            command, *expected_command,
            "review help capability metadata when a command changes"
        );
        assert!(
            features
                .iter()
                .all(|feature| matches!(*feature, "roff" | "tui" | "pager" | "mcp" | "update"))
        );
        writeln!(output, "    ({:?}, &{features:?}, &[", example.description).unwrap();
        for part in example.command_parts {
            let (text, placeholder) = match part {
                TldrCommandPart::Text { value } => (value, false),
                TldrCommandPart::Placeholder { value } => (value, true),
            };
            writeln!(output, "        ({text:?}, {placeholder}),").unwrap();
        }
        output.push_str("    ]),\n");
    }
    output.push_str("];\n");
    output
}
