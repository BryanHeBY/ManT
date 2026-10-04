//! Original owner addresses, exact readback rows and mapped byte bounds.
use super::fixtures::child_owner;
use super::*;

pub(super) fn assert_owner_identity(
    value: &ResolvedContent,
    baseline: &ResolvedContent,
    position: Position,
) {
    assert_eq!(item(value).entry, item(baseline).entry);
    let child = child_owner(value, position);
    let control = child_owner(baseline, position);
    assert_eq!(child.facts(), control.facts());
    assert_eq!(child.source(), Some(source(25)));
    assert_eq!(child.validated_names().unwrap(), [CHILD]);
    let binding = &child.facts().unwrap().name_bindings[0].occurrences[0];
    assert_eq!(inline_plain_text(&child.form(binding).unwrap()), CHILD);
    assert_eq!(child.blocks(), control.blocks());
    let root = &child.facts().unwrap().forms[0].parts[0].root;
    let content = child.inline_content_root(root).unwrap();
    let Inline::Anchor {
        id,
        fragment_aliases,
        owner_source,
    } = &content.content[1]
    else {
        panic!("the original named root still owns its anchor")
    };
    assert_eq!(id, "child-place");
    assert_eq!(fragment_aliases, &[FragmentAlias::from("Child.Place")]);
    assert_eq!(*owner_source, Some(source(25)));
}

pub(super) fn assert_navigation_address(
    value: &ResolvedContent,
    expected: &ContentLocation,
    line: u32,
) {
    let document = value.document.as_ref().unwrap();
    let mut links = vec![];
    let report = scan_references(document, ReferenceScanLimits::default(), |link| {
        let location = link.location.to_owned().unwrap();
        assert_eq!(&location, expected);
        assert!(std::ptr::eq(
            location.resolve_link(document).unwrap(),
            link.link
        ));
        assert_eq!(inline_plain_text(link.label), "");
        assert_eq!(link.source, Some(source(line)));
        let Some(ReferenceOwnerRef {
            owner: EntryOwner::Definition(owner),
            ..
        }) = link.semantic_owner
        else {
            panic!("navigation retains the outer definition's identity")
        };
        assert!(std::ptr::eq(owner, item(value)));
        links.push(link.target.clone());
        std::ops::ControlFlow::Continue(())
    });
    assert!(report.complete());
    assert_eq!(links, Navigation::External.targets());
}

