//! Source-bound delayed text through the actual public consumer interfaces.
#![cfg(feature = "roff")]
#[path = "support/deferred_ownership.rs"]
mod cases;
#[path = "support/explanation_ranges.rs"]
mod explanation_ranges;
#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "deferred_ownership/markdown.rs"]
mod markdown;
#[path = "deferred_ownership/queries.rs"]
mod queries;

#[path = "deferred_ownership/annotations.rs"]
mod annotation_ownership;
#[path = "deferred_ownership/publication.rs"]
mod publication;

#[test]
fn original_members_survive_json_two_markdown_readers_and_queries() {
    for (case, _) in cases::consumers() {
        let original = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        cases::assert_rich(&original, &case, true);
        cases::assert_constraints(&original, &case);
        queries::check(&original, &case);
        markdown::check(&original, &case);
    }
}
