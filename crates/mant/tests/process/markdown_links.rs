//! Reference definitions stay visible to authored leading-hard-row paragraphs.
use super::process_support::run_text_input;
use mant_ir::{Block, Inline, LinkTarget};

#[test]
fn leading_rows_keep_reference_labels_in_direct_display_and_json() {
    for (reference, label) in [
        ("[hello][ref]", "hello"),
        ("[ref][]", "ref"),
        ("[ref]", "ref"),
    ] {
        for before in [false, true] {
            let definition = "[ref]: https://example.org\n\n";
            let (prefix, suffix) = if before {
                (definition, "")
            } else {
                ("", definition)
            };
            let source =
                format!("# TEST\n\n## DESCRIPTION\n\n{prefix}<br />\n{reference}\n\n{suffix}");
            let output = run_text_input(
                &[
                    "--input",
                    "-",
                    "--input-format",
                    "markdown",
                    "--display",
                    "direct",
                    "--color",
                    "never",
                ],
                &source,
            );
            assert!(output.status.success(), "{:?}", output.stderr);
            assert_eq!(output.stderr.len(), 0);
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains(&format!("\n\n{label}")), "{source}\n{text}");
            assert!(!text.contains(reference), "{source}\n{text}");
            let output = run_text_input(
                &[
                    "--input",
                    "-",
                    "--input-format",
                    "markdown",
                    "--format",
                    "json",
                    "--compact",
                ],
                &source,
            );
            assert!(output.status.success(), "{:?}", output.stderr);
            let query: mant_protocol::QueryBundle = serde_json::from_slice(&output.stdout).unwrap();
            let document = query.document.unwrap();
            let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
                panic!("reference paragraph: {document:?}")
            };
            let [
                Inline::LineBreak { .. },
                Inline::Link {
                    children, target, ..
                },
            ] = children.as_slice()
            else {
                panic!("one leading hard row and one reference link: {children:?}")
            };
            assert_eq!(mant_ir::inline_plain_text(children), label);
            assert_eq!(
                *target,
                LinkTarget::External {
                    uri: "https://example.org".into()
                }
            );
        }
    }
}
