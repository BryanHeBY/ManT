//! Kept words consume their actual incoming separator before escape controls.

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
    let rows = projected.lines().collect::<Vec<_>>();
    let start = rows
        .iter()
        .position(|row| row.trim() == "DESCRIPTION")
        .unwrap()
        + 1;
    let end = rows.iter().position(|row| row.trim() == "NEXT").unwrap();
    let rows = &rows[start..end];
    let margin = rows
        .iter()
        .filter(|row| !row.trim().is_empty())
        .map(|row| row.len() - row.trim_start_matches(' ').len())
        .min()
        .unwrap_or_default();
    rows.iter()
        .map(|row| row.get(margin..).unwrap_or_default().trim_end().to_owned())
        .collect()
}

#[test]
fn keep_words_cross_all_word_kinds_lines_and_controlled_separators() {
    // All 96 exact sources passed pristine lint and were rendered in ASCII,
    // UTF-8, HTML and tree before these assertions. mdoc_term.c NODE_LINE
    // restores PREKEEP before handlers; term.c::term_word writes its incoming
    // separator, promotes PREKEEP, then executes p/c/z. Bk BODY exit clears
    // KEEP/PREKEEP. The common page margin alone is removed: A\p -> empty
    // word -> B can retain a real extra leading blank on the next row.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("kept_word_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 2 * 4 * 2 * 6);
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
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let actual = body_rows(&mant_render::render_query_man(&query));
        if actual != expected {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
