//! Tests for the Debian `mt-gnu(1)` gzip fixture (from cpio).

use crate::common::{self, DEBIAN_MT_GNU_SECTIONS};
use crate::fixtures::debian_manual;

/// Section topology and anchor-ID control-character sanitisation.
#[test]
fn keeps_complete_section_topology_and_clean_anchor_ids() {
    let document = debian_manual("mt-gnu");
    common::assert_section_topology("debian/mt-gnu", document, DEBIAN_MT_GNU_SECTIONS);
    common::assert_anchor_ids_are_clean("debian/mt-gnu", document);
}

/// No consecutive vertical-space blocks duplicate the same gap.
#[test]
fn does_not_have_duplicate_vertical_spacing() {
    common::assert_no_duplicate_vertical_spacing(
        &debian_manual("mt-gnu").sections,
        "debian/mt-gnu",
    );
}
