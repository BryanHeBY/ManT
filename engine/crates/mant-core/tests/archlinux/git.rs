//! Tests for the Arch Linux `git(1)` gzip fixture.

use crate::common::{self, GIT_SECTIONS};
use crate::fixtures::{archlinux_manual, archlinux_manual_query};
use mant_ast::{Block, ExcerptSelection, OutlineDetail};
use mant_core::{build_outline, build_outline_with_detail, select_excerpt};

/// Section topology (24 sections), nested children in ENVIRONMENT VARIABLES,
/// preformatted blocks in SYNOPSIS and OPTIONS, semantic `--help` option,
/// and ancillary command references.
#[test]
fn keeps_nested_sections_examples_and_inline_grouping() {
    let document = archlinux_manual("git");
    common::assert_section_topology("archlinux/git", document, GIT_SECTIONS);
    assert_eq!(document.sections.len(), 24);

    let environment = common::section(document, "ENVIRONMENT VARIABLES");
    assert!(
        environment
            .children
            .iter()
            .any(|child| child.title == "Git Diffs")
    );
    assert!(!common::section(document, "GIT COMMANDS").blocks.is_empty());

    let preformatted = common::document_blocks(document)
        .into_iter()
        .filter_map(common::as_preformatted)
        .collect::<Vec<_>>();
    assert_eq!(preformatted.len(), 4);

    let synopsis = common::section(document, "SYNOPSIS");
    let synopsis_pre = synopsis
        .blocks
        .iter()
        .find_map(common::as_preformatted)
        .expect("Git synopsis display");
    assert!(common::contains_emphasis(synopsis_pre, "git"));
    assert!(common::count_line_breaks(synopsis_pre) > 3);
    assert!(!common::inline_text(synopsis_pre).contains("\n\n"));

    common::assert_preformatted(
        common::section(document, "OPTIONS"),
        "git --git-dir=a.git",
        8,
    );
    common::assert_preformatted(
        common::section(document, "CONFIGURATION MECHANISM"),
        "[core]",
        4,
    );
    let git_diffs = environment
        .children
        .iter()
        .find(|child| child.title == "Git Diffs")
        .expect("Git Diffs subsection");
    common::assert_preformatted(git_diffs, "path old-file", 8);

    let help = common::nested_definition_items(common::section(document, "OPTIONS"))
        .into_iter()
        .find(|item| {
            item.identity
                .as_ref()
                .is_some_and(|identity| identity.names.iter().any(|name| name == "--help"))
        })
        .expect("semantic --help option");
    let option_summary = help
        .description
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. }
                if common::inline_text(children).contains("Prints the synopsis") =>
            {
                Some(children)
            }
            _ => None,
        })
        .expect("grouped -h description");
    assert!(common::contains_strong(option_summary, "--all"));
    assert!(common::contains_strong(option_summary, "-a"));
    assert!(common::inline_text(option_summary).contains("is given then all available"));

    let ancillary = common::section(document, "Ancillary Commands");
    let ancillary_text = common::block_slice_text(&ancillary.blocks);
    assert!(ancillary_text.contains("git-config(1)"));
    assert!(ancillary_text.contains("git-fast-export(1)"));

    let outline = build_outline_with_detail(&archlinux_manual_query("git"), OutlineDetail::Options)
        .expect("git option outline");
    assert!(common::find_outline_entry(&outline.nodes, "--help").is_some());
    assert!(common::find_outline_entry(&outline.nodes, "-C").is_some());
}

/// V2 outlines expose sub-section ids (`git-diffs-28`) and
/// `select_excerpt` retrieves their content by path.
#[test]
fn supports_outline_discovery_and_targeted_excerpts() {
    let query = archlinux_manual_query("git");
    let outline = build_outline(&query).expect("Git outline");
    let git_diffs = &outline.nodes[15].children()[3];
    assert_eq!(git_diffs.path(), "16.4");
    assert_eq!(git_diffs.id(), "git-diffs-28");
    assert_eq!(git_diffs.title(), "Git Diffs");

    let excerpt = select_excerpt(&query, &["git-diffs-28".to_owned(), "16.4".to_owned()])
        .expect("Git Diffs excerpt");
    assert_eq!(excerpt.selections.len(), 1);
    let ExcerptSelection::DocumentSection {
        path,
        breadcrumbs,
        section,
        ..
    } = &excerpt.selections[0]
    else {
        panic!("expected Git Diffs manual section");
    };
    assert_eq!(path, "16.4");
    assert_eq!(breadcrumbs[0].title, "ENVIRONMENT VARIABLES");
    assert!(common::block_slice_text(&section.blocks).contains("GIT_EXTERNAL_DIFF"));
}

/// No roff escapes or control characters leak into text values.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("archlinux/git", archlinux_manual("git"));
}
