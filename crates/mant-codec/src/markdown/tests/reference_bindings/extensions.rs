//! Source-bound catalogue of mandatory M4 controls and composition cases.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt::Write;

#[derive(Deserialize)]
struct Catalogue {
    case_count: usize,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    family: String,
    source: String,
    source_sha256: String,
    expected_text: Option<String>,
    expected_link_labels: Vec<String>,
    expected_refused: usize,
    expected_decoded_definitions: Vec<String>,
}

#[test]
fn mandatory_reference_extensions_keep_source_and_metadata_owners() {
    let matrix: Catalogue = serde_json::from_str(include_str!("extensions.json")).unwrap();
    assert_eq!(matrix.case_count, 71);
    assert_eq!(matrix.cases.len(), matrix.case_count);
    for case in matrix.cases {
        let mut hash = String::with_capacity(64);
        for byte in &Sha256::digest(case.source.as_bytes()) {
            write!(hash, "{byte:02x}").unwrap();
        }
        assert_eq!(hash, case.source_sha256, "{}", case.id);
        if case.family == "first-definition-erased" {
            witness_erased_definition_source(&case.source);
        }
        let document = parse(&case.source);
        let actual = reading(&document);
        if let Some(text) = case.expected_text {
            assert_eq!(actual.text, text, "{}", case.id);
        }
        assert_eq!(
            actual.links.iter().map(|link| &link.2).collect::<Vec<_>>(),
            case.expected_link_labels.iter().collect::<Vec<_>>(),
            "{}",
            case.id
        );
        assert_eq!(
            findings(&document, "markdown.recovered-link-target").len(),
            case.expected_refused,
            "{}",
            case.id
        );
        let decoded = decoded_findings(&document);
        assert_eq!(
            decoded.len(),
            case.expected_decoded_definitions.len(),
            "{}",
            case.id
        );
        for (finding, definition) in decoded.into_iter().zip(case.expected_decoded_definitions) {
            let start = case.source.find(&definition).unwrap();
            assert_span(
                &case.source,
                finding.source.unwrap(),
                start..start + definition.len(),
            );
        }
        for finding in &document.diagnostics {
            if let Some(span) = finding.source {
                assert_span(&case.source, span, span_range(span));
            }
        }
    }
}

fn witness_erased_definition_source(source: &str) {
    let options = super::super::super::markdown_options();
    let original = pulldown_cmark::Parser::new_ext(source, options);
    let definition = original.reference_definitions().get("r").unwrap();
    assert!(matches!(definition.dest.as_ref(), "\u{85}" | "\u{9b}"));
    // The C1 bytes become two source spaces. Locked firstpass then treats
    // this destination-less line as literal `[r]:`, not a definition. Its
    // shortcut reference is later refused too, but its safe `r:` text stays.
    // Do not invent the expectation that preserving a label deletes that text.
    let masked = source.replace(['\u{85}', '\u{9b}'], "  ");
    let parser = pulldown_cmark::Parser::new_ext(&masked, options);
    assert_eq!(
        parser
            .reference_definitions()
            .get("r")
            .unwrap()
            .dest
            .as_ref(),
        "https://other.test"
    );
    let visible = parser
        .filter_map(|event| match event {
            pulldown_cmark::Event::Text(value) => Some(value.into_string()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(visible, "xr:");
}
