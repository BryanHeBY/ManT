//! A HEAD output owner executes the same vertical requests as ordinary text.

use mant_ir::ResolvedContent;

#[derive(serde::Deserialize)]
struct Case {
    source: String,
    prefix_rows: Vec<String>,
}

fn round_trip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    assert!(!json.contains("\\u0000mant:"));
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

#[test]
fn head_vertical_requests_keep_real_empty_rows_and_short_field_controls() {
    // Every complete fixture ran pristine ASCII/UTF-8/HTML/tree/lint before
    // recording these assertions; all lint=0. mdoc_term.c::termp_pp_pre()
    // calls term_vspace() before writing its target. term.c::term_vspace()
    // first runs term_newln(), then consumes skipvsp or emits an endline.
    // Compare hard rows before AFTER: remove the common left margin and
    // trailing field padding; preserve every internal blank row and space.
    // HEAD/BODY responsive placement is not asserted as terminal geometry.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/native_head_vertical_requests.json")).unwrap();
    assert_eq!(cases.len(), 60);
    for case in cases {
        let query = round_trip(&case.source);
        let text = mant_render::render_query_man(&query);
        let body = text.split_once("DESCRIPTION\n").unwrap().1;
        let before_after = body
            .lines()
            .take_while(|row| !row.contains("AFTER"))
            .map(|row| row.trim_matches(' ').to_owned())
            .collect::<Vec<_>>();
        assert_eq!(before_after, case.prefix_rows, "{}\n{text}", case.source);
        let markdown = mant_codec::encode::render_markdown(&query);
        assert_eq!(markdown.matches("AFTER").count(), 1);
        assert_eq!(markdown.matches("BodyWord").count(), 1);
    }
}

#[test]
fn paragraph_target_belongs_after_the_executed_gap_and_term_separator() {
    // Both exact sources ran pristine CVS before this assertion.
    // mdoc_validate.c::post_tg() moves gap's ID onto Pp, and
    // mdoc_term.c::termp_pp_pre() writes that tag after term_vspace().
    for (label, expected_first) in [
        ("LABEL", "LABEL  "),
        ("VERYVERYLONGLABEL", "VERYVERYLONGLABEL\n"),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 8n\n.It Xo\n.No {label}\n.Tg gap\n.Pp\n.No AFTER\n.Xc\n.No BodyWord\n.El\n.Sh NEXT\n.No END\n"
        );
        let query = round_trip(&source);
        let block = &query.document.as_ref().unwrap().sections[1].blocks[0];
        let mant_ir::Block::DefinitionList { items, .. } = block else {
            panic!("definition list expected: {block:#?}")
        };
        let [first, second] = items[0].terms.as_slice() else {
            panic!("two semantic alternatives: {items:#?}")
        };
        assert_eq!(mant_ir::inline_plain_text(first), expected_first);
        assert_eq!(mant_ir::inline_plain_text(second), "AFTER");
        assert!(matches!(
            second.first(),
            Some(mant_ir::Inline::Anchor { id, .. }) if id.as_str() == "gap"
        ));
        let index = mant_ir::DocumentIndex::build(query.document.as_ref().unwrap());
        assert!(index.fragment_target("gap").is_some());
    }
}
