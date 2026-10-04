//! Legal IR prefixes are transparent only when they own no physical boundary.
use super::{NAME, content, round_trip, text};
use mant_codec::encode::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options, render_markdown,
};
use mant_ir::*;
use mant_protocol::{QueryBundle, SearchCase, SearchQuery, SearchScope, SearchSyntax};

fn prose(nodes: Vec<Inline>) -> Block {
    Block::Paragraph {
        children: nodes,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn literal(nodes: Vec<Inline>) -> Block {
    Block::Preformatted {
        children: nodes,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
        language: None,
    }
}

fn prefixes() -> Vec<(&'static str, Vec<Block>)> {
    vec![
        ("empty-prose", vec![prose(vec![])]),
        ("empty-text", vec![prose(vec![text("")])]),
        (
            "navigation",
            vec![prose(vec![Inline::anchor_with_aliases(
                "body-prefix",
                vec!["prefix.alias".into()],
            )])],
        ),
        (
            "zero-space",
            vec![Block::VerticalSpace {
                lines: 0,
                source: None,
            }],
        ),
        ("no-literal-row", vec![literal(vec![])]),
        (
            "literal-navigation",
            vec![literal(vec![Inline::anchor("literal-prefix")])],
        ),
        (
            "empty-style",
            vec![prose(vec![Inline::Strong {
                children: vec![Inline::Emphasis {
                    children: vec![text("")],
                }],
            }])],
        ),
        (
            "mixed",
            vec![
                prose(vec![text("")]),
                prose(vec![Inline::anchor("mixed-prefix")]),
                Block::VerticalSpace {
                    lines: 0,
                    source: None,
                },
                literal(vec![]),
            ],
        ),
    ]
}

fn wrap(block: Block, context: &str) -> Block {
    let table = |block| Block::Table {
        rows: vec![TableRow {
            kind: TableRowKind::Data,
            cells: vec![TableCell {
                blocks: vec![block],
                kind: TableCellKind::Text,
                break_after: false,
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        column_preferences: ColumnPreferences {
            widths: vec![80],
            ..Default::default()
        },
        layout: LayoutHint::default(),
        source: None,
    };
    match context {
        "root" => block,
        "list" => Block::List {
            kind: ListKind::Plain,
            compact: true,
            items: vec![ListItem {
                blocks: vec![block],
                layout: ListItemLayout::default(),
                entry: None,
                source: None,
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        "table" => table(block),
        "nested-table" => table(table(block)),
        _ => unreachable!(),
    }
}

fn specimen(
    relation: HeadBodyRelation,
    prefix: &[Block],
    multiple: bool,
    context: &str,
) -> ResolvedContent {
    let mut value = content(relation, DefinitionBodyAlignment::AfterTerm, false);
    let blocks = &mut value.document.as_mut().unwrap().sections[0].blocks;
    let Block::DefinitionList { items, .. } = &mut blocks[0] else {
        unreachable!()
    };
    let item = &mut items[0];
    if multiple {
        item.terms.insert(0, vec![text("FIRST")].into());
        let facts = item.entry.as_mut().unwrap();
        facts.forms = vec![EntryForm::term(1)];
        facts.name_bindings[0].occurrences[0].parts[0].root = EntryInlineRoot::Term { index: 1 };
    }
    item.description.splice(0..0, prefix.iter().cloned());
    item.description.push(prose(vec![text("LaterWord")]));
    let block = blocks.pop().unwrap();
    blocks.push(wrap(block, context));
    value
}

fn owner(value: &ResolvedContent) -> &DefinitionItem {
    struct Items<'a>(Vec<&'a DefinitionItem>);
    impl<'a> visit::Visit<'a> for Items<'a> {
        fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
            self.0.push(item);
        }
    }
    let mut items = Items(Vec::new());
    visit::Visit::visit_document(&mut items, value.document.as_ref().unwrap());
    assert_eq!(items.0.len(), 1);
    items.0[0]
}

fn assert_original_addresses(value: &ResolvedContent, body_index: usize) {
    let item = owner(value);
    let owner = EntryOwner::Definition(item);
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, [NAME]);
    assert_eq!(
        inline_plain_text(&owner.form(&facts.name_bindings[0].occurrences[0]).unwrap()),
        NAME
    );
    let body = EntryForm {
        parts: vec![EntryContentSlice {
            root: EntryInlineRoot::Block { index: body_index },
            path: vec![0, 0],
            bytes: Some(0.."BodyWord".len()),
        }],
    };
    assert_eq!(inline_plain_text(&owner.form(&body).unwrap()), "BodyWord");
    let artifact = render_addressable_markdown_with_options(value, MarkdownOptions::ADDRESSABLE);
    let entries = artifact.nodes().iter().filter(|node| matches!(node.node(), MarkdownNode::DocumentEntry { owner: EntryOwner::Definition(mapped), .. } if std::ptr::eq(*mapped, item))).collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    for word in [NAME, "BodyWord", "TailWord", "LaterWord"] {
        assert_eq!(artifact.text()[entries[0].range()].matches(word).count(), 1);
        for scope in [SearchScope::Visible, SearchScope::Markdown] {
            let search = mant_query::search_query(
                value,
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
            assert_eq!(search.matches.len(), 1, "{scope:?}/{word}");
            for occurrence in &search.matches[0].occurrences {
                let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                    ..usize::try_from(occurrence.markdown.end_byte).unwrap();
                assert_eq!(&artifact.text()[range], word);
            }
        }
    }
}

fn assert_reader(value: &ResolvedContent, baseline: &ResolvedContent, context: &str) {
    struct Links<'a>(Vec<&'a str>);
    impl<'a> visit::Visit<'a> for Links<'a> {
        fn visit_inline(&mut self, node: &'a Inline) {
            if let Inline::Link {
                target: LinkTarget::External { uri },
                ..
            } = node
            {
                self.0.push(uri.as_str());
            }
            visit::walk_inline(self, node);
        }
    }
    let actual = render_markdown(value);
    let expected = render_markdown(baseline);
    assert_eq!(actual, expected, "{context}");
    let parsed = mant_loader::load_markdown_text(&actual, None).unwrap();
    let reference = mant_loader::load_markdown_text(&expected, None).unwrap();
    assert_eq!(parsed.document, reference.document);
    let mut links = Links(Vec::new());
    visit::Visit::visit_document(&mut links, parsed.document.as_ref().unwrap());
    assert_eq!(
        links.0,
        if context.contains("table") {
            vec![]
        } else {
            vec!["https://ex.org"]
        }
    );
}

#[test]
fn transparent_prefixes_keep_every_consumer_seam_and_original_body_address() {
    for relation in [
        HeadBodyRelation::joined(),
        HeadBodyRelation::separated(),
        HeadBodyRelation::Separate,
    ] {
        for multiple in [false, true] {
            for context in ["root", "list", "table", "nested-table"] {
                let baseline = specimen(relation, &[], multiple, context);
                let expected = mant_render::render_query_man(&baseline);
                let seam = if relation == HeadBodyRelation::joined() {
                    format!("{NAME}BodyWord")
                } else if relation == HeadBodyRelation::separated() {
                    format!("{NAME}  BodyWord")
                } else {
                    format!("{NAME}\n          BodyWord")
                };
                assert!(expected.contains(&seam), "{context}: {expected}");
                assert!(expected.contains("\n          LaterWord"), "{expected}");
                for (label, prefix) in prefixes() {
                    let original = specimen(relation, &prefix, multiple, context);
                    let restored = round_trip(&original);
                    for value in [&original, &restored] {
                        assert_eq!(
                            mant_render::render_query_man(value),
                            expected,
                            "{context}/{label}/{relation:?}"
                        );
                        let ansi = mant_render::render_query_text_with(value, |_, text| {
                            format!("\x1b[1m{text}\x1b[0m")
                        });
                        assert_eq!(ansi.replace("\x1b[1m", "").replace("\x1b[0m", ""), expected);
                        assert_reader(value, &baseline, context);
                        assert_original_addresses(value, prefix.len());
                    }
                }
            }
        }
    }
}

#[test]
fn explicit_boundaries_and_authored_empty_literal_rows_do_not_skip_to_later_prose() {
    let mut spaced = prose(vec![Inline::anchor("spaced-prefix")]);
    let Block::Paragraph { layout, .. } = &mut spaced else {
        unreachable!()
    };
    layout.spacing_before_lines = 1;
    for (prefix, row_suffix) in [
        (
            Block::VerticalSpace {
                lines: 1,
                source: None,
            },
            "\n\n          BodyWord",
        ),
        (spaced, "\n\n          BodyWord"),
        (literal(vec![text("")]), "\n          BodyWord"),
        (prose(vec![Inline::line_break()]), "\n          BodyWord"),
    ] {
        for context in ["root", "list", "table", "nested-table"] {
            let value = specimen(
                HeadBodyRelation::joined(),
                std::slice::from_ref(&prefix),
                false,
                context,
            );
            let wire = serde_json::to_string(&QueryBundle::from(&value)).unwrap();
            let restored: ResolvedContent =
                serde_json::from_str::<QueryBundle>(&wire).unwrap().into();
            assert_eq!(restored, value);
            let plain = mant_render::render_query_man(&restored);
            assert!(
                plain.contains(&format!("{NAME}{row_suffix}")),
                "{context}: {plain}"
            );
            assert!(!plain.contains(&format!("{NAME}BodyWord")), "{plain}");
            let markdown = render_markdown(&restored);
            let reader = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let readback = mant_render::render_query_man(&reader);
            assert!(
                !readback.contains(&format!("{NAME}BodyWord")),
                "{markdown}\n{readback}"
            );
            assert_eq!(readback.matches("BodyWord").count(), 1);
            assert_eq!(readback.matches("LaterWord").count(), 1);
        }
    }
}

#[test]
fn omitted_structural_body_roots_keep_later_executed_spacing_on_readback() {
    for relation in [
        HeadBodyRelation::joined(),
        HeadBodyRelation::separated(),
        HeadBodyRelation::Separate,
    ] {
        for block_spacing in [false, true] {
            for rows in [0, 1, 2] {
                for context in ["root", "list", "table", "nested-table"] {
                    let mut value = specimen(relation, &[], false, "root");
                    let Block::DefinitionList { items, .. } =
                        &mut value.document.as_mut().unwrap().sections[0].blocks[0]
                    else {
                        unreachable!()
                    };
                    let item = &mut items[0];
                    let empty = Block::List {
                        kind: ListKind::Plain,
                        compact: true,
                        items: vec![],
                        layout: LayoutHint::default(),
                        source: None,
                    };
                    if block_spacing {
                        let Block::Paragraph { layout, .. } = &mut item.description[0] else {
                            unreachable!()
                        };
                        layout.spacing_before_lines = rows;
                        item.description.insert(0, empty);
                    } else {
                        item.description.splice(
                            0..0,
                            [
                                empty,
                                Block::VerticalSpace {
                                    lines: rows,
                                    source: None,
                                },
                            ],
                        );
                    }
                    // An export may omit the empty structure, but cannot use
                    // that simplification to pick a later shared BODY root.
                    assert!(item.shared_description().is_none());
                    let blocks = &mut value.document.as_mut().unwrap().sections[0].blocks;
                    let block = blocks.pop().unwrap();
                    blocks.push(wrap(block, context));
                    for value in [&value, &round_trip(&value)] {
                        let markdown = render_markdown(value);
                        let parsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
                        let readback = mant_render::render_query_man(&parsed);
                        let start = readback.find(NAME).unwrap() + NAME.len();
                        let end = readback.find("BodyWord").unwrap();
                        let breaks = readback[start..end].matches('\n').count();
                        // Prose syntax simplifies positive distance to one
                        // blank row; fenced table text retains literal rows.
                        // A zero gap remains a separate hard row in either.
                        assert_eq!(
                            breaks,
                            if context.contains("table") {
                                usize::from(rows) + 1
                            } else if rows == 0 {
                                1
                            } else {
                                2
                            },
                            "{context}/{relation:?}/{block_spacing}/{rows}:\n{markdown}\n{readback}"
                        );
                        for word in [NAME, "BodyWord", "TailWord", "LaterWord"] {
                            assert_eq!(readback.matches(word).count(), 1);
                        }
                    }
                }
            }
        }
    }
}
