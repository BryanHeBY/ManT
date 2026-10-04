//! Public definition policy never changes accepted content or its owner paths.
//!
//! These are constructed IR contracts, independent of native reachability.
//! Exact native HEAD/BODY behavior is covered by `roff_lowering`'s separately
//! source-bound `definition_relations` fixture.

use mant_codec::encode::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options, render_markdown,
};
use mant_ir::*;
use mant_protocol::{
    ContentSelector, EvidenceClass, ExcerptSelection, ExplanationOptions, ExplanationQuery,
    QueryBundle, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};

const NAME: &str = "name中";

#[path = "definition_content_contracts/body_selection.rs"]
mod body_selection;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn content(
    relation: HeadBodyRelation,
    body_alignment: DefinitionBodyAlignment,
    literal: bool,
) -> ResolvedContent {
    let mut content = mant_loader::load_markdown_text("# Probe\n\n## OPTIONS\n", None).unwrap();
    let children = vec![
        Inline::Link {
            target: LinkTarget::External {
                uri: "https://ex.org".into(),
            },
            title: None,
            children: vec![text("BodyWord")],
        },
        Inline::line_break(),
        Inline::Code {
            value: "TailWord".into(),
        },
    ];
    let body = if literal {
        Block::Preformatted {
            inline_layout: InlineLayout {
                row_hints: vec![RowLayoutHint {
                    row: 1,
                    indent_columns: 3,
                }],
            },
            children,
            language: None,
            layout: LayoutHint::default(),
            source: None,
        }
    } else {
        Block::Paragraph {
            inline_layout: InlineLayout {
                row_hints: vec![RowLayoutHint {
                    row: 1,
                    indent_columns: 3,
                }],
            },
            children,
            layout: LayoutHint::default(),
            source: None,
        }
    };
    let occurrence = EntryForm {
        parts: vec![EntryContentSlice {
            root: EntryInlineRoot::Term { index: 0 },
            path: vec![0, 0],
            bytes: Some(0..NAME.len()),
        }],
    };
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
        items: vec![DefinitionItem {
            head_body_relation: relation,
            terms: (vec![vec![Inline::Strong {
                children: vec![text(NAME)],
            }]])
            .into_iter()
            .map(Into::into)
            .collect(),
            description: vec![body],
            source: None,
            layout: DefinitionLayout {
                body_alignment,
                body_indent_columns: 10,
                min_term_gap_columns: 2,
                spacing_before_lines: None,
            },
            entry: Some(EntryFacts {
                id: "named-entry".into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: vec![NAME.into()],
                forms: vec![EntryForm::term(0)],
                name_bindings: vec![EntryNameBinding {
                    name: 0,
                    occurrences: vec![occurrence],
                    evidence: EntryNameEvidence::Declared,
                }],
                alias_groups: vec![],
                alias_of: None,
                value_domain: None,
            }),
        }],
    }];
    content
}

fn item(content: &ResolvedContent) -> &DefinitionItem {
    let Block::DefinitionList { items, .. } =
        &content.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("definition owner")
    };
    &items[0]
}

fn round_trip(content: &ResolvedContent) -> ResolvedContent {
    let wire = serde_json::to_string(&QueryBundle::from(content)).unwrap();
    let restored: ResolvedContent = serde_json::from_str::<QueryBundle>(&wire).unwrap().into();
    assert_eq!(&restored, content);
    assert_eq!(
        validate_document(restored.document.as_ref().unwrap()).len(),
        0
    );
    restored
}

fn assert_owner_projections(content: &ResolvedContent) -> String {
    let item = item(content);
    let owner = EntryOwner::Definition(item);
    let facts = owner.facts().unwrap();
    assert_eq!(
        inline_plain_text(&owner.form(&facts.name_bindings[0].occurrences[0]).unwrap()),
        NAME
    );
    let index = SemanticIndex::build(content.document.as_ref().unwrap());
    assert_eq!(index.section("options")[0].names, [NAME]);
    assert_eq!(index.section("options")[0].forms, [NAME]);
    let explanation = mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: NAME.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let evidence = explanation
        .evidence
        .iter()
        .find(|e| e.class == EvidenceClass::DirectEntry)
        .unwrap();
    let excerpt =
        mant_query::select_excerpt(content, &[ContentSelector::path(evidence.outline.path())])
            .unwrap();
    let [ExcerptSelection::DocumentEntry { entry, .. }] = excerpt.selections.as_slice() else {
        panic!("one original owner")
    };
    let Some(EntryOwner::Definition(selected)) = entry.entry_owner() else {
        panic!("original definition shape")
    };
    assert_eq!(selected, item);
    assert!(mant_render::render_excerpt_text(&excerpt).contains("BodyWord"));
    evidence.outline.path().into()
}

