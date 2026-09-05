//! Development-only generator shared by the example and the drift test.

use std::fmt::Write as _;

use mant_engine::{TldrPageLocation, parse_tldr_page};
use mant_ir::TldrCommandPart;

pub fn generate(manual: &str) -> String {
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
    let mut output = String::from(
        "// Generated from docs/manuals/mant.md; do not edit.\n\
         // Regenerate: cargo run --locked -p mant --example generate_help_tldr\n\
         #[rustfmt::skip]\n\
         pub(super) const EXAMPLES: &[HelpExample] = &[\n",
    );
    for example in page.examples {
        writeln!(output, "    ({:?}, &[", example.description).unwrap();
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
