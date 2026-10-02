//! Accepted word receipts are the only exported inline body.

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
fn accepted_word_receipts_keep_all_operand_and_owner_boundaries() {
    // Every exact source ran registered pristine ASCII/UTF-8/HTML/tree/lint
    // before these assertions (108 clean lint sources). mdoc_validate.c
    // post_bx inserts Ns/BSD and the version-release join; termp_xx_pre/post
    // controls KEEP. Authored p/c/z/font words execute before generated BSD.
    // Native hard-row gold also runs each unchanged source at width 1000;
    // default width-78 rows remain recorded but include device soft wraps.
    // I1 reran all 108 exact sources in the five pristine profiles before
    // removing the replacement-spelling axis. The accepted glyphs and rows
    // remain unchanged; public JSON has no second visible-body carrier.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/portable_word_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 9 * 4 * 3);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let expected = case["wide_rows"]
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
        assert!(
            !json.contains("portable-display"),
            "{name}: retired public shape"
        );
        assert!(
            !json.contains("mant:lk-presentation"),
            "{name}: private owner leaked"
        );
        assert!(!json.contains("\\u0000"), "{name}: private owner leaked");
        assert_eq!(
            mant_render::render_query_man(&loaded),
            mant_render::render_query_man(&query),
            "{name}: JSON changed accepted glyph ownership or rows"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
