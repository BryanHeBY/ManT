//! Codec-internal lowering contracts; no query or rendering dependencies.
use super::*;

#[test]
fn target_identity_is_independent_from_optional_raw_source_recovery() {
    let path = std::path::Path::new("target-source-parity.7");
    let source = b".Dd September 4, 2026\n.Dt TARGET-SOURCE-PARITY 7\n.Os\n\
.Tg Mixed.Section\n\
.Sh HEADING\n\
.Pp\n\
Paragraph before a target request.\n\
.Tg\n\
.Ic derived-command\n\
.Pp\n\
.Fn automatic_function\n";
    let with_source = parse_manual_bytes(path, source).expect("lower source-aware document");
    let report = Parser::default()
        .parse_bytes(path, source)
        .expect("parse owned native tree");
    let without_source = lower_mandoc_document(path, &report);

    let with_source_index = mant_ir::DocumentIndex::build(&with_source);
    let without_source_index = mant_ir::DocumentIndex::build(&without_source);
    for target in ["Mixed.Section", "derived-command", "automatic_function"] {
        assert_eq!(
            with_source_index
                .fragment_target(target)
                .map(mant_ir::NodeId::as_str),
            without_source_index
                .fragment_target(target)
                .map(mant_ir::NodeId::as_str),
            "target {target} changed when raw source recovery was unavailable"
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
