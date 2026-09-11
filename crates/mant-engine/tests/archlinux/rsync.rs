//! Tests for the Arch Linux `rsync(1)` fixture and its authored ASCII marks.

use crate::common::{self, RSYNC_SECTIONS};
use crate::fixtures::{archlinux_manual, archlinux_manual_query};
use mant_ir::Block;
use mant_render::render_query_man;

/// Section topology: 32 sections from NAME through AUTHOR.
#[test]
fn keeps_section_topology() {
    common::assert_section_topology("archlinux/rsync", archlinux_manual("rsync"), RSYNC_SECTIONS);
}

/// `.IP o` does not prove a bullet: retain the original marks and payloads.
#[test]
fn exit_values_retains_literal_o_tags() {
    let doc = archlinux_manual("rsync");
    let exit = common::section(doc, "EXIT VALUES");

    let has_bullet_list = exit.blocks.iter().any(|block| {
        matches!(
            block,
            Block::List {
                kind: mant_ir::ListKind::Bullet,
                ..
            }
        )
    });
    assert!(
        !has_bullet_list,
        "literal ASCII tags must not be silently replaced by bullets"
    );

    // Preserve ambiguous source marks without a section-name heuristic.
    let has_definition_list = exit
        .blocks
        .iter()
        .any(|block| matches!(block, Block::DefinitionList { .. }));
    assert!(
        has_definition_list,
        "rsync EXIT VALUES should retain its literal marks"
    );
    assert!(common::definition_items(exit).iter().all(|item| {
        item.terms
            .iter()
            .all(|term| common::inline_text(term) == "o")
    }));
}

/// The tagged items contain the expected exit codes.
#[test]
fn exit_values_contain_expected_codes() {
    let doc = archlinux_manual("rsync");
    let exit = common::section(doc, "EXIT VALUES");
    let text = common::block_slice_text(&exit.blocks);

    assert!(text.contains('0'), "exit code 0 missing in: {text:?}");
    assert!(
        text.contains("Success"),
        "expected 'Success' in exit values: {text:?}"
    );
}

/// Text output preserves source `o` tags rather than inventing `-` marks.
#[test]
fn man_format_renders_exit_values_with_literal_marks() {
    let output = render_query_man(&archlinux_manual_query("rsync"));

    let exit_start = output
        .find("EXIT VALUES")
        .expect("EXIT VALUES section in man output");
    let exit_section = &output[exit_start..];
    // Look for the end of the section (next heading or end of string).
    let section_end = exit_section[12..]
        .find("\n\n")
        .map_or(exit_section.len(), |i| i + 12);
    let exit_chunk = &exit_section[..section_end + 200.min(exit_section.len() - section_end)];

    assert!(
        exit_chunk
            .lines()
            .any(|line| line.trim_start().starts_with("o ")),
        "rsync EXIT VALUES lost the source 'o' marks: {exit_chunk:?}"
    );
}

/// No roff escapes leak.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("archlinux/rsync", archlinux_manual("rsync"));
}

/// No duplicate vertical spacing.
#[test]
fn does_not_have_duplicate_vertical_spacing() {
    common::assert_bounded_vertical_spacing(&archlinux_manual("rsync").sections, "archlinux/rsync");
}
