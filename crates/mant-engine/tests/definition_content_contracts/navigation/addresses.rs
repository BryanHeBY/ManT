use super::*;
use fixtures::{Case, source};

pub(super) fn assert_original(value: &ResolvedContent, baseline: &ResolvedContent, case: Case) {
    let document = value.document.as_ref().unwrap();
    let current = item(value);
    let previous = item(baseline);
    assert_eq!(current.entry, previous.entry);
    assert_eq!(current.terms, previous.terms);
    assert_eq!(current.source, Some(source(10)));
    let baseline_body = if case.navigation.has_anchors() { 2 } else { 1 };
    assert_eq!(
        &current.description[2..],
        &previous.description[baseline_body..]
    );
    assert_eq!(geometry::block_gap(&current.description[1]), case.spacing);
    let owner = EntryOwner::Definition(current);
    let names = if case.head.has_word() {
        vec![NAME.to_owned()]
    } else {
        vec![]
    };
    assert_eq!(owner.validated_names().unwrap(), names);
    if case.head.has_word() {
        let occurrence = &current.entry.as_ref().unwrap().name_bindings[0].occurrences[0];
        assert_eq!(inline_plain_text(&owner.form(occurrence).unwrap()), NAME);
    }
    let body_path = [
        ContentBlockStep::Block { index: 0 },
        ContentBlockStep::DefinitionItem { index: 0 },
        ContentBlockStep::Block { index: 2 },
    ];
    assert!(std::ptr::eq(
        resolve_content_block(&document.sections[0].blocks, &body_path).unwrap(),
        &raw const current.description[2]
    ));
    assert_eq!(
        geometry::block_source(&current.description[2]),
        Some(source(20))
    );
    assert_prefix_addresses(document, current, case);
}

fn assert_prefix_addresses(document: &Document, current: &DefinitionItem, case: Case) {
    let Block::Paragraph {
        children,
        inline_layout,
        ..
    } = &current.description[0]
    else {
        unreachable!()
    };
    let mut found = vec![];
    let report = scan_references(document, ReferenceScanLimits::default(), |link| {
        let position = link.location.to_owned().unwrap();
        assert!(std::ptr::eq(
            position.resolve_link(document).unwrap(),
            link.link
        ));
        let root = position.inline_content(document).unwrap();
        assert!(std::ptr::eq(root.content.as_ptr(), children.as_ptr()));
        assert!(std::ptr::eq(root.layout, inline_layout));
        assert_eq!(inline_plain_text(link.label), "");
        assert_eq!(link.source, Some(source(12)));
        let Some(ReferenceOwnerRef {
            owner: EntryOwner::Definition(mapped),
            ..
        }) = link.semantic_owner
        else {
            panic!("original semantic owner")
        };
        assert!(std::ptr::eq(mapped, current));
        found.push((position, link.target.clone()));
        std::ops::ControlFlow::Continue(())
    });
    assert!(report.complete());
    let expected = case
        .navigation
        .paths()
        .into_iter()
        .zip(case.navigation.targets())
        .map(|(path, target)| {
            (
                ContentLocation::Content {
                    sections: vec![0],
                    blocks: vec![
                        ContentBlockStep::Block { index: 0 },
                        ContentBlockStep::DefinitionItem { index: 0 },
                        ContentBlockStep::Block { index: 0 },
                    ],
                    root: ContentInlineRoot::Inlines,
                    path,
                },
                target,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(found, expected);
}

pub(super) fn assert_artifact(
    value: &ResolvedContent,
    baseline: &ResolvedContent,
    options: MarkdownOptions,
) {
    let artifact = render_addressable_markdown_with_options(value, options);
    let reference = render_addressable_markdown_with_options(baseline, options);
    let mapped = artifact
        .nodes()
        .iter()
        .filter(|node| matches!(node.node(), MarkdownNode::DocumentEntry { .. }))
        .collect::<Vec<_>>();
    let expected = reference
        .nodes()
        .iter()
        .filter(|node| matches!(node.node(), MarkdownNode::DocumentEntry { .. }))
        .collect::<Vec<_>>();
    assert_eq!(mapped.len(), 1);
    assert_eq!(expected.len(), 1);
    let MarkdownNode::DocumentEntry {
        owner: EntryOwner::Definition(owner),
        names,
        source: span,
        path,
        ..
    } = mapped[0].node()
    else {
        panic!("original definition")
    };
    let MarkdownNode::DocumentEntry {
        path: expected_path,
        ..
    } = expected[0].node()
    else {
        unreachable!()
    };
    assert!(std::ptr::eq(*owner, item(value)));
    assert_eq!(*names, item(value).entry.as_ref().unwrap().names.as_slice());
    assert_eq!(*span, Some(source(10)));
    assert_eq!(path, expected_path);
    let range = mapped[0].range();
    let bytes = &artifact.text()[range.clone()];
    assert_eq!(bytes.matches(BODY).count(), 1);
    assert_eq!(
        bytes.matches(TAIL).count(),
        0,
        "Tail must stay outside the original owner"
    );
    let start = artifact.text().find(BODY).unwrap();
    assert!(range.start <= start && start + BODY.len() <= range.end);
    assert_eq!(&artifact.text()[start..start + BODY.len()], BODY);
    if options.preserve_anchors {
        assert_search(value, artifact.text(), path.to_string().as_str());
    }
}

fn assert_search(value: &ResolvedContent, markdown: &str, owner_path: &str) {
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let found = mant_query::search_query(
            value,
            &SearchQuery {
                pattern: BODY.into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope,
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        let [hit] = found.matches.as_slice() else {
            panic!("one {scope:?} BODY: {found:#?}")
        };
        assert_eq!(hit.outline.path(), owner_path);
        let [occurrence] = hit.occurrences.as_slice() else {
            panic!("one BODY range")
        };
        let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
            ..usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(&markdown[range], BODY);
    }
}
