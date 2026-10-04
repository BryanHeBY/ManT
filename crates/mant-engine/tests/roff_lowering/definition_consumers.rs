//! Native capacity receipts and executed BODY boundaries reach real consumers.

use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, HeadBodyRelation, ResolvedContent};
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
                item.head_body_relation.joins_without_separator(),
                joined,
                "{}: independent native capacity edge",
                case.id
            );
            assert!(matches!(
                item.head_body_relation,
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
        let body_prefix = native_body_prefix(case, &cases);
        let mut expected = expected_rows(case, true);
        let body_row = expected
            .iter_mut()
            .find(|row| row.contains("BODY"))
            .unwrap();
        let body_start = body_row.find("BODY").unwrap();
        body_row.insert_str(body_start, &" ".repeat(body_prefix));
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let decoded = round_trip(&original);
        for content in [&original, &decoded] {
            assert_native_rows(case, content);
            assert_owner_ranges(case, content);
            let body = definition(content)
                .description
                .iter()
                .find_map(|block| match block {
                    Block::Paragraph { children, .. } => Some(mant_ir::inline_plain_text(children)),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                body,
                format!("{}BODY", " ".repeat(body_prefix)),
                "{}: accepted BODY cells",
                case.id
            );
            let markdown = render_markdown_with_options(content, MarkdownOptions::default());
            let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
            assert_eq!(
                imported_rows(imported_item_blocks(&imported)),
                expected,
                "{}: effective BODY boundary, {markdown}",
                case.id
            );
            assert_search(content, "HEAD", "HEAD");
            assert_search(content, "BODY", "BODY");
        }
    }
    assert_eq!(count, 112);
}

fn native_body_prefix(case: &Case, cases: &[Case]) -> usize {
    // These 112 exact sources reran all five pristine profiles before this
    // assertion. term.c::term_word writes a separator before interpreting an
    // empty/control-only word. Compare its real BODY column against the exact
    // source with only that word removed: field geometry stays unchanged.
    let empty_word = [".No \"\"\n", ".No \\&\n", ".No \\fB\n"]
        .into_iter()
        .find(|word| case.source.contains(word));
    let baseline_source = empty_word.map_or_else(
        || case.source.clone(),
        |word| case.source.replacen(word, "", 1),
    );
    let baseline = cases
        .iter()
        .find(|other| other.source == baseline_source)
        .unwrap();
    let body_column = |case: &Case| {
        let row = case
            .native_rows
            .iter()
            .find(|row| row.contains("BODY"))
            .unwrap();
        row[..row.find("BODY").unwrap()].chars().count()
    };
    let extra = body_column(case)
        .checked_sub(body_column(baseline))
        .unwrap();
    assert_eq!(
        extra,
        usize::from(empty_word.is_some()),
        "{}: native word-cell receipt",
        case.id
    );
    // termp_it_pre emits the inset BODY's fixed blank with term_word("\\ ").
    // It is accepted content when BODY shares that row; a real br/sp/Pp
    // consumes it before the subsequent BODY row. The registered native NBSP
    // receipt identifies that cell separately from ordinary field padding.
    let fixed = baseline
        .native_rows
        .iter()
        .find(|row| row.contains("BODY"))
        .unwrap()
        .chars()
        .filter(|character| *character == '\u{a0}')
        .count();
    fixed + extra
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