fn assert_artifact_search(content: &ResolvedContent, owner_path: &str) {
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    let entries = artifact
        .nodes()
        .iter()
        .filter(|mapped| {
            let MarkdownNode::DocumentEntry {
                path,
                owner: EntryOwner::Definition(owner),
                names,
                ..
            } = mapped.node()
            else {
                return false;
            };
            assert_eq!(*owner, item(content));
            assert_eq!(
                *names,
                item(content).entry.as_ref().unwrap().names.as_slice()
            );
            assert_eq!(path.to_string(), owner_path);
            let bytes = &artifact.text()[mapped.range()];
            for word in [NAME, "BodyWord", "TailWord"] {
                assert_eq!(bytes.matches(word).count(), 1);
            }
            true
        })
        .count();
    assert_eq!(entries, 1);
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        for word in [NAME, "BodyWord", "TailWord"] {
            let response = mant_query::search_query(
                content,
                &SearchQuery {
                    pattern: word.into(),
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Sensitive,
                    scope,
                    word: false,
                    context_lines: 0,
                    limit: 100,
                    offset: 0,
                },
            )
            .unwrap();
            let [hit] = response.matches.as_slice() else {
                panic!("one hit {scope:?}: {word}")
            };
            assert_eq!(hit.outline.path(), owner_path);
            let [occurrence] = hit.occurrences.as_slice() else {
                panic!("one occurrence")
            };
            let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                ..usize::try_from(occurrence.markdown.end_byte).unwrap();
            assert_eq!(&artifact.text()[range], word);
        }
    }
}

#[test]
fn shared_policies_preserve_json_entry_bindings_explanation_and_artifact_ranges() {
    for alignment in [
        DefinitionBodyAlignment::AfterTerm,
        DefinitionBodyAlignment::Indented,
    ] {
        for relation in [HeadBodyRelation::joined(), HeadBodyRelation::separated()] {
            for literal in [false, true] {
                let content = round_trip(&content(relation, alignment, literal));
                let before = content.clone();
                let path = assert_owner_projections(&content);
                assert_artifact_search(&content, &path);
                assert_eq!(
                    content, before,
                    "read projections cannot alter glyph owners"
                );
            }
        }
    }
}

#[test]
fn markdown_word_seams_and_fenced_payload_keep_one_item_without_inventing_hard_rows() {
    for alignment in [
        DefinitionBodyAlignment::AfterTerm,
        DefinitionBodyAlignment::Indented,
    ] {
        for relation in [HeadBodyRelation::joined(), HeadBodyRelation::separated()] {
            for literal in [false, true] {
                let content = content(relation, alignment, literal);
                let markdown = render_markdown(&content);
                if !literal {
                    let seam = if relation.joins_without_separator() {
                        ""
                    } else {
                        " "
                    };
                    assert!(
                        markdown.contains(&format!("**{NAME}**{seam}[BodyWord](https://ex.org)")),
                        "{markdown}"
                    );
                }
                let imported = mant_codec::parse_markdown(&markdown, None)
                    .unwrap()
                    .document;
                let Block::List { items, .. } = &imported.sections[0].blocks[0] else {
                    panic!("public Markdown projects definitions as an ordinary list: {markdown}")
                };
                assert_eq!(items.len(), 1, "a fence does not split the content owner");
                if literal {
                    let blocks = items[0]
                        .blocks
                        .iter()
                        .filter_map(|block| match block {
                            Block::Preformatted { children, .. } => {
                                Some(inline_plain_text(children))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(blocks, ["BodyWord\n   TailWord"]);
                    // Fence framing needs formatting rows, and CommonMark
                    // cannot retain live links/styles inside its code payload.
                    // The source IR still owns those exact children and hard rows.
                    assert!(matches!(
                        item(&content).description[0],
                        Block::Preformatted { .. }
                    ));
                }
            }
        }
    }
}
