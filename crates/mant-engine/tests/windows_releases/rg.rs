//! Tests for ripgrep's official MSVC Windows release manual.
#[path = "../../src/semantic_test_read.rs"]
mod semantic_read;

use mant_ir::{Block, ListKind};
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery, OutlineDetail};
use mant_query::{build_outline_with_detail, explain_query};
use mant_render::render_excerpt_markdown;

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
    assert_eq!(document.meta.manual_section.as_deref(), Some("1"));
    assert_eq!(document.meta.date.as_deref(), Some("2026-07-15"));
    assert_eq!(document.meta.os.as_deref(), Some("15.2.0 (rev e89fff89ac)"));

    let outline = build_outline_with_detail(&windows_release_query("rg"), OutlineDetail::Entries)
        .expect("build rg option outline");
    assert_eq!(count_outline_entries(&outline.nodes), 136);
    // Each exact short + italic ALLCAPS + long declaration ran the pinned
    // CVS -Tutf8 reference first. term.c::term_word() ends the italic run
    // before the comma; man_term.c::pre_RS() only indents the description.
    for name in ["--regexp", "--threads", "--glob"] {
        assert!(find_outline_entry(&outline.nodes, name).is_some(), "{name}");
    }

    for (section, length) in [("AUTOMATIC FILTERING", 4), ("CONFIGURATION FILES", 2)] {
        assert!(
            common::section(document, section)
                .blocks
                .iter()
                .any(|block| matches!(
                    block,
                    Block::List {
                        kind: ListKind::Ordered { start: Some(1) },
                        items,
                        ..
                    } if items.len() == length
                ))
        );
    }
}

#[test]
fn renders_the_reviewed_glob_option_as_a_targeted_excerpt() {
    let query = windows_release_query("rg");
    let excerpt =
        semantic_read::semantic_excerpt(&query, &["--glob".to_owned()]).expect("select rg --glob");
    let markdown = render_excerpt_markdown(&excerpt);

    assert!(markdown.contains("-g, --glob"));
    assert!(markdown.contains("Globbing rules match **.gitignore** globs"));
    assert!(markdown.contains("Precede a glob with a **!** to exclude it"));
}

#[test]
fn repeated_space_hanging_options_have_direct_flow_explanations() {
    // The exact rg fixture ran pinned CVS -Ttree first. roff.c::
    // roff_node_alloc() retains each .sp before a complete declaration;
    // man_macro.c::blk_exp() creates its direct RS description.
    let query = windows_release_query("rg");
    for (name, line, names) in [
        ("--threads", 611, &["-j", "--threads"][..]),
        ("--glob", 721, &["-g", "--glob"][..]),
    ] {
        let result = explain_query(
            &query,
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, 1, "{name}");
        let direct = result
            .evidence
            .iter()
            .find(|item| item.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(direct.source.unwrap().line, line, "{name}");
        assert_eq!(direct.entry.as_ref().unwrap().names, names, "{name}");
        result.validate_references().unwrap();
    }
}

#[test]
fn does_not_leak_roff_markup_or_duplicate_spacing() {
    let document = windows_release_manual("rg");
    common::assert_document_has_no_source_markup("windows-releases/rg", document);
    common::assert_bounded_vertical_spacing(document, "windows-releases/rg");
}
