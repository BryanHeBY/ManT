//! Zero-scalar navigation stays between physical blocks or at their real EOF.
use super::*;
use fixtures::{Body, Case, Head, Navigation, source, specimen};

#[derive(Clone, Copy, Debug)]
enum Sequence {
    Literals,
    Prose,
    Trailing,
    Definition,
    List,
}

pub(super) fn owner_mut(value: &mut ResolvedContent) -> &mut DefinitionItem {
    let Block::DefinitionList { items, .. } =
        &mut value.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        unreachable!()
    };
    &mut items[0]
}

fn literal_a() -> Block {
    Block::Preformatted {
        children: vec![text("LITERAL_A")],
        inline_layout: InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: Some(source(20)),
    }
}

fn list(blocks: Vec<Block>) -> Block {
    Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            blocks,
            layout: ListItemLayout::default(),
            source: None,
            entry: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn nested_definition(body: Block, relation: HeadBodyRelation) -> Block {
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![vec![text("InnerWord")].into()],
            description: vec![body],
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

fn value(
    navigation: Navigation,
    relation: HeadBodyRelation,
    sequence: Sequence,
) -> ResolvedContent {
    let case = Case {
        navigation,
        relation,
        body: Body::Literal,
        head: Head::Word,
        spacing: 0,
    };
    let mut value = specimen(case, true);
    let prefix = item(&value).description[0].clone();
    let literal_b = item(&value).description[2].clone();
    let prose_fixture = specimen(
        Case {
            body: Body::Paragraph,
            ..case
        },
        false,
    );
    // Adjacent retains a raw-anchor prefix even in the no-link fixture.
    // The actual BODY remains the final block, after its explicit spacing.
    let prose = item(&prose_fixture).description.last().unwrap().clone();
    assert!(matches!(&prose, Block::Paragraph { children, .. }
        if inline_plain_text(children) == BODY));
    owner_mut(&mut value).description = match sequence {
        Sequence::Literals => vec![literal_a(), prefix, literal_b],
        Sequence::Prose => vec![literal_a(), prefix, prose],
        Sequence::Trailing => vec![literal_b, prefix],
        Sequence::Definition => vec![literal_a(), prefix, nested_definition(prose, relation)],
        Sequence::List => vec![literal_a(), list(vec![prefix, prose])],
    };
    value
}

pub(super) fn without_empty_links(value: &ResolvedContent) -> ResolvedContent {
    struct Remove;
    impl visit::VisitMut for Remove {
        fn visit_inline_mut(&mut self, node: &mut Inline) {
            if matches!(node, Inline::Link { children, .. } if children.is_empty()) {
                // Retire only navigation, leaving owner arrays, positive gaps,
                // raw anchors and every original body at their exact address.
                *node = Inline::Strong { children: vec![] };
            } else {
                visit::walk_inline_mut(self, node);
            }
        }
    }
    let mut baseline = value.clone();
    visit::VisitMut::visit_document_mut(&mut Remove, baseline.document.as_mut().unwrap());
    baseline
}

pub(super) fn projection(actual: &str, baseline: &str, navigation: Navigation) -> ResolvedContent {
    let imported = mant_loader::load_markdown_text(actual, None).unwrap();
    let expected = mant_loader::load_markdown_text(baseline, None).unwrap();
    let actual_blocks = &imported.document.as_ref().unwrap().sections[0].blocks;
    let expected_blocks = &expected.document.as_ref().unwrap().sections[0].blocks;
    assert_eq!(
        readback::shape(actual_blocks),
        readback::shape(expected_blocks),
        "actual:\n{actual}\nbaseline:\n{baseline}"
    );
    assert_eq!(
        mant_render::render_query_man(&imported),
        mant_render::render_query_man(&expected),
        "actual:\n{actual}\nbaseline:\n{baseline}"
    );
    let targets = navigation
        .targets()
        .into_iter()
        .filter(|target| matches!(target, LinkTarget::External { .. }))
        .map(|target| (target, String::new()))
        .collect::<Vec<_>>();
    assert_eq!(readback::links(&imported), targets, "{actual}");
    for (target, label) in targets {
        assert_eq!(label, "");
        let LinkTarget::External { uri } = target else {
            unreachable!()
        };
        assert_eq!(actual.matches(&uri).count(), 1, "{actual}");
    }
    imported
}

fn link_positions(
    value: &ResolvedContent,
) -> Vec<(ContentLocation, LinkTarget, Option<SourceSpan>)> {
    let document = value.document.as_ref().unwrap();
    let mut positions = vec![];
    let report = scan_references(document, ReferenceScanLimits::default(), |link| {
        let position = link.location.to_owned().unwrap();
        assert!(std::ptr::eq(
            position.resolve_link(document).unwrap(),
            link.link
        ));
        assert_eq!(inline_plain_text(link.label), "");
        assert_eq!(link.source, Some(source(12)));
        let Some(ReferenceOwnerRef {
            owner: EntryOwner::Definition(mapped),
            ..
        }) = link.semantic_owner
        else {
            panic!("original semantic owner")
        };
        assert!(std::ptr::eq(mapped, item(value)));
        positions.push((position, link.target.clone(), link.source));
        std::ops::ControlFlow::Continue(())
    });
    assert!(report.complete());
    positions
}

pub(super) fn assert_snapshot(
    original: &ResolvedContent,
    restored: &ResolvedContent,
    navigation: Navigation,
) {
    assert_eq!(restored, original);
    assert_eq!(link_positions(restored), link_positions(original));
    let targets = link_positions(restored)
        .into_iter()
        .map(|(_, target, _)| target)
        .collect::<Vec<_>>();
    assert_eq!(targets, navigation.targets());
    let owner = EntryOwner::Definition(item(restored));
    assert_eq!(
        owner.validated_names().unwrap(),
        item(original).entry.as_ref().unwrap().names.as_slice()
    );
}

fn assert_types(imported: &ResolvedContent, sequence: Sequence) {
    struct Types {
        literals: Vec<(String, Option<String>)>,
        lists: usize,
    }
    impl<'ir> visit::Visit<'ir> for Types {
        fn visit_block(&mut self, block: &'ir Block) {
            match block {
                Block::Preformatted {
                    children, language, ..
                } => self
                    .literals
                    .push((inline_plain_text(children), language.clone())),
                Block::List { .. } => self.lists += 1,
                _ => {}
            }
            visit::walk_block(self, block);
        }
    }
    let mut types = Types {
        literals: vec![],
        lists: 0,
    };
    visit::Visit::visit_document(&mut types, imported.document.as_ref().unwrap());
    let a = ("LITERAL_A".into(), None);
    let b = ("Body中\nLiteralTail".into(), Some("txt".into()));
    let expected = match sequence {
        Sequence::Literals => vec![a, b],
        Sequence::Trailing => vec![b],
        Sequence::Prose | Sequence::Definition | Sequence::List => vec![a],
    };
    assert_eq!(types.literals, expected);
    assert_eq!(
        types.lists,
        if matches!(sequence, Sequence::Definition | Sequence::List) {
            2
        } else {
            1
        }
    );
}

fn assert_rows(
    imported: &ResolvedContent,
    sequence: Sequence,
    navigation: Navigation,
    options: MarkdownOptions,
) {
    let plain = mant_render::render_query_man(imported);
    assert_eq!(plain.matches(BODY).count(), 1);
    assert_eq!(plain.matches(TAIL).count(), 1);
    if navigation.has_anchors() && options.preserve_anchors {
        // Retained anchor syntax has its frozen literal readback policy. The
        // complete shape/plain comparison above retains it without filtering.
        return;
    }
    let (before, after) = match sequence {
        Sequence::Trailing => ("LiteralTail", TAIL),
        Sequence::Definition => ("LITERAL_A", "InnerWord"),
        Sequence::Literals | Sequence::Prose | Sequence::List => ("LITERAL_A", BODY),
    };
    let start = plain.find(before).unwrap() + before.len();
    let end = plain.find(after).unwrap();
    assert_eq!(
        plain[start..end].matches('\n').count(),
        // These BODY neighbors have no positive gap. A closed fence needs
        // one syntax newline; only the outer list's scope exit needs two.
        if matches!(sequence, Sequence::Trailing) {
            2
        } else {
            1
        },
        "{sequence:?}: {plain}"
    );
}

#[test]
fn navigation_between_blocks_inside_lists_and_at_literal_eof_keeps_physical_boundaries() {
    for navigation in [
        Navigation::External,
        Navigation::Manual,
        Navigation::Multiple,
        Navigation::Adjacent,
    ] {
        for relation in [
            HeadBodyRelation::joined(),
            HeadBodyRelation::separated(),
            HeadBodyRelation::Separate,
        ] {
            for sequence in [
                Sequence::Literals,
                Sequence::Prose,
                Sequence::Trailing,
                Sequence::Definition,
                Sequence::List,
            ] {
                let original = value(navigation, relation, sequence);
                let restored = round_trip(&original);
                let baseline = round_trip(&without_empty_links(&original));
                assert_snapshot(&original, &restored, navigation);
                for preserve_anchors in [false, true] {
                    let options = MarkdownOptions {
                        preserve_anchors,
                        preserve_semantics: false,
                    };
                    let actual = render_markdown_with_options(&restored, options);
                    let expected = render_markdown_with_options(&baseline, options);
                    let imported = projection(&actual, &expected, navigation);
                    assert_types(&imported, sequence);
                    assert_rows(&imported, sequence, navigation, options);
                    addresses::assert_artifact(&restored, &baseline, options);
                }
                assert_eq!(restored, original);
            }
        }
    }
}
