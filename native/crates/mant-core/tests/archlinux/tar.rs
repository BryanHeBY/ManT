//! Tests for the Arch Linux `tar(1)` gzip fixture.

use crate::common::{self, TAR_SECTIONS};
use crate::fixtures::archlinux_manual;

/// Three-part synopsis (Traditional, UNIX, GNU), >=15 OPTIONS sub-sections,
/// and `--create` option in the Operation mode definition list.
#[test]
fn keeps_definition_lists_subsections_and_inline_styles() {
    let document = archlinux_manual("tar");
    common::assert_section_topology("archlinux/tar", document, TAR_SECTIONS);

    let synopsis = common::section(document, "SYNOPSIS");
    assert_eq!(
        synopsis
            .children
            .iter()
            .map(|child| child.title.as_str())
            .collect::<Vec<_>>(),
        ["Traditional usage", "UNIX-style usage", "GNU-style usage"]
    );
    let tar_options = common::section(document, "OPTIONS");
    assert!(tar_options.children.len() >= 15);
    assert!(
        common::definition_items(common::section(document, "Operation mode"))
            .iter()
            .flat_map(|item| &item.terms)
            .any(|term| common::inline_text(term).contains("--create"))
    );
}

/// No roff escapes leak into text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("archlinux/tar", archlinux_manual("tar"));
}
