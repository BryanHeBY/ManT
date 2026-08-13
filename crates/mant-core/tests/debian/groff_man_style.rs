//! Tests for the Debian `groff_man_style(7)` gzip fixture.
//!
//! This page is a groff macro tutorial that intentionally contains
//! literal `\f` font escapes as subject matter, so markup-leak
//! assertions are not applied here.

use crate::common::{self, DEBIAN_GROFF_MAN_STYLE_SECTIONS};
use crate::fixtures::debian_manual;
use mant_ir::Inline;

/// 8-section topology (Name through See also).
#[test]
fn keeps_complete_section_topology() {
    let document = debian_manual("groff_man_style");
    common::assert_section_topology(
        "debian/groff_man_style",
        document,
        DEBIAN_GROFF_MAN_STYLE_SECTIONS,
    );
}

/// No duplicate vertical spacing.
#[test]
fn does_not_have_duplicate_vertical_spacing() {
    common::assert_no_duplicate_vertical_spacing(
        &debian_manual("groff_man_style").sections,
        "debian/groff_man_style",
    );
}

/// groff defines a portable fallback for its modern `.MR` macro. libmandoc
/// expands that fallback to italic text, but `ManT` must retain the original
/// cross-reference semantics for terminal navigation.
#[test]
fn keeps_mr_fallbacks_as_typed_manual_references() {
    let document = debian_manual("groff_man_style");
    let mut references = Vec::new();
    common::visit_document_inlines(document, &mut |inline| {
        if let Inline::Link {
            target:
                mant_ir::LinkTarget::Manual {
                    name,
                    section: Some(section),
                },
            ..
        } = inline
        {
            references.push((name.clone(), section.clone()));
        }
    });

    assert_eq!(references.len(), 47);
    for target in [("groff_man", "7"), ("tar", "1"), ("printf", "3")] {
        assert!(
            references
                .iter()
                .any(|reference| reference.0 == target.0 && reference.1 == target.1),
            "missing .MR target {target:?}"
        );
    }
}
