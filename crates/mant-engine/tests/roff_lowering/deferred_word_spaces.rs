//! Deferred native separators remain with the word that wrote their cells.

use flate2::read::GzDecoder;
use serde::Deserialize;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

#[derive(Deserialize)]
struct Matrix {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    group: String,
    id: String,
    source: String,
    rows: Vec<String>,
}

fn description_rows<'a>(text: &'a str, group: &str) -> Vec<&'a str> {
    let (_, body) = text.split_once("DESCRIPTION\n").expect("DESCRIPTION");
    if group == "N1b/word-boundaries" {
        let (body, _) = body.split_once("\nENDTEST\n").expect("ENDTEST");
        // One newline is section furniture. A second trailing newline
        // belongs to a native completed row, including a bare `\p`.
        return body
            .strip_suffix('\n')
            .unwrap_or(body)
            .split('\n')
            .collect();
    }
    let body = body
        .split_once("\nEND\n")
        .map_or(body, |(section, _)| section);
    // Page separation and the renderer's optional final newline are not part
    // of these owner receipts. Interior whitespace and row order are exact.
    body.trim_end_matches('\n').split('\n').collect()
}

#[test]
fn deferred_word_spaces_keep_native_glyphs_and_exact_separators_after_json() {
    let bytes = include_bytes!("deferred_word_spaces/cases.json.gz");
    let matrix: Matrix = serde_json::from_reader(GzDecoder::new(bytes.as_slice())).unwrap();
    verify_cases(
        matrix.cases,
        |case| &case.id,
        |case| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let transported = json_string_roundtrip(&original);
            let rendered = mant_render::render_query_man(&transported);
            assert_eq!(
                description_rows(&rendered, &case.group),
                case.rows,
                "{rendered:?}"
            );
        },
    );
}
