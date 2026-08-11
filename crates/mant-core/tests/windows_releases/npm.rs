//! Tests for npm's CRLF manual from the official Node.js Windows ZIP.

use crate::common::{self, block_slice_text, collect_sections};
use crate::fixtures::windows_release_manual;

#[test]
fn keeps_the_nested_npm_manual_and_windows_build_requirements() {
    let document = windows_release_manual("npm");
    assert_eq!(document.meta.title.as_deref(), Some("NPM"));
    assert_eq!(document.meta.section.as_deref(), Some("1"));
    assert_eq!(document.meta.date.as_deref(), Some("June 2026"));
    assert_eq!(document.meta.os.as_deref(), Some("NPM@11.17.0"));
    assert_eq!(document.sections.len(), 1);

    let mut sections = Vec::new();
    collect_sections(&document.sections, &mut sections);
    assert_eq!(sections.len(), 14);
    for title in ["Synopsis", "Dependencies", "Directories", "Developer Usage"] {
        assert!(
            sections.iter().any(|section| section.title == title),
            "missing reviewed npm section {title}",
        );
    }

    let dependencies = block_slice_text(&common::section(document, "Dependencies").blocks);
    assert!(dependencies.contains("using the git"));
    assert!(dependencies.contains("On Windows, Python and Microsoft Visual Studio C++ are needed"));
    assert!(!dependencies.contains("\\fBgit\\fR"));
}

#[test]
fn removes_redundant_generator_fonts_without_hiding_other_markup() {
    let document = windows_release_manual("npm");
    common::assert_document_has_no_source_markup("windows-releases/npm", document);
    common::assert_no_duplicate_vertical_spacing(&document.sections, "windows-releases/npm");
}
