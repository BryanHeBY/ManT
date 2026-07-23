//! Tests for the `rsync(1)` fixture — EXIT VALUES bullet normalisation
//! (the regression that motivated the normalisation pass).

use crate::common::{self, RSYNC_SECTIONS};
use crate::fixtures::{fedora44_manual, fedora44_manual_query};
use mant_ast::Block;
use mant_core::render_query_man;

/// Section topology: 32 sections from NAME through AUTHOR.
#[test]
fn keeps_section_topology() {
    common::assert_section_topology("fedora44/rsync", fedora44_manual("rsync"), RSYNC_SECTIONS);
}

/// EXIT VALUES uses `.IP o` markers that must be normalised into a bullet
/// list rather than a definition list with per-item `o` terms.
#[test]
fn exit_values_is_normalised_to_bullet_list() {
    let doc = fedora44_manual("rsync");
    let exit = common::section(doc, "EXIT VALUES");

    let has_bullet_list = exit.blocks.iter().any(|block| {
        matches!(
            block,
            Block::List {
                kind: mant_ast::ListKind::Bullet,
                ..
            }
        )
    });
    assert!(
        has_bullet_list,
        "rsync EXIT VALUES should contain a normalised bullet list"
    );

    // No definition list should remain — the `o` markers are uniform bullets.
    let has_definition_list = exit
        .blocks
        .iter()
        .any(|block| matches!(block, Block::DefinitionList { .. }));
    assert!(
        !has_definition_list,
        "rsync EXIT VALUES should not retain a definition list after normalisation"
    );
}

/// The bullet list items contain the expected exit codes.
#[test]
fn exit_values_contain_expected_codes() {
    let doc = fedora44_manual("rsync");
    let exit = common::section(doc, "EXIT VALUES");
    let text = common::block_slice_text(&exit.blocks);

    assert!(text.contains('0'), "exit code 0 missing in: {text:?}");
    assert!(
        text.contains("Success"),
        "expected 'Success' in exit values: {text:?}"
    );
}

/// --format man renders EXIT VALUES as a bullet list, not a definition list
/// with per-item `o` terms.
#[test]
fn man_format_renders_exit_values_as_bullet_list() {
    let output = render_query_man(&fedora44_manual_query("rsync"));

    // Should NOT contain "o " as a standalone term line.
    let exit_start = output
        .find("EXIT VALUES")
        .expect("EXIT VALUES section in man output");
    let exit_section = &output[exit_start..];
    // Look for the end of the section (next heading or end of string).
    let section_end = exit_section[12..]
        .find("\n\n")
        .map_or(exit_section.len(), |i| i + 12);
    let exit_chunk = &exit_section[..section_end + 200.min(exit_section.len() - section_end)];

    // No standalone "o\n" or "o  " term line.
    assert!(
        !exit_chunk.contains("\no\n") && !exit_chunk.contains("\no  "),
        "rsync EXIT VALUES should not have standalone 'o' term lines, got: {exit_chunk:?}"
    );
}

/// No roff escapes leak.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("fedora44/rsync", fedora44_manual("rsync"));
}

/// No duplicate vertical spacing.
#[test]
fn does_not_have_duplicate_vertical_spacing() {
    common::assert_no_duplicate_vertical_spacing(
        &fedora44_manual("rsync").sections,
        "fedora44/rsync",
    );
}
