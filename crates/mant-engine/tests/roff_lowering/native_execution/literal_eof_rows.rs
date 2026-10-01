//! A literal empty row owns its delimiter even without a following word.

use mant_ir::ResolvedContent;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    source: String,
    rows: Vec<String>,
}

#[test]
fn literal_rows_survive_eof_and_fill_transitions() {
    // All 72 exact sources ran the registered pristine ASCII/UTF-8/HTML/
    // tree/lint profiles before recording these assertions. man_term.c's
    // NODE_NOFILL entry and empty TEXT branch execute term_newln/term_vspace;
    // term.c:489 endline is an actual row, not an optional output terminator.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/literal_eof_rows/cases.json")).unwrap();
    assert_eq!(cases.len(), 72);
    let mut failures = Vec::new();
    for case in cases {
        let loaded = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(!json.contains("\\u0000mant:"), "{}", case.name);
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let rendered = mant_render::render_query_man(&query);
        let rows = rendered.lines().collect::<Vec<_>>();
        let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
        let actual = &rows[start..];
        if actual != case.rows {
            failures.push(format!(
                "{}\n{}\nactual {actual:?}\nreference {:?}",
                case.name, case.source, case.rows
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
