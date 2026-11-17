//! Process versions in the existing structured response families.

use super::process_support::run_text_input;
use mant_ir::ParserInfo;
use serde_json::{Value, json};

fn assert_process_provenance(source: &str, format: &str, entry: &str, parser: &ParserInfo) {
    for (view, schema, producer) in [
        (&[][..], "mant.query/v0.14", Some("/document/producer")),
        (&["--node", "1"], "mant.excerpt/v0.14", Some("/producer")),
        (
            &["--explain", entry],
            "mant.explanation/v0.14",
            Some("/producer"),
        ),
        (&["--outline"], "mant.outline/v0.14", None),
        (&["--search", entry], "mant.search/v0.14", None),
    ] {
        let mut arguments = vec!["--input", "-", "--input-format", format, "--format", "json"];
        arguments.extend_from_slice(view);
        let output = run_text_input(&arguments, source);
        assert!(output.status.success(), "{format} {schema}: {output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).expect("response JSON");
        assert_eq!(value["schema"], schema);
        if let Some(pointer) = producer {
            assert_eq!(
                value.pointer(pointer),
                Some(&json!({
                    "name": "mant",
                    "version": env!("CARGO_PKG_VERSION"),
                    "engine": {"name": parser.name, "version": parser.version},
                })),
                "{format} {schema}"
            );
        } else {
            assert!(value.get("producer").is_none(), "{format} {schema}");
        }
    }
}

#[test]
fn markdown_views_publish_the_process_version_and_keep_parser_identity() {
    let source = "# Version probe\n\n## Usage\n\nBODY\n";
    let parsed = mant_loader::load_markdown_text(source, None)
        .unwrap()
        .document
        .unwrap();
    let parser = parsed.parser.unwrap();
    assert_eq!(parser.name, "pulldown-cmark");
    assert_process_provenance(source, "markdown", "BODY", &parser);
}

#[cfg(feature = "roff")]
#[test]
fn roff_views_publish_the_process_version_and_keep_parser_identity() {
    // Reuse the native declaration fixture already checked by the pinned
    // parser. This test exercises the response metadata around that parser.
    let source = include_str!("../../../mant-engine/tests/fixtures/entry-presentation.1");
    let parsed = mant_loader::load_roff_bytes(source.as_bytes())
        .unwrap()
        .document
        .unwrap();
    let parser = parsed.parser.unwrap();
    assert_eq!(parser.name, "libmandoc");
    assert_process_provenance(source, "roff", "language", &parser);
}
