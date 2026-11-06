//! Native table exits reach source-bound query and two Markdown readers.
#![cfg(feature = "roff")]
use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
#[path = "support/table_execution.rs"]
mod cases;
#[path = "support/explanation_ranges.rs"]
mod explanation_ranges;
#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "native_table_execution/names.rs"]
mod names;
#[path = "native_table_execution/queries.rs"]
mod queries;
#[path = "native_table_execution/styles.rs"]
mod styles;

#[test]
fn complete_table_exit_witnesses_survive_json_markdown_and_queries() {
    let cases = cases::cases();
    assert_eq!(cases.len(), 62);
    for case in &cases {
        let original = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
            let mut current = original.clone();
            for round in 0..=2 {
                assert_eq!(
                    mant_ir::validate_document(current.document.as_ref().unwrap()),
                    []
                );
                for width in [40, 78] {
                    cases::assert_follower(
                        &mant_render::render_query_man_at_width(&current, width),
                        case,
                    );
                }
                queries::assert_query(&current, &case.expected_label);
                styles::assert_following_links(&current, case, round);
                if round < 2 {
                    let markdown = render_markdown_with_options(&current, options);
                    let artifact = render_addressable_markdown_with_options(&current, options);
                    assert_eq!(artifact.text(), markdown, "{} round{round}", case.id);
                    current = json_roundtrip::json_string_roundtrip(
                        &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                    );
                }
            }
        }
    }
}
