//! Device padding belongs to the origin of the word that consumes it.

use mant_ir::ResolvedContent;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    source: String,
    rows: Vec<String>,
}

fn description_rows(query: &ResolvedContent) -> Vec<String> {
    let rendered = mant_render::render_query_man(query);
    let rows = rendered.lines().collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let boundary = rows.iter().position(|row| *row == "NEXT").unwrap();
    let spacing = query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "NEXT")
        .unwrap()
        .spacing_before_lines;
    let end = boundary - usize::from(spacing);
    assert!(rows[end..boundary].iter().all(|row| row.is_empty()));
    rows[start..end]
        .iter()
        .map(|row| (*row).to_owned())
        .collect()
}

#[test]
fn field_padding_and_word_origins_stay_together_across_margin_flushes() {
    // Every complete source ran pristine ASCII/UTF-8/HTML/tree/lint before
    // these assertions. roff_term.c::roff_term_pre_mc() flushes the current
    // field; term.c::term_flushln() carries minbl into the next printed word.
    // The following Ed restores its origin, so that generated padding must
    // follow that word's origin through style/link wrappers and JSON.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/column_margin_rows/cases.json")).unwrap();
    assert_eq!(cases.len(), 20);
    let mut failures = Vec::new();
    for case in cases {
        let loaded = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{}: private receipt escaped",
            case.name
        );
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let actual = description_rows(&query);
        if actual != case.rows {
            failures.push(format!(
                "{}\n{}\nactual {actual:?}\nreference {:?}",
                case.name, case.source, case.rows
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
