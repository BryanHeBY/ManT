//! Tests for Fedora Linux 44's `sh(1)` alias of the Bash manual.

use mant_ir::SourceFormat;

use crate::common::{self, collect_sections, source_path_ends_with};
use crate::fixtures::fedora44_manual;

#[test]
fn parses_the_real_bash_backed_shell_manual() {
    let document = fedora44_manual("sh");
    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(document.meta.section.as_deref(), Some("1"));
    assert!(source_path_ends_with(document, "fedora44/sh.1.zst"));

    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    assert_eq!(document.sections.len(), 38);
    for title in ["NAME", "SHELL GRAMMAR", "REDIRECTION", "FUNCTIONS"] {
        assert!(sections.iter().any(|section| section.title == title));
    }
}

#[test]
fn keeps_the_bash_shell_page_spacing_and_anchors_normalized() {
    let document = fedora44_manual("sh");
    common::assert_anchor_ids_are_clean("fedora44/sh", document);
    common::assert_no_duplicate_vertical_spacing(&document.sections, "fedora44/sh");
}
