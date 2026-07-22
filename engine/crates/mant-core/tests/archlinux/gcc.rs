//! Tests for the Arch Linux `gcc(1)` gzip fixture.

use crate::common::{self, GCC_SECTIONS};
use crate::fixtures::archlinux_manual;
use mant_ast::Block;

/// Deeply nested OPTIONS hierarchy (20 sub-sections), >250 preformatted
/// blocks, C++ class-hierarchy examples, phantom-paragraph suppression,
/// and definition-list paragraph spacing.
#[test]
fn keeps_large_hierarchy_fonts_and_pod_displays_without_control_text() {
    let document = archlinux_manual("gcc");
    common::assert_section_topology("archlinux/gcc", document, GCC_SECTIONS);

    let options = common::section(document, "OPTIONS");
    assert_eq!(options.children.len(), 20);
    assert_eq!(options.children[0].title, "Option Summary");
    assert_eq!(
        options.children[1].title,
        "Options Controlling the Kind of Output"
    );
    assert!(
        options
            .children
            .iter()
            .any(|child| child.title == "Options to Request or Suppress Warnings")
    );

    let synopsis = common::section(document, "SYNOPSIS");
    let synopsis_inlines = synopsis
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. } => Some(children.as_slice()),
            _ => None,
        })
        .expect("GCC synopsis paragraph");
    assert!(common::contains_strong(synopsis_inlines, "-std="));
    assert!(common::contains_emphasis(synopsis_inlines, "standard"));

    let blocks = common::document_blocks(document);
    let displays = blocks
        .iter()
        .filter_map(|block| common::as_preformatted(block))
        .collect::<Vec<_>>();
    assert!(displays.len() > 250);
    let class_example = displays
        .iter()
        .find(|children| common::inline_text(children).contains("struct A { int a; };"))
        .expect("GCC class hierarchy example");
    assert!(common::inline_text(class_example).contains("struct C : B, A { };"));
    assert!(displays.iter().all(|children| {
        let text = common::inline_text(children);
        text.trim() != "CW" && text.trim() != "R"
    }));

    let phantom_paragraphs = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(common::inline_text(children)),
            _ => None,
        })
        .filter(|text| matches!(text.trim(), "0" | "4"))
        .count();
    assert_eq!(
        phantom_paragraphs, 0,
        "roff request arguments leaked as text"
    );

    let cxx_options = common::section(document, "Options Controlling C++ Dialect");
    let suggest_final_methods = common::nested_definition_items(cxx_options)
        .into_iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|term| common::inline_text(term).contains("-Wsuggest-final-methods"))
        })
        .expect("GCC -Wsuggest-final-methods option");
    assert_eq!(
        suggest_final_methods.spacing_before_lines,
        Some(1),
        "default man(7) paragraph distance must separate adjacent GCC options",
    );
}

/// No roff escapes or `0` / `4` dimension values leak into text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("archlinux/gcc", archlinux_manual("gcc"));
}
