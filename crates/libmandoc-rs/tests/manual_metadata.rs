//! Full section lookup, suffix fallback and explicit manual-volume precedence.
#[path = "support/native_source_cases.rs"]
mod native_source_cases;

#[test]
fn section_suffixes_preserve_exact_spelling_and_volume_precedence() {
    // man_validate.c::post_TH uses the explicit fifth operand first.
    // mdoc_validate.c::post_dt treats operand three as architecture; an
    // excess fourth operand cannot replace msec.c's exact-first volume.
    for case in native_source_cases::cases(include_str!("fixtures/manual_metadata.json"), 21) {
        native_source_cases::assert_case(&case);
    }
}
