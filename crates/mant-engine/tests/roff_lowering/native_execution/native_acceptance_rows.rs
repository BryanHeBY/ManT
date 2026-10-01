//! Native word acceptance retains the exact initial, interior and final rows.

use mant_ir::ResolvedContent;

fn body_rows(rendered: &str) -> Vec<String> {
    let mut projected = String::with_capacity(rendered.len());
    for character in rendered.chars() {
        match character {
            '\u{8}' => {
                projected.pop();
            }
            '\u{a0}' => projected.push(' '),
            _ => projected.push(character),
        }
    }
    let rows = projected.lines().map(str::trim).collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let end = rows.iter().position(|row| *row == "NEXT").unwrap();
    rows[start..end]
        .iter()
        .map(|row| (*row).to_owned())
        .collect()
}

#[test]
fn all_original_acceptance_sources_keep_their_complete_body_row_window() {
    // These are the unchanged A110 plus man1/man2 inputs from the original
    // shared execution matrix. All 112 exact files ran pristine ASCII,
    // UTF-8, HTML, tree and lint before these gold rows were recorded.
    // term.c::term_fill distinguishes accepted prefixes, NBRZW graph cells,
    // BACKBEFORE and rejected suffixes. DESCRIPTION/NEXT delimit only page
    // furniture; source-produced initial/final empty rows remain assertions.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/native_acceptance_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 110 + 2);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let expected = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let query = mant_loader::load_roff_bytes(source.as_bytes())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let json = mant_render::render_query_json(&query, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{name}: private owner leaked"
        );
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = decoded.into();
        let actual = body_rows(&mant_render::render_query_man(&query));
        if actual != expected {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
