//! Exercises terminal lowering against the same real roff corpus as mant-core.
//!
//! These tests intentionally avoid distribution-specific pixel snapshots.
//! They prove that large, structurally different manuals survive every common
//! terminal width while keeping all sidebar destinations addressable.

use std::path::{Path, PathBuf};

use mant_ast::{QueryBundle, QuerySchema};
use mant_ui::DocumentView;

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real")
        .join(relative)
}

fn view(relative: &str) -> DocumentView {
    let document = mant_core::parse_manual_source(&fixture(relative)).expect("parse real fixture");
    DocumentView::new(&QueryBundle {
        schema: QuerySchema::V3,
        label: relative.to_owned(),
        document: Some(document),
        tldr: None,
    })
}

#[test]
fn real_manuals_render_at_narrow_and_wide_terminal_widths() {
    for relative in [
        "archlinux/gawk.1.zst",
        "debian/sh.1.gz",
        "fedora44/gcc.1.zst",
        "fedora44/tar.1.zst",
    ] {
        let view = view(relative);
        assert!(
            !view.navigation().is_empty(),
            "{relative} has no navigation"
        );

        for width in [32, 80, 132] {
            let rendered = view.render(width);
            assert!(
                rendered.row_count >= view.section_count(),
                "{relative} lost content at width {width}"
            );
            for item in view.navigation() {
                assert!(
                    rendered.anchor_row(&item.target_id).is_some(),
                    "{relative} lost anchor {} for navigation {} at width {width}",
                    item.target_id,
                    item.id
                );
            }
            if let Some((row, character)) =
                rendered
                    .text
                    .lines
                    .iter()
                    .enumerate()
                    .find_map(|(row, line)| {
                        line.to_string()
                            .chars()
                            .find(|character| character.is_control())
                            .map(|character| (row, character))
                    })
            {
                panic!(
                    "{relative} emitted control U+{:04X} on row {row} at width {width}",
                    u32::from(character)
                );
            }
        }
    }
}

#[test]
fn semantic_definition_anchors_survive_real_tar_lowering() {
    let document = mant_core::parse_manual_source(&fixture("fedora44/tar.1.zst"))
        .expect("parse Fedora tar fixture");
    let acls_id = document
        .sections
        .iter()
        .flat_map(section_definitions)
        .find_map(|identity| {
            identity
                .names
                .iter()
                .any(|name| name == "--acls")
                .then(|| identity.id.clone())
        })
        .expect("tar --acls identity");
    let view = DocumentView::new(&QueryBundle {
        schema: QuerySchema::V3,
        label: "tar".to_owned(),
        document: Some(document),
        tldr: None,
    });

    assert!(view.render(80).anchor_row(&acls_id).is_some());
}

fn section_definitions(section: &mant_ast::Section) -> Vec<&mant_ast::DefinitionIdentity> {
    let mut definitions = Vec::new();
    collect_block_definitions(&section.blocks, &mut definitions);
    for child in &section.children {
        definitions.extend(section_definitions(child));
    }
    definitions
}

fn collect_block_definitions<'a>(
    blocks: &'a [mant_ast::Block],
    definitions: &mut Vec<&'a mant_ast::DefinitionIdentity>,
) {
    for block in blocks {
        match block {
            mant_ast::Block::DefinitionList { items, .. } => {
                for item in items {
                    if let Some(identity) = &item.identity {
                        definitions.push(identity);
                    }
                    collect_block_definitions(&item.description, definitions);
                }
            }
            mant_ast::Block::List { items, .. } => {
                for item in items {
                    collect_block_definitions(&item.blocks, definitions);
                }
            }
            mant_ast::Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    collect_block_definitions(&cell.blocks, definitions);
                }
            }
            mant_ast::Block::Paragraph { .. }
            | mant_ast::Block::Preformatted { .. }
            | mant_ast::Block::Equation { .. }
            | mant_ast::Block::VerticalSpace { .. }
            | mant_ast::Block::ThematicBreak { .. }
            | mant_ast::Block::Unsupported { .. } => {}
        }
    }
}
