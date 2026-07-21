//! Tests for the Arch Linux `ls(1)` gzip fixture.

use crate::common::{self, LS_SECTIONS};
use crate::fixtures::archlinux_manual;

/// Section topology, 60-option definition list in DESCRIPTION, exit-status
/// items, and inline strong formatting.
#[test]
fn keeps_section_topology_and_definition_lists() {
    let document = archlinux_manual("ls");
    common::assert_section_topology("archlinux/ls", document, LS_SECTIONS);

    let description = common::section(document, "DESCRIPTION");
    let options = common::definition_items(description);
    assert_eq!(options.len(), 60);
    let all = options
        .iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| common::inline_text(term).contains("--all"))
        })
        .expect("ls --all option");
    assert!(
        all.terms
            .iter()
            .any(|term| common::contains_strong(term, "-a, --all"))
    );

    let exit = common::section(document, "Exit status:");
    let exit_items = common::definition_items(exit);
    assert_eq!(exit_items.len(), 3);
    assert_eq!(common::inline_text(&exit_items[0].terms[0]), "0");
    assert!(common::block_slice_text(&exit_items[2].description).contains("serious trouble"));
}

/// No roff escapes, HTML markup, or ASCII control characters leak into
/// visible text values.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("archlinux/ls", archlinux_manual("ls"));
}
