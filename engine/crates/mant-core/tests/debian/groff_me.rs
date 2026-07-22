//! Tests for the Debian `groff_me(7)` gzip fixture.

use crate::common::{self, DEBIAN_GROFF_ME_SECTIONS};
use crate::fixtures::debian_manual;

/// 6-section topology (Name, Synopsis, Description, Files, Notes, See also).
#[test]
fn keeps_complete_section_topology() {
    let document = debian_manual("groff_me");
    common::assert_section_topology("debian/groff_me", document, DEBIAN_GROFF_ME_SECTIONS);
}

/// No roff escapes leak into inline text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("debian/groff_me", debian_manual("groff_me"));
}

/// No duplicate vertical spacing.
#[test]
fn does_not_have_duplicate_vertical_spacing() {
    common::assert_no_duplicate_vertical_spacing(
        &debian_manual("groff_me").sections,
        "debian/groff_me",
    );
}
