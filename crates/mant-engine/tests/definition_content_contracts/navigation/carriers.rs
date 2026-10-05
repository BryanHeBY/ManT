//! Nested zero-body carriers do not acquire a list marker or physical row.
use super::*;
use fixtures::{Body, Head, Navigation, source};
use neighbors::{projection, without_empty_links};

const HEAD: &str = "CarrierHead";

#[derive(Clone, Copy, Debug)]
enum Carrier {
    Definition,
    List,
}

#[derive(Clone, Copy, Debug)]
enum Position {
    Prefix,
    Tail,
}

fn paragraph(children: Vec<Inline>, line: u32) -> Block {
    Block::Paragraph {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: Some(source(line)),
    }
}

fn list(block: Block) -> Block {
    Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            blocks: vec![block],
            layout: ListItemLayout::default(),
            source: None,
            entry: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn definition(head: Vec<Inline>, description: Vec<Block>, relation: HeadBodyRelation) -> Block {
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![head.into()],
            description,
            head_body_relation: relation,
            layout: DefinitionLayout::default(),
            source: None,
            entry: None,
        }],
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn carrier(kind: Carrier, relation: HeadBodyRelation) -> Block {
    let empty_reference = paragraph(
        vec![Inline::Link {
            target: LinkTarget::External {
                uri: EXTERNAL_A.into(),
            },
            title: None,
            children: vec![],
        }],
        12,
    );
    let child = definition(vec![], vec![empty_reference], relation);
    match kind {
        Carrier::Definition => child,
        Carrier::List => list(child),
    }
}

fn body(kind: Body) -> Block {
    match kind {
        Body::Paragraph => paragraph(vec![text(BODY)], 20),
        Body::Literal => Block::Preformatted {
            children: vec![text("Body中\nLiteralTail")],
            inline_layout: InlineLayout::default(),
            language: Some("txt".into()),
            layout: LayoutHint::default(),
            source: Some(source(20)),
        },
        Body::List => list(paragraph(vec![text(BODY)], 20)),
        _ => unreachable!("this matrix uses prose, literal and ordinary list"),
    }
}

fn value(
    kind: Carrier,
    position: Position,
    head: Head,
    relation: HeadBodyRelation,
    footer: Body,
) -> ResolvedContent {
    let mut value = mant_loader::load_markdown_text("# Probe\n\n## OPTIONS\n", None).unwrap();
    let zero = carrier(kind, relation);
    let physical = body(footer);
    let description = match position {
        Position::Prefix => vec![zero, physical],
        Position::Tail => vec![physical, zero],
    };
    let head = if head.has_word() {
        vec![text(HEAD)]
    } else {
        vec![]
    };
    value.document.as_mut().unwrap().sections[0].blocks = vec![
        definition(head, description, relation),
        paragraph(vec![text(TAIL)], 30),
    ];
    value
}

fn link_addresses(value: &ResolvedContent) -> Vec<(ContentLocation, LinkTarget)> {
    let document = value.document.as_ref().unwrap();
    let mut links = vec![];
    let report = scan_references(document, ReferenceScanLimits::default(), |link| {
        let address = link.location.to_owned().unwrap();
        assert!(std::ptr::eq(
            address.resolve_link(document).unwrap(),
            link.link
        ));
        assert_eq!(inline_plain_text(link.label), "");
        assert_eq!(link.source, Some(source(12)));
        links.push((address, link.target.clone()));
        std::ops::ControlFlow::Continue(())
    });
    assert!(report.complete());
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].1, Navigation::External.targets()[0]);
    links
}

fn assert_types(imported: &ResolvedContent, footer: Body, outer: bool) {
    struct Types {
        literals: Vec<(String, Option<String>)>,
        lists: usize,
        prose: Vec<String>,
    }
    impl<'ir> visit::Visit<'ir> for Types {
        fn visit_block(&mut self, block: &'ir Block) {
            match block {
                Block::List { .. } => self.lists += 1,
                Block::Preformatted {
                    children, language, ..
                } => {
                    self.literals
                        .push((inline_plain_text(children), language.clone()));
                }
                Block::Paragraph { children, .. } => {
                    let text = inline_plain_text(children);
                    if text.contains(BODY) {
                        self.prose.push(text);
                    }
                }
                _ => {}
            }
            visit::walk_block(self, block);
        }
    }
    let mut types = Types {
        literals: vec![],
        lists: 0,
        prose: vec![],
    };
    visit::Visit::visit_document(&mut types, imported.document.as_ref().unwrap());
    let expected = if footer == Body::Literal {
        vec![("Body中\nLiteralTail".to_owned(), Some("txt".to_owned()))]
    } else {
        vec![]
    };
    assert_eq!(
        types.literals, expected,
        "the actual footer stays a complete typed fence"
    );
    assert_eq!(
        types.lists,
        usize::from(outer) + usize::from(footer == Body::List),
        "only the physical outer item and real list BODY own markers"
    );
    assert_eq!(types.prose.len(), usize::from(footer != Body::Literal));
    if let Some(prose) = types.prose.first() {
        assert!(prose.ends_with(BODY), "{prose}");
    }
    let blocks = &imported.document.as_ref().unwrap().sections[0].blocks;
    assert!(
        matches!(blocks.last(), Some(Block::Paragraph { children, .. })
        if inline_plain_text(children) == TAIL)
    );
}

