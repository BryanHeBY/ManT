//! Source argument normalization and actual extended-HEAD placement.
#[path = "support/native_source_cases.rs"]
mod native_source_cases;

#[test]
fn width_macro_text_and_neighboring_arguments_follow_native_validation() {
    for case in native_source_cases::cases(include_str!("fixtures/list_arguments.json"), 40) {
        native_source_cases::assert_case(&case);
    }
}
