//! Codec-internal lowering contracts; no query or rendering dependencies.
use crate::mandoc::native_execution;

use super::*;

#[test]
fn native_execution_report_owns_each_authored_and_automatic_target() {
    // The exact fixture was checked with the pinned CVS HTML renderer before
    // this assertion was written: it emits `Mixed.Section`,
    // `derived-command`, and `automatic_function` as three addressable IDs.
    // `tag.c::tag_postprocess()` moves each target to its final structural
    // owner, so the production projector must recover all three exclusively
    // from the owned execution report, without a second source interpretation.
    let path = std::path::Path::new("target-source-parity.7");
    let source = include_bytes!("../native_execution/fixtures/target-source-parity.7");
    let report = Parser::default()
        .execute_bytes(path, source, libmandoc_rs::ExecutionLimits::default())
        .expect("execute one owned native report");
    let document =
        native_execution::lower_native_document(path, &report).expect("native projection succeeds");

    let index = mant_ir::DocumentIndex::build(&document);
    for target in ["Mixed.Section", "derived-command", "automatic_function"] {
        assert!(
            index.fragment_target(target).is_some(),
            "native report did not retain target {target}: {document:#?}"
        );
    }
}

#[test]
fn semantic_identity_uses_the_same_composite_glyph_projection_as_visible_text() {
    let path = std::path::Path::new("semantic-composite-glyph.1");
    let source = b".TH SEMANTIC-COMPOSITE 1\n\
.SH OPTIONS\n\
.TP\n\
.B A\\z\\o'BC'D\n\
Description.\n";
    let document = parse_manual_bytes(path, source).expect("lower term with overstrike glyph");
    let index = mant_ir::DocumentIndex::build(&document);

    assert!(
        index.contains("term-ad"),
        "semantic projection: {document:#?}"
    );
    assert!(!index.contains("term-acd"));
}

#[test]
fn native_section_ids_ignore_unrelated_section_insertions() {
    let mut original = LoweringContext::new(None, None);
    let original_name = original.section_id("NAME");
    let original_options = original.section_id("OPTIONS");

    let mut edited = LoweringContext::new(None, None);
    assert_eq!(edited.section_id("NOTES"), "notes");
    assert_eq!(edited.section_id("NAME"), original_name);
    assert_eq!(edited.section_id("OPTIONS"), original_options);
    assert_eq!(edited.section_id("OPTIONS"), "options-2");
}

#[test]
fn native_section_ids_disambiguate_final_slug_collisions() {
    let mut context = LoweringContext::new(None, None);
    assert_eq!(context.section_id("FOO"), "foo");
    assert_eq!(context.section_id("FOO"), "foo-2");
    assert_eq!(context.section_id("FOO 2"), "foo-2-2");
}
