//! Native survival and intentional recovery have independent source owners.
#[path = "../support/deferred_ownership.rs"]
mod cases;
use super::json_roundtrip::json_string_roundtrip;

#[test]
fn delayed_source_members_keep_complete_targets_styles_and_original_receivers() {
    assert_eq!(cases::consumers().len(), 42);
    let cases = cases::cases();
    assert_eq!(cases.len(), 219);
    let mut failures = Vec::new();
    for case in cases {
        let result = std::panic::catch_unwind(|| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let content = json_string_roundtrip(&original);
            assert_eq!(content, original);
            assert_eq!(
                mant_ir::validate_document(content.document.as_ref().unwrap()),
                []
            );
            cases::assert_rich(&content, &case, true);
            cases::assert_constraints(&content, &case);
            let wire = serde_json::to_string(content.document.as_ref().unwrap()).unwrap();
            assert!(
                !wire.contains("mant:deferred-origin:"),
                "{} private ownership escaped",
                case.id
            );
            if let Some(gold) = &case.diagnostics {
                assert_eq!(
                    serde_json::to_value(&content.document.as_ref().unwrap().diagnostics).unwrap(),
                    serde_json::to_value(gold).unwrap(),
                    "{} diagnostics",
                    case.id
                );
            }
            if let Some(gold) = &case.reading_rows {
                let text = mant_render::render_query_man(&content);
                let lines = text.split('\n').collect::<Vec<_>>();
                let start = lines
                    .iter()
                    .position(|line| *line == "DESCRIPTION")
                    .unwrap()
                    + 1;
                let end = lines.iter().rposition(|line| *line == "NEXT").unwrap();
                let mut rows = lines[start..end].to_vec();
                if rows.last() == Some(&"") {
                    rows.pop();
                }
                assert_eq!(rows, *gold, "{} complete enhanced reading rows", case.id);
            }
        });
        if result.is_err() {
            failures.push(case.id);
        }
    }
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn generated_list_markers_do_not_claim_the_previous_source_row() {
    // Each complete bullet/dash/enum source ran pristine five profiles first.
    // mdoc_term.c::termp_bl_pre and termp_it_pre/print_bvspace close the
    // predecessor buffer before the marker. Buffered recovery cannot reach
    // that marker; bare armed z is a different state with no old member.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("deferred_ownership/markers.json")).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 6);
    for case in cases.as_array().unwrap() {
        let content = json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        let actual = cases::snapshot(&content);
        let text = actual.text();
        let old = "X\\[bad-name]";
        let positions = cases::positions(&text, old);
        assert_eq!(positions.len(), 1, "{} full old member", case["id"]);
        let following = cases::positions(&text, "BODY");
        assert_eq!(following.len(), 1);
        assert!(positions[0] < following[0]);
        assert!(actual.links.is_empty());
        assert_eq!(actual.codes, Vec::<String>::new());
        let previous_item = case["former_owner"] == "previous-item";
        for cell in &actual.cells[positions[0]..positions[0] + old.len()] {
            assert_eq!(cell.style, cases::Style::default());
            assert_eq!(cell.owner.contains("/list/0/"), previous_item);
            assert!(!cell.owner.contains("/list/1/"));
        }
        assert!(actual.cells[following[0]].owner.contains(if previous_item {
            "/list/1/"
        } else {
            "/list/0/"
        }));
        assert_eq!(
            mant_ir::validate_document(content.document.as_ref().unwrap()),
            []
        );
    }
}
