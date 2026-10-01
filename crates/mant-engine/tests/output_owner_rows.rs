//! Native output owners retain authored destinations and exact link positions.

use mant_ir::{DocumentIndex, ReferenceScope, ResolvedContent};
use mant_protocol::{
    EntryProjection, ReferenceCount, ReferenceProjection, ReferenceProjectionMode,
    ReferenceTargetType,
};

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

fn assert_public_owners(case: &serde_json::Value, query: &ResolvedContent) {
    let name = case["name"].as_str().unwrap();
    let document = query.document.as_ref().unwrap();
    assert!(
        mant_ir::validate_document(document).is_empty(),
        "{name}: invalid IR address"
    );
    if case["target"].as_bool().unwrap() {
        assert!(
            DocumentIndex::build(document)
                .fragment_target("mant-field-word-2-0")
                .is_some(),
            "{name}: authored marker-shaped destination was deleted"
        );
    }
    let projection = ReferenceProjection {
        mode: ReferenceProjectionMode::All,
        target_types: vec![ReferenceTargetType::External, ReferenceTargetType::Email],
        ..Default::default()
    };
    let references =
        mant_query::project_references(document, None, ReferenceScope::Document, &projection);
    assert_eq!(
        references.occurrences,
        ReferenceCount::Exact {
            value: case["links"].as_u64().unwrap()
        },
        "{name}"
    );
    for record in &references.records {
        let link = record
            .origin
            .resolve_link(document)
            .expect("exact link origin");
        let mant_ir::Inline::Link { children, .. } = link else {
            unreachable!("resolve_link returns a link")
        };
        assert_eq!(record.label, mant_ir::inline_plain_text(children), "{name}");
        if let Some(expected) = case["first_label"].as_str() {
            assert_eq!(
                record.label, expected,
                "{name}: label moved to a later owner"
            );
        }
    }
    let outline =
        mant_query::build_outline_with_references(query, EntryProjection::All, None, &projection)
            .unwrap();
    let json = serde_json::to_string(&outline).unwrap();
    assert!(
        !json.contains("\\u0000mant:"),
        "{name}: private owner leaked into outline"
    );
    let _: mant_protocol::QueryOutline = serde_json::from_str(&json).unwrap();
    let markdown = mant_codec::encode::render_markdown(query);
    assert!(
        !markdown.contains('\0'),
        "{name}: private owner leaked into Markdown"
    );
}

#[test]
fn every_output_sink_preserves_accepted_rows_and_public_semantic_owners() {
    // All 69 sources ran registered pristine ASCII/UTF-8/HTML/tree/lint
    // before recording these assertions. print_mdoc_node dispatches real
    // HEAD/BODY/No/D1/Dl/tbl/heading owners; man_UR_pre's first BODY label
    // keeps its identity when print_man_node later visits PP/IP/nf/fi.
    // tbl uses a bare Lk whose recovered text agrees with native operands,
    // and a real outer Tg adjacent to tbl, rather than invented cell AST.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("output_owner_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 9 * 5 + 2 * 3 * 4);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        // tbl deliberately ignores high-level macros. The already admitted
        // recovery enhancement executes its isolated fragment instead; its
        // exact independent pristine gold is recorded beside the raw tbl
        // output, never substituted for that original reference evidence.
        let row_gold = if case["scope"] == "table-macro-recovery" {
            &case["fragment_rows"]
        } else {
            &case["rows"]
        };
        let expected = row_gold
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{name}: private owner leaked"
        );
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        assert_public_owners(case, &query);
        let actual = body_rows(&mant_render::render_query_man(&query));
        if actual != expected {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