pub(super) fn assert_search_owner(
    value: &ResolvedContent,
    word: &str,
    id: &str,
    line: u32,
    scope: SearchScope,
) {
    let artifact = render_addressable_markdown_with_options(value, MarkdownOptions::ADDRESSABLE);
    let result = mant_query::search_query(
        value,
        &SearchQuery {
            pattern: word.into(),
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
    let [hit] = result.matches.as_slice() else {
        panic!("one {word:?}: {result:#?}")
    };
    assert_eq!(hit.outline.node.id(), id, "{word}: {:#?}", result.matches);
    assert_eq!(hit.node_source, Some(source(line)));
    let [occurrence] = hit.occurrences.as_slice() else {
        panic!("one exact byte range")
    };
    let bytes = usize::try_from(occurrence.markdown.start_byte).unwrap()
        ..usize::try_from(occurrence.markdown.end_byte).unwrap();
    assert_eq!(&artifact.text()[bytes], word);
}

pub(super) fn assert_owned_ranges(
    value: &ResolvedContent,
    baseline: &ResolvedContent,
    position: Position,
    options: MarkdownOptions,
) {
    let artifact = render_addressable_markdown_with_options(value, options);
    let control = render_addressable_markdown_with_options(baseline, options);
    let uri_start = artifact.text().find(EXTERNAL_A).unwrap();
    let uri_end = uri_start + EXTERNAL_A.len();
    let mut entries = 0;
    for mapped in artifact.nodes() {
        let MarkdownNode::DocumentEntry {
            owner,
            names,
            source: span,
            path,
            ..
        } = mapped.node()
        else {
            continue;
        };
        entries += 1;
        let range = mapped.range();
        let bytes = &artifact.text()[range.clone()];
        assert!(!bytes.contains(TAIL), "{bytes}");
        let previous = control
            .nodes()
            .iter()
            .find(|node| match node.node() {
                MarkdownNode::DocumentEntry {
                    owner: previous, ..
                } => previous.facts().unwrap().id == owner.facts().unwrap().id,
                _ => false,
            })
            .unwrap();
        let MarkdownNode::DocumentEntry {
            path: expected_path,
            ..
        } = previous.node()
        else {
            unreachable!()
        };
        assert_eq!(path, expected_path);
        if owner.facts().unwrap().id == CHILD_ID {
            assert_eq!(*names, &[CHILD.to_owned()]);
            assert_eq!(*span, Some(source(25)));
            assert!(std::ptr::eq(
                owner.facts().unwrap(),
                child_owner(value, position).facts().unwrap()
            ));
            assert!(bytes.contains(CHILD) && bytes.contains(PAYLOAD), "{bytes}");
            assert!(
                range.end <= uri_start || uri_end <= range.start,
                "ancestor navigation leaked into child: {bytes}"
            );
            let previous_bytes = &control.text()[previous.range()];
            assert_child_marker(
                artifact.text(),
                bytes,
                control.text(),
                previous_bytes,
                options.preserve_anchors && matches!(owner, EntryOwner::List(_)),
            );
            for word in [CHILD, PAYLOAD] {
                let start = artifact.text().find(word).unwrap();
                assert!(range.start <= start && start + word.len() <= range.end);
                assert_eq!(&artifact.text()[start..start + word.len()], word);
            }
        } else {
            assert_eq!(owner.facts().unwrap().id, "navigation-owner");
            assert_eq!(*span, Some(source(10)));
            assert_eq!(bytes.matches(EXTERNAL_A).count(), 1, "{bytes}");
            assert!(range.start <= uri_start && uri_end <= range.end);
        }
    }
    assert_eq!(entries, 2);
}

fn assert_child_marker(actual: &str, bytes: &str, baseline: &str, previous: &str, emitted: bool) {
    let marker = format!("id=\"{CHILD_ID}\"");
    // Ordinary List emits its owner ID; Definition's frozen export policy
    // does not synthesize that marker. Authored child-place/alias anchors
    // remain in both baselines. An emitted own ID must stay with its child.
    assert_eq!(
        baseline.matches(&marker).count(),
        usize::from(emitted),
        "{baseline}"
    );
    assert_eq!(
        actual.matches(&marker).count(),
        baseline.matches(&marker).count(),
        "{actual}"
    );
    assert_eq!(
        previous.matches(&marker).count(),
        usize::from(emitted),
        "{baseline}"
    );
    assert_eq!(
        bytes.matches(&marker).count(),
        previous.matches(&marker).count(),
        "the child's own ID stays in its original range\nactual:\n{actual}\nbaseline:\n{baseline}\nchild:\n{bytes}\ncontrol child:\n{previous}"
    );
}

pub(super) fn assert_owned_rows(
    imported: &ResolvedContent,
    child: Child,
    surface: Surface,
    head: Head,
    options: MarkdownOptions,
) {
    let plain = mant_render::render_query_man(imported);
    for word in [CHILD, PAYLOAD, TAIL] {
        assert_eq!(plain.matches(word).count(), 1, "{plain}");
    }
    assert_eq!(plain.matches(NAME).count(), usize::from(head.has_word()));
    assert_literal_payload(imported, surface);
    if options.preserve_anchors {
        return;
    }
    if head.has_word() && !matches!(child, Child::EmptyDefinition) {
        let start = plain.find(NAME).unwrap() + NAME.len();
        let end = plain.find(CHILD).unwrap();
        assert_eq!(
            plain[start..end].matches('\n').count(),
            if matches!(surface, Surface::LiteralFirst) {
                2
            } else {
                1
            },
            "structural BODY starts its own row: {plain}"
        );
    }
    let last = if matches!(surface, Surface::Literal | Surface::LiteralFirst) {
        "LiteralEnd"
    } else {
        PAYLOAD
    };
    let start = plain.find(last).unwrap() + last.len();
    let end = plain.find(TAIL).unwrap();
    assert_eq!(
        plain[start..end].matches('\n').count(),
        2,
        "Tail remains an independent paragraph: {plain}"
    );
}

fn assert_literal_payload(imported: &ResolvedContent, surface: Surface) {
    struct Literals(Vec<String>);
    impl<'ir> visit::Visit<'ir> for Literals {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::Preformatted {
                children, language, ..
            } = block
                && language.as_deref() == Some("txt")
            {
                self.0.push(inline_plain_text(children));
            }
            visit::walk_block(self, block);
        }
    }
    let mut literals = Literals(vec![]);
    visit::Visit::visit_document(&mut literals, imported.document.as_ref().unwrap());
    let expected = match surface {
        Surface::Literal => vec!["Payload終\nLiteralEnd".to_owned()],
        Surface::LiteralFirst => vec!["Child名\nPayload終\nLiteralEnd".to_owned()],
        Surface::Paragraph | Surface::NestedList => vec![],
    };
    assert_eq!(
        literals.0, expected,
        "a named literal root remains a complete fence payload"
    );
}

pub(super) fn assert_hard_rows(imported: &ResolvedContent, tail: BodyTail, multiple: bool) {
    let plain = mant_render::render_query_man(imported);
    assert_eq!(plain.matches(NAME).count(), 1);
    assert_eq!(plain.matches(SECOND_HEAD).count(), usize::from(multiple));
    if multiple {
        let start = plain.find(NAME).unwrap() + NAME.len();
        let end = plain.find(SECOND_HEAD).unwrap();
        assert_eq!(
            plain[start..end].matches('\n').count(),
            2,
            "independent Hard terms: {plain}"
        );
    }
    let last = if matches!(tail, BodyTail::Literal) {
        "LiteralTail"
    } else if multiple {
        SECOND_HEAD
    } else {
        NAME
    };
    assert_eq!(
        plain.matches(BODY).count(),
        usize::from(matches!(tail, BodyTail::Literal))
    );
    let start = plain.find(last).unwrap() + last.len();
    let end = plain.find(TAIL).unwrap();
    assert_eq!(
        plain[start..end].matches('\n').count(),
        2,
        "zero term cannot occupy an open tail: {plain}"
    );
}
