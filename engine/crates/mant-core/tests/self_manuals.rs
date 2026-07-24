//! Keeps the shipped Markdown manuals inside the supported document subset.

use mant_ast::{
    Block, Inline, ListKind, OutlineDetail, OutlineNode, QueryBundle, QuerySchema, Section,
    SectionRole,
};
use mant_core::{build_outline_with_detail, parse_markdown, render_markdown, render_query_text};

const MANT_MANUAL: &str = include_str!("../../../../docs/manuals/mant.md");
const MANTUI_MANUAL: &str = include_str!("../../../../docs/manuals/mantui.md");

#[test]
fn shipped_manuals_parse_without_lossy_fallbacks() {
    for (name, source) in [("mant.md", MANT_MANUAL), ("mantui.md", MANTUI_MANUAL)] {
        let document = parse_markdown(source, Some(format!("docs/manuals/{name}")));

        assert_eq!(
            document.meta.title.as_deref(),
            Some(name.trim_end_matches(".md"))
        );
        assert!(document.blocks.is_empty(), "{name} starts with its title");
        assert!(
            !document.sections.is_empty(),
            "{name} has a navigable outline"
        );
        assert!(
            document.diagnostics.is_empty(),
            "{name} must not rely on unsupported Markdown: {:?}",
            document.diagnostics
        );
        assert!(
            has_quick_reference(&document.sections),
            "{name} keeps its embedded quick reference semantic"
        );
        assert!(
            has_tldr_examples(&document.sections),
            "{name} quick reference follows the tldr description/command layout"
        );

        let query = QueryBundle {
            schema: QuerySchema::V3,
            label: name.to_owned(),
            document: Some(document),
            tldr: None,
        };
        assert!(!render_markdown(&query).contains("<a "));
        assert!(!render_query_text(&query).is_empty());
    }
}

#[test]
fn shipped_manual_options_are_addressable_for_agents_and_the_tui() {
    for (name, source, expected) in [
        ("mant.md", MANT_MANUAL, "--search"),
        ("mantui.md", MANTUI_MANUAL, "--help"),
    ] {
        let query = QueryBundle {
            schema: QuerySchema::V3,
            label: name.to_owned(),
            document: Some(parse_markdown(source, Some(format!("docs/manuals/{name}")))),
            tldr: None,
        };
        let outline =
            build_outline_with_detail(&query, OutlineDetail::Options).expect("manual outline");

        assert!(
            contains_entry(&outline.nodes, expected),
            "{name} should expose {expected} as a semantic entry"
        );
    }
}

fn has_quick_reference(sections: &[Section]) -> bool {
    sections.iter().any(|section| {
        section.role == Some(SectionRole::QuickReference) || has_quick_reference(&section.children)
    })
}

fn has_tldr_examples(sections: &[Section]) -> bool {
    sections.iter().any(|section| {
        if section.role == Some(SectionRole::QuickReference) {
            return matches!(
                section.blocks.first(),
                Some(Block::List {
                    kind: ListKind::Plain,
                    compact: false,
                    items,
                    ..
                }) if items.len() >= 4 && items.iter().all(|item| {
                    matches!(
                        item.blocks.as_slice(),
                        [
                            Block::Paragraph { .. },
                            Block::Paragraph { children, layout, .. },
                        ] if matches!(children.as_slice(), [Inline::Code { .. }])
                            && layout.spacing_before_lines == 1
                    )
                })
            );
        }
        has_tldr_examples(&section.children)
    })
}

fn contains_entry(nodes: &[OutlineNode], name: &str) -> bool {
    nodes.iter().any(|node| {
        matches!(
            node,
            OutlineNode::DocumentEntry { names, .. }
                if names.iter().any(|candidate| candidate == name)
        ) || contains_entry(node.children(), name)
    })
}
