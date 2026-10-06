//! Bibliographic suffix inference never invents visible content or new owners.
#[path = "support/native_source_contracts.rs"]
mod native_source_contracts;

#[test]
fn native_manual_metadata_round_trips_without_changing_content_ownership() {
    let fixture = include_str!("../../libmandoc-rs/tests/fixtures/manual_metadata.json");
    for case in native_source_contracts::cases(fixture, 21) {
        native_source_contracts::assert_case(&case);
    }
}
