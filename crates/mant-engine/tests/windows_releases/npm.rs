//! Tests for npm's CRLF manual from the official Node.js Windows ZIP.

use crate::common::{self, block_slice_text, collect_sections};
use crate::fixtures::windows_release_manual;

#[test]
fn keeps_the_nested_npm_manual_and_windows_build_requirements() {
    let document = windows_release_manual("npm");
    assert_eq!(document.meta.title.as_deref(), Some("NPM"));
    assert_eq!(document.meta.manual_section.as_deref(), Some("1"));
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
    assert!(dependencies.contains(r"using the \fBgit\fR"));
    assert!(dependencies.contains("On Windows, Python and Microsoft Visual Studio C++ are needed"));
}

#[test]
fn preserves_authored_literal_font_spellings_without_decoding_them_twice() {
    let document = windows_release_manual("npm");
    let rendered = mant_engine::render_query_text(&common::query_for_document("npm", document));
    // The source spells these backslashes with [rs]. Both mandoc and groff
    // print the literal font strings; a blanket no-\\f assertion hid data.
    assert!(rendered.contains(r"\fBgit\fR"));
    assert!(rendered.contains(r"\fBpackage.json\fR"));
    assert!(!rendered.contains(r"\[rs]"));
    assert!(!rendered.contains(['\u{1d}', '\u{1e}', '\u{1f}']));
    common::assert_no_duplicate_vertical_spacing(&document.sections, "windows-releases/npm");
}
