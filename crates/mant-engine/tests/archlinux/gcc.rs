//! Tests for the Arch Linux `gcc(1)` gzip fixture.

use crate::common::{self, GCC_SECTIONS};
use crate::fixtures::archlinux_manual;
use mant_ir::Block;

#[test]
fn explanation_retains_both_help_definitions_and_the_qualified_tail() {
    let query = common::query_for_document("gcc", archlinux_manual("gcc"));
    let result = mant_query::select_explanation(&query, "--help").unwrap();
    assert_eq!(result.outcome, mant_protocol::ExplanationOutcome::Evidence);
    let named = result
        .evidence
        .iter()
        .filter(|evidence| {
            evidence
                .bases
                .iter()
                .any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Name { .. }))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        named.len(),
        2,
        "independently documented plain and CLASS help"
    );
    assert_ne!(named[0].outline.node.id(), named[1].outline.node.id());
    assert!(result.evidence.iter().any(|evidence| {
        evidence.entry.is_none()
            && evidence
                .bases
                .contains(&mant_protocol::EvidenceBasis::Literal)
    }));
    assert!("--help".parse::<mant_protocol::ContentSelector>().is_err());
    let full = mant_render::render_explanation_text(&result);
    for text in [
        "undocumented",
        "joined",
        "separate",
        "should not consist solely of inverted",
        "--help=warnings,^joined,^undocumented",
    ] {
        assert!(full.contains(text), "missing {text}");
    }
}

#[test]
fn help_classes_qualifiers_and_tail_examples_share_one_owner() {
    fn help(nodes: &[mant_protocol::OutlineNode]) -> Option<&mant_protocol::OutlineNode> {
        nodes.iter().find_map(|node| {
            if node.title().starts_with("--help") && !node.children().is_empty() {
                Some(node)
            } else {
                help(node.children())
            }
        })
    }
    let document = archlinux_manual("gcc");
    let query = common::query_for_document("gcc", document);
    let outline =
        mant_query::build_outline_projection(&query, mant_protocol::EntryProjection::All, None)
            .unwrap();
    let help = help(&outline.nodes).expect("help with classes");
    for qualifier in ["undocumented", "joined", "separate"] {
        assert!(
            common::find_outline_entry(help.children(), qualifier).is_some(),
            "missing {qualifier}"
        );
    }
    assert_eq!(help.children().len(), 9);
    let excerpt =
        mant_query::select_excerpt(&query, &[mant_protocol::ContentSelector::path(help.path())])
            .unwrap();
    let text = mant_render::render_excerpt_text(&excerpt);
    for retained in [
        "These are the supported qualifiers",
        "should not consist solely of inverted",
        "--help=target,undocumented",
        "--help=warnings,^joined,^undocumented",
        "diff /tmp/O2-opts /tmp/O3-opts | grep enabled",
    ] {
        assert!(text.contains(retained), "missing {retained}: {text}");
    }
    assert!(!text.contains("Display the version number and copyrights"));
    let [mant_protocol::ExcerptSelection::DocumentEntry { entry, .. }] = &excerpt.selections[..]
    else {
        panic!("help entry")
    };
    assert!(
        entry
            .entry_owner()
            .unwrap()
            .facts()
            .unwrap()
            .value_domain
            .is_none()
    );
}

/// Deeply nested OPTIONS hierarchy (20 sub-sections), >250 preformatted
/// blocks, C++ class-hierarchy examples, phantom-paragraph suppression,
/// and definition-list paragraph spacing.
#[test]
fn keeps_large_hierarchy_fonts_and_pod_displays_without_control_text() {
    let document = archlinux_manual("gcc");
    common::assert_section_topology("archlinux/gcc", document, GCC_SECTIONS);

    let options = common::section(document, "OPTIONS");
    assert_eq!(options.children.len(), 20);
    assert_eq!(options.children[0].heading.plain_text(), "Option Summary");
    assert_eq!(
        options.children[1].heading.plain_text(),
        "Options Controlling the Kind of Output"
    );
    assert!(
        options
            .children
            .iter()
            .any(|child| child.heading.plain_text() == "Options to Request or Suppress Warnings")
    );

    common::assert_gcc_synopsis_layout(document);

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
        suggest_final_methods.layout.spacing_before_lines,
        Some(1),
        "default man(7) paragraph distance must separate adjacent GCC options",
    );
}

/// No roff escapes or `0` / `4` dimension values leak into text.
#[test]
fn does_not_leak_roff_markup() {
    common::assert_document_has_no_source_markup("archlinux/gcc", archlinux_manual("gcc"));
}
