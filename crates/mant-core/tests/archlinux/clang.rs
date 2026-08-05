//! Tests for the Arch Linux `clang(1)` gzip fixture.

use crate::common::{self, CLANG_SECTIONS};
use crate::fixtures::archlinux_manual;
use mant_ast::Block;

/// Stage-selection options, Sphinx INDENT wrappers that must collapse into
/// one semantic option list, `.in` dimension suppression, and preformatted
/// language standards.
#[test]
fn keeps_option_pairs_and_discards_rst_control_dimensions() {
    let document = archlinux_manual("clang");
    common::assert_section_topology("archlinux/clang", document, CLANG_SECTIONS);

    let stage_options = common::section(document, "Stage Selection Options");
    for (term, description) in [
        ("-E", "Run the preprocessor stage."),
        (
            "-fsyntax-only",
            "Run the preprocessor, parser and semantic analysis stages.",
        ),
        ("-c", "generating a target \".o\" object file"),
    ] {
        let item = common::definition_items(stage_options)
            .into_iter()
            .find(|item| {
                item.terms
                    .iter()
                    .any(|value| common::inline_text(value) == term)
            })
            .unwrap_or_else(|| panic!("missing Clang option {term}"));
        assert!(common::block_slice_text(&item.description).contains(description));
    }

    let target_options = common::section(document, "Target Selection Options");
    assert_eq!(
        target_options.spacing_before_lines, 1,
        "the SS macro must retain its leading paragraph distance",
    );
    let target_lists = target_options
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        target_lists.len(),
        1,
        "Sphinx INDENT wrappers must remain one semantic option list",
    );
    assert!(target_lists[0].len() > 5);
    assert_eq!(target_lists[0][0].spacing_before_lines, Some(0));
    let target_list_layout = target_options
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("Target Selection definition layout");
    assert_eq!(
        target_list_layout.spacing_before_lines, 1,
        "the first option must retain TP spacing after introductory prose",
    );
    assert!(
        target_lists[0]
            .iter()
            .skip(1)
            .all(|item| item.spacing_before_lines == Some(1)),
        "default man(7) paragraph distance must survive INDENT wrappers",
    );

    let code_generation = common::section(document, "Code Generation Options");
    let first_code_generation_layout = code_generation
        .blocks
        .first()
        .and_then(|block| match block {
            Block::DefinitionList { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("Code Generation starts with an option list");
    assert_eq!(
        first_code_generation_layout.spacing_before_lines, 0,
        "a transparent INDENT wrapper must not add space before a section's first block",
    );

    let phantom_dimensions = common::document_blocks(document)
        .into_iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(common::inline_text(children)),
            _ => None,
        })
        .filter(|text| {
            let text = text.trim();
            !text.is_empty()
                && text.ends_with('u')
                && text[..text.len() - 1]
                    .chars()
                    .all(|character| character.is_ascii_digit())
        })
        .count();
    assert_eq!(phantom_dimensions, 0, "roff .in arguments leaked as text");

    let language = common::section(document, "Language Selection and Mode Options");
    assert!(
        common::document_blocks_from_sections(std::slice::from_ref(language))
            .iter()
            .filter_map(|block| common::as_preformatted(block))
            .any(|children| common::inline_text(children).contains("iso9899:1990"))
    );
}

/// No roff escapes or dimension values leak into inline text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("archlinux/clang", archlinux_manual("clang"));
}
