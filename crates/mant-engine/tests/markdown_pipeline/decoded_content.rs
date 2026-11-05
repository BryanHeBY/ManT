//! Entity-admitted scalars survive real Markdown transport and queries.
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, ResolvedContent};
use mant_protocol::{QueryBundle, SearchCase, SearchQuery, SearchScope, SearchSyntax};

#[path = "../support/decoded_content.rs"]
mod decoded;
use super::link_evidence;

fn wire(content: &ResolvedContent) -> ResolvedContent {
    let text = serde_json::to_string(&QueryBundle::from(content)).unwrap();
    let restored = ResolvedContent::from(serde_json::from_str::<QueryBundle>(&text).unwrap());
    assert_eq!(&restored, content);
    restored
}

fn original_content(case: &decoded::Case, context: usize) -> ResolvedContent {
    let source = format!(
        "# Probe\n\n## Payload\n\n{}\n",
        if context == 2 {
            format!("- {}", case.source)
        } else {
            case.source.into()
        }
    );
    let mut original = mant_loader::load_markdown_text(&source, None).unwrap();
    case.assert_first_diagnostic(&original);
    if context == 1 {
        let section = &mut original.document.as_mut().unwrap().sections[0];
        let Block::Paragraph { children, .. } = section.blocks.remove(0) else {
            panic!("parsed source link")
        };
        section.heading.content = children;
    }
    if context == 3 {
        let section = &mut original.document.as_mut().unwrap().sections[0];
        let description = std::mem::take(&mut section.blocks);
        section.blocks = vec![Block::DefinitionList {
            items: vec![mant_ir::DefinitionItem {
                terms: vec![
                    vec![mant_ir::Inline::Text {
                        value: "HEAD".into(),
                    }]
                    .into(),
                ],
                description,
                source: None,
                entry: None,
                head_body_relation: mant_ir::HeadBodyRelation::Separate,
                layout: mant_ir::DefinitionLayout::default(),
            }],
            declaration_groups: vec![],
            compact: true,
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }];
    }
    original
}

fn link_owner(content: &ResolvedContent) -> ResolvedContent {
    use mant_ir::{Inline, visit::Visit};
    struct LinkOwner(Vec<Inline>);
    impl<'a> Visit<'a> for LinkOwner {
        fn visit_inline(&mut self, node: &'a Inline) {
            if matches!(node, Inline::Link { .. }) {
                self.0.push(node.clone());
            } else {
                mant_ir::visit::walk_inline(self, node);
            }
        }
    }
    let mut owner = LinkOwner(Vec::new());
    owner.visit_document(content.document.as_ref().unwrap());
    assert_eq!(owner.0.len(), 1);
    let mut focused = content.clone();
    let document = focused.document.as_mut().unwrap();
    document.heading = None;
    document.sections.clear();
    document.blocks = vec![Block::Paragraph {
        children: owner.0,
        inline_layout: mant_ir::InlineLayout::default(),
        layout: mant_ir::LayoutHint::default(),
        source: None,
    }];
    focused
}

fn assert_separate_boundary(content: &ResolvedContent, label: &str) {
    let [Block::List { items, .. }] = content.document.as_ref().unwrap().sections[0]
        .blocks
        .as_slice()
    else {
        panic!("Separate definition projects to one portable list");
    };
    assert_eq!(items.len(), 1);
    let [Block::Paragraph { children, .. }] = items[0].blocks.as_slice() else {
        panic!("the shared portable paragraph retains the HEAD/BODY hard boundary");
    };
    assert_eq!(
        mant_ir::inline_plain_text(children),
        format!("HEAD\n{label}")
    );
}

fn witness_owner(content: &ResolvedContent, variant: usize) -> ResolvedContent {
    if variant == 3 {
        // Separate becomes a hard line inside a portable list. Its added
        // structural LF changes global flattened offsets, not the original
        // Link's local scalar/code/target/title/occurrence ownership.
        link_owner(content)
    } else {
        content.clone()
    }
}

#[test]
fn decoded_entities_keep_original_link_scalars_diagnostics_and_search_ranges() {
    for case in decoded::CASES {
        for context in 0..4 {
            let original = original_content(case, context);
            case.assert_content(&original);
            let evidence = link_evidence::Meaning::capture(&witness_owner(&original, context));
            assert_eq!(
                evidence.target().to_uri().as_deref(),
                Some("https://a.test")
            );
            for semantics in [false, true] {
                let mut content = wire(&original);
                for round in 1..=2 {
                    let markdown = render_markdown_with_options(
                        &content,
                        MarkdownOptions {
                            preserve_semantics: semantics,
                            preserve_anchors: false,
                        },
                    );
                    assert!(
                        !markdown
                            .chars()
                            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
                        "{}: no unsafe emitted control",
                        case.id
                    );
                    content = wire(&mant_loader::load_markdown_text(&markdown, None).unwrap());
                    case.assert_content(&content);
                    if context == 3 {
                        assert_separate_boundary(&content, case.label);
                    }
                    evidence.assert_preserved(
                        &witness_owner(&content, context),
                        &format!("{}/{context}/{semantics}/{round}", case.id),
                    );
                    assert!(
                        !content
                            .document
                            .as_ref()
                            .unwrap()
                            .diagnostics
                            .iter()
                            .any(|d| d.code.as_deref() == Some("markdown.control-characters")),
                        "{}: normalization must not be delayed",
                        case.id
                    );
                    let query = SearchQuery {
                        pattern: case.label.into(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Sensitive,
                        scope: SearchScope::Visible,
                        word: false,
                        context_lines: 0,
                        limit: 20,
                        offset: 0,
                    };
                    let found = mant_query::search_query(&content, &query).unwrap();
                    assert_eq!(found.total, 1, "{}: scalar search", case.id);
                    for hit in found.matches.iter().flat_map(|hit| &hit.occurrences) {
                        assert_eq!(hit.matched_text, case.label);
                    }
                }
            }
        }
    }
}
