//! Regression test for Pandoc-style pages whose only structure is `.SS`
//! subsections with no enclosing `.SH`.
//!
//! libmandoc parses these pages, but the section lowering used to accept a
//! subsection only when it was nested inside a `.SH` body. Root-level `.SS`
//! blocks were therefore dropped and the document was reported as having "no
//! readable sections". These pages must lower their subsections into visible
//! top-level sections instead.

use std::path::PathBuf;

use mant_core::parse_manual_source;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/roff/subsection-only.3")
}

#[test]
fn root_level_subsections_lower_into_visible_sections() {
    let document = parse_manual_source(&fixture_path()).expect("lower subsection-only fixture");

    let titles: Vec<&str> = document
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Name", "Synopsis", "Description", "Return value"],
        "each root-level .SS must become a visible section",
    );

    assert!(
        document
            .sections
            .iter()
            .all(|section| !section.blocks.is_empty()),
        "every promoted subsection must retain its body",
    );
}
