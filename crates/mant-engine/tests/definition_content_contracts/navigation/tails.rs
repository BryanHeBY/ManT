//! Terminal navigation cannot become a soft space or merge physical HEAD roots.
use super::*;
use fixtures::{Body, Case, Head, Navigation, source, specimen};
use neighbors::{assert_snapshot, owner_mut, projection, without_empty_links};

#[derive(Clone, Copy, Debug)]
enum Tail {
    Prose,
    NoBody,
    EmptyList,
}

fn specimen_tail(case: Case, tail: Tail, multiple: bool) -> ResolvedContent {
    let mut value = specimen(case, true);
    let prefix = item(&value).description[0].clone();
    let current = owner_mut(&mut value);
    current.description = match tail {
        Tail::Prose => vec![
            current.description[1].clone(),
            current.description[2].clone(),
            prefix,
        ],
        Tail::NoBody => vec![prefix],
        Tail::EmptyList => vec![
            Block::List {
                kind: ListKind::Plain,
                compact: true,
                items: vec![],
                layout: LayoutHint::default(),
                source: Some(source(20)),
            },
            prefix,
        ],
    };
    if multiple {
        let mut second = vec![text("SecondHead")];
        if case.head == Head::Hard {
            second.push(Inline::LineBreak {});
        }
        current.terms.push(second.into());
    }
    value
}

fn assert_mapping(
    value: &ResolvedContent,
    baseline: &ResolvedContent,
    options: MarkdownOptions,
    navigation: Navigation,
) {
    let artifact = render_addressable_markdown_with_options(value, options);
    let control = render_addressable_markdown_with_options(baseline, options);
    let nodes = artifact
        .nodes()
        .iter()
        .filter(|node| matches!(node.node(), MarkdownNode::DocumentEntry { .. }))
        .collect::<Vec<_>>();
    let before = control
        .nodes()
        .iter()
        .filter(|node| matches!(node.node(), MarkdownNode::DocumentEntry { .. }))
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1);
    assert_eq!(before.len(), 1);
    let MarkdownNode::DocumentEntry {
        owner: EntryOwner::Definition(mapped),
        path,
        source: span,
        names,
        ..
    } = nodes[0].node()
    else {
        panic!("source definition")
    };
    let MarkdownNode::DocumentEntry { path: expected, .. } = before[0].node() else {
        unreachable!()
    };
    assert!(std::ptr::eq(*mapped, item(value)));
    assert_eq!(path, expected);
    assert_eq!(*span, Some(source(10)));
    assert_eq!(*names, item(value).entry.as_ref().unwrap().names.as_slice());
    let bytes = &artifact.text()[nodes[0].range()];
    assert_eq!(bytes.matches(NAME).count(), 1);
    assert_eq!(bytes.matches(TAIL).count(), 0);
    for target in navigation.targets() {
        if let LinkTarget::External { uri } = target {
            assert_eq!(
                bytes.matches(&uri).count(),
                1,
                "terminal references belong to the same owner: {bytes}"
            );
        }
    }
}

fn assert_rows(
    imported: &ResolvedContent,
    case: Case,
    tail: Tail,
    multiple: bool,
    options: MarkdownOptions,
) {
    let document = imported.document.as_ref().unwrap();
    let blocks = &document.sections[0].blocks;
    let [Block::List { items, .. }, Block::Paragraph { children, .. }] = blocks.as_slice() else {
        panic!("one original item and independent Tail: {blocks:#?}")
    };
    assert_eq!(inline_plain_text(children), TAIL);
    assert_eq!(items.len(), 1);
    let paragraphs = items[0]
        .blocks
        .iter()
        .filter_map(|block| {
            if let Block::Paragraph { children, .. } = block {
                Some(inline_plain_text(children))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if matches!(tail, Tail::Prose) {
        let body = paragraphs
            .iter()
            .filter(|paragraph| paragraph.contains(BODY))
            .collect::<Vec<_>>();
        assert_eq!(body.len(), 1);
        assert!(
            body[0].ends_with(BODY),
            "a terminal empty link cannot introduce a soft space: {body:?}"
        );
    } else {
        assert!(!paragraphs.iter().any(|paragraph| paragraph.contains(BODY)));
        assert_eq!(
            items[0]
                .blocks
                .iter()
                .filter(|block| matches!(block, Block::List { .. }))
                .count(),
            0,
            "the empty source list is omitted"
        );
    }
    let plain = mant_render::render_query_man(imported);
    assert_eq!(plain.matches(NAME).count(), 1);
    assert_eq!(plain.matches("SecondHead").count(), usize::from(multiple));
    if case.navigation.has_anchors() && options.preserve_anchors {
        return;
    }
    if multiple {
        let start = plain.find(NAME).unwrap() + NAME.len();
        let end = plain.find("SecondHead").unwrap();
        assert_eq!(
            plain[start..end].matches('\n').count(),
            if case.head == Head::Hard { 2 } else { 1 },
            "independent HEAD roots cannot flatten: {plain}"
        );
    }
    let last = if matches!(tail, Tail::Prose) {
        BODY
    } else if multiple {
        "SecondHead"
    } else {
        NAME
    };
    let start = plain.find(last).unwrap() + last.len();
    let end = plain.find(TAIL).unwrap();
    assert_eq!(
        plain[start..end].matches('\n').count(),
        2,
        "navigation cannot add an EOF row: {plain}"
    );
}

fn assert_reencoding(
    actual: &str,
    baseline: &str,
    case: Case,
    tail: Tail,
    multiple: bool,
    options: MarkdownOptions,
) {
    let imported = projection(actual, baseline, case.navigation);
    assert_rows(&imported, case, tail, multiple, options);
    let control = mant_loader::load_markdown_text(baseline, None).unwrap();
    let encoded = render_markdown_with_options(&imported, options);
    let expected = render_markdown_with_options(&control, options);
    // The first import is real ordinary List IR. Re-export must retain the
    // empty External link through its complete final prose coding context.
    let second = projection(&encoded, &expected, case.navigation);
    assert_rows(&second, case, tail, multiple, options);
    if !options.preserve_anchors {
        // Preserved raw HTML destinations keep their existing literal policy
        // on readback; generated document IDs are not globally idempotent.
        // Both generations above still compare exact actual/baseline text.
        assert_eq!(
            mant_render::render_query_man(&second),
            mant_render::render_query_man(&imported),
            "re-export must not manufacture body spaces or hard rows: {encoded}"
        );
    }
}

#[test]
fn trailing_empty_references_keep_prose_and_independent_term_groups_on_two_readbacks() {
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
            for head in [Head::Word, Head::Hard] {
                for multiple in [false, true] {
                    for tail in [Tail::Prose, Tail::NoBody, Tail::EmptyList] {
                        for spacing in if matches!(tail, Tail::Prose) {
                            &[0, 1][..]
                        } else {
                            &[0][..]
                        } {
                            let case = Case {
                                navigation,
                                relation,
                                head,
                                spacing: *spacing,
                                body: Body::Paragraph,
                            };
                            let original = specimen_tail(case, tail, multiple);
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
                                assert_reencoding(
                                    &actual, &expected, case, tail, multiple, options,
                                );
                                assert_mapping(&restored, &baseline, options, navigation);
                                if matches!(tail, Tail::Prose) {
                                    addresses::assert_artifact(&restored, &baseline, options);
                                }
                            }
                            assert_eq!(restored, original);
                        }
                    }
                }
            }
        }
    }
}