fn assert_rows(
    imported: &ResolvedContent,
    footer: Body,
    head: Head,
    relation: HeadBodyRelation,
    position: Position,
) {
    let plain = mant_render::render_query_man(imported);
    assert_eq!(plain.matches(HEAD).count(), usize::from(head.has_word()));
    assert_eq!(plain.matches(BODY).count(), 1, "{plain}");
    assert_eq!(plain.matches(TAIL).count(), 1);
    if head.has_word() && footer == Body::Paragraph {
        let start = plain.find(HEAD).unwrap() + HEAD.len();
        let end = plain.find(BODY).unwrap();
        assert_eq!(
            plain[start..end].matches('\n').count(),
            usize::from(
                matches!(position, Position::Prefix) || relation == HeadBodyRelation::Separate
            ),
            "{plain}"
        );
    }
    let last = if footer == Body::Literal {
        "LiteralTail"
    } else {
        BODY
    };
    let start = plain.find(last).unwrap() + last.len();
    let end = plain.find(TAIL).unwrap();
    assert_eq!(
        plain[start..end].matches('\n').count(),
        2,
        "no carrier row appears at EOF: {plain}"
    );
    assert_types(imported, footer, true);
}

fn assert_start(
    value: &ResolvedContent,
    footer: Body,
    position: Position,
    relation: HeadBodyRelation,
) {
    let owner = item(value);
    let start = owner.description_start().unwrap();
    assert_eq!(start.block_index, 0);
    assert!(std::ptr::eq(start.block, &raw const owner.description[0]));
    if matches!(position, Position::Prefix) {
        // Selection stops at the nested structural carrier, even though its
        // Markdown body scalars are empty. Later prose at index 1 cannot
        // acquire a shared HEAD/BODY seam by skipping this public barrier.
        assert!(matches!(
            start.block,
            Block::List { .. } | Block::DefinitionList { .. }
        ));
        assert!(start.inline_content().is_none());
    }
    let shares = matches!(position, Position::Tail)
        && footer != Body::List
        && relation != HeadBodyRelation::Separate;
    assert_eq!(owner.shared_description().is_some(), shares);
}

#[test]
fn nested_navigation_only_carriers_preserve_their_real_sibling_footer() {
    for kind in [Carrier::Definition, Carrier::List] {
        for footer in [Body::Paragraph, Body::Literal, Body::List] {
            for head in [Head::Empty, Head::Word] {
                for relation in [
                    HeadBodyRelation::joined(),
                    HeadBodyRelation::separated(),
                    HeadBodyRelation::Separate,
                ] {
                    for position in [Position::Prefix, Position::Tail] {
                        let original = value(kind, position, head, relation, footer);
                        let restored = round_trip(&original);
                        let baseline = round_trip(&without_empty_links(&original));
                        assert_eq!(link_addresses(&restored), link_addresses(&original));
                        assert_start(&restored, footer, position, relation);
                        for preserve_anchors in [false, true] {
                            let options = MarkdownOptions {
                                preserve_anchors,
                                preserve_semantics: false,
                            };
                            let actual = render_markdown_with_options(&restored, options);
                            let control = render_markdown_with_options(&baseline, options);
                            let imported = projection(&actual, &control, Navigation::External);
                            assert_rows(&imported, footer, head, relation, position);
                        }
                        assert_eq!(restored, original);
                    }
                }
            }
        }
    }
}

#[test]
fn entirely_zero_outer_carrier_omits_its_marker_before_root_footer() {
    for kind in [Carrier::Definition, Carrier::List] {
        for footer in [Body::Paragraph, Body::Literal, Body::List] {
            let mut original = value(
                kind,
                Position::Prefix,
                Head::Empty,
                HeadBodyRelation::joined(),
                footer,
            );
            let Block::DefinitionList { items, .. } =
                &mut original.document.as_mut().unwrap().sections[0].blocks[0]
            else {
                unreachable!()
            };
            let physical = items[0].description.pop().unwrap();
            original.document.as_mut().unwrap().sections[0]
                .blocks
                .insert(1, physical);
            let restored = round_trip(&original);
            let baseline = round_trip(&without_empty_links(&original));
            assert_eq!(link_addresses(&restored), link_addresses(&original));
            for preserve_anchors in [false, true] {
                let options = MarkdownOptions {
                    preserve_anchors,
                    preserve_semantics: false,
                };
                let actual = render_markdown_with_options(&restored, options);
                let control = render_markdown_with_options(&baseline, options);
                let imported = projection(&actual, &control, Navigation::External);
                assert_types(&imported, footer, false);
                let plain = mant_render::render_query_man(&imported);
                assert_eq!(plain.matches(HEAD).count(), 0);
                assert_eq!(plain.matches(BODY).count(), 1);
                let last = if footer == Body::Literal {
                    "LiteralTail"
                } else {
                    BODY
                };
                let start = plain.find(last).unwrap() + last.len();
                let end = plain.find(TAIL).unwrap();
                // A fence closes its block without requesting source spacing.
                // Sibling paragraphs and exiting a list still need a blank
                // syntax line; the navigation-only carrier contributes none.
                let rows = if footer == Body::Literal { 1 } else { 2 };
                assert_eq!(plain[start..end].matches('\n').count(), rows, "{plain}");
            }
            assert_eq!(restored, original);
        }
    }
}
