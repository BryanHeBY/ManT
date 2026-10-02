//! Native capacity receipts and executed BODY boundaries reach real consumers.

use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{HeadBodyRelation, ResolvedContent};
use mant_protocol::QueryBundle;

#[path = "definition_consumers/support.rs"]
mod support;
use support::*;

#[test]
fn capacity_edges_and_all_styled_seams_survive_wire_reader_and_queries() {
    // These exact 87 sources ran all five pristine profiles before freezing
    // the assertions. term.c::term_flushln uses the final open-row viscol and
    // minbl, including equality; mdoc_term.c::termp_it_pre adds two width cells.
    // Every marker's It HEAD/BODY ancestry is checked, not inferred from Xo.
    let cases = cases();
    let selected = cases.iter().filter(|case| case.category == "capacity");
    let mut count = 0;
    for case in selected {
        count += 1;
        assert_ast_owners(case);
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let decoded = round_trip(&original);
        for content in [&original, &decoded] {
            let item = definition(content);
            let joined = case
                .native_rows
                .iter()
                .any(|row| row.trim_start_matches(' ') == format!("{}BODY", case.head));
            assert_eq!(
                item.layout.head_body_relation.joins_without_separator(),
                joined,
                "{}: independent native capacity edge",
                case.id
            );
            assert!(matches!(
                item.layout.head_body_relation,
                HeadBodyRelation::Shared { .. }
            ));
            assert_eq!(item.terms.len(), 1, "{}", case.id);
            assert!(mant_ir::inline_plain_text(&item.terms[0]).contains(&case.head));
            assert!(!mant_ir::inline_plain_text(&item.terms[0]).contains("BODY"));
            assert_native_rows(case, content);
            assert_owner_ranges(case, content);
            {
                // The portable reader keeps arbitrary attributed HTML as
                // literal source. ADDRESSABLE destinations are instead
                // checked against the actual artifact and original owner.
                let markdown = render_markdown_with_options(content, MarkdownOptions::default());
                let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
                let blocks = imported_item_blocks(&imported);
                assert_eq!(
                    imported_rows(blocks),
                    expected_rows(case, false),
                    "{}: reader content/rows, {markdown}",
                    case.id
                );
                assert_carrier(blocks, &case.head, &case.head_carrier, &case.head);
                assert_carrier(blocks, "BODY", &case.body_carrier, "BODY");
            }
            assert_search(content, &case.head, &case.head);
            assert_search(content, "BODY", "BODY");
            if case.head_carrier == "No" && case.body_carrier == "No" {
                let word = if joined {
                    format!("{}BODY", case.head)
                } else {
                    format!("{} BODY", case.head)
                };
                assert_search(content, &word, &word);
            }
        }
        assert_eq!(original, decoded, "projections preserve original owners");
    }
    assert_eq!(count, 87);
}

#[test]
fn effective_body_start_keeps_hard_rows_paragraphs_and_empty_node_controls() {
    // roff_term.c::roff_term_pre_br ends the active row; pre_sp and
    // mdoc_term.c::termp_pp_pre additionally call term_vspace. Positive
    // distance may become one CommonMark paragraph boundary, never a soft
    // newline that joins HEAD and BODY. Empty output blocks are not boundaries.
    let cases = cases();
    let mut count = 0;
    for case in cases.iter().filter(|case| case.category == "body-boundary") {
        count += 1;
        assert_ast_owners(case);
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let decoded = round_trip(&original);
        for content in [&original, &decoded] {
            assert_native_rows(case, content);
            assert_owner_ranges(case, content);
            let markdown = render_markdown_with_options(content, MarkdownOptions::default());
            let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
            assert_eq!(
                imported_rows(imported_item_blocks(&imported)),
                expected_rows(case, true),
                "{}: effective BODY boundary, {markdown}",
                case.id
            );
            assert_search(content, "HEAD", "HEAD");
            assert_search(content, "BODY", "BODY");
        }
    }
    assert_eq!(count, 112);
}

fn round_trip(content: &ResolvedContent) -> ResolvedContent {
    let wire = serde_json::to_string(&QueryBundle::from(content)).unwrap();
    let restored: ResolvedContent = serde_json::from_str::<QueryBundle>(&wire).unwrap().into();
    assert_eq!(&restored, content);
    assert_eq!(
        mant_ir::validate_document(restored.document.as_ref().unwrap()),
        mant_ir::validate_document(content.document.as_ref().unwrap()),
        "wire decoding cannot change validation of bare relative Lk targets"
    );
    restored
}
