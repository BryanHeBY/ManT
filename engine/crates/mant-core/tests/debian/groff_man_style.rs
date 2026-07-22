//! Tests for the Debian `groff_man_style(7)` gzip fixture.
//!
//! This page is a groff macro tutorial that intentionally contains
//! literal `\f` font escapes as subject matter, so markup-leak
//! assertions are not applied here.

use crate::common::{self, DEBIAN_GROFF_MAN_STYLE_SECTIONS};
use crate::fixtures::debian_manual;

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
