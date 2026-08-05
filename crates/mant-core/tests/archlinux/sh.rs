//! Tests for Arch Linux's POSIX `sh(1p)` gzip fixture.

use mant_ast::SourceFormat;

use crate::common::{self, collect_sections};
use crate::fixtures::archlinux_manual;

#[test]
fn parses_the_posix_shell_manual_from_its_real_section() {
    let document = archlinux_manual("sh");
    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(document.meta.section.as_deref(), Some("1P"));
    assert!(
        document
            .source
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("archlinux/sh.1p.gz")),
    );

    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    assert_eq!(document.sections.len(), 22);
    for title in ["NAME", "SYNOPSIS", "EXTENDED DESCRIPTION", "RATIONALE"] {
        assert!(sections.iter().any(|section| section.title == title));
    }
}

#[test]
fn keeps_the_posix_shell_page_structurally_clean() {
    let document = archlinux_manual("sh");
    common::assert_document_has_no_source_markup("archlinux/sh", document);
    common::assert_no_duplicate_vertical_spacing(&document.sections, "archlinux/sh");
}
