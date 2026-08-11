//! Tests for ripgrep's official MSVC Windows release manual.

use mant_ast::OutlineDetail;
use mant_core::{build_outline_with_detail, render_excerpt_markdown, select_excerpt};

use crate::common::{self, count_outline_entries, find_outline_entry};
use crate::fixtures::{windows_release_manual, windows_release_query};

const RG_SECTIONS: &[&str] = &[
    "NAME",
    "SYNOPSIS",
    "DESCRIPTION",
    "REGEX SYNTAX",
    "POSITIONAL ARGUMENTS",
    "OPTIONS",
    "EXIT STATUS",
    "AUTOMATIC FILTERING",
    "CONFIGURATION FILES",
    "SHELL COMPLETION",
    "CAVEATS",
    "VERSION",
    "HOMEPAGE",
    "AUTHORS",
];

#[test]
fn keeps_release_metadata_sections_and_semantic_options() {
    let document = windows_release_manual("rg");
    common::assert_section_topology("windows-releases/rg", document, RG_SECTIONS);
    assert_eq!(document.meta.section.as_deref(), Some("1"));
    assert_eq!(document.meta.date.as_deref(), Some("2026-07-15"));
    assert_eq!(document.meta.os.as_deref(), Some("15.2.0 (rev e89fff89ac)"));

    let outline = build_outline_with_detail(&windows_release_query("rg"), OutlineDetail::Entries)
        .expect("build rg option outline");
    assert_eq!(count_outline_entries(&outline.nodes), 104);
    assert!(find_outline_entry(&outline.nodes, "--glob").is_some());
}

#[test]
fn renders_the_reviewed_glob_option_as_a_targeted_excerpt() {
    let query = windows_release_query("rg");
    let excerpt = select_excerpt(&query, &["glob".to_owned()]).expect("select rg --glob");
    let markdown = render_excerpt_markdown(&excerpt);

    assert!(markdown.contains("-g, --glob"));
    assert!(markdown.contains("Globbing rules match **.gitignore** globs"));
    assert!(markdown.contains("Precede a glob with a **!** to exclude it"));
}

#[test]
fn does_not_leak_roff_markup_or_duplicate_spacing() {
    let document = windows_release_manual("rg");
    common::assert_document_has_no_source_markup("windows-releases/rg", document);
    common::assert_no_duplicate_vertical_spacing(&document.sections, "windows-releases/rg");
}
