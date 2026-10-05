use super::*;

fn targets(value: &ResolvedContent) -> Vec<LinkTarget> {
    let document = value.document.as_ref().unwrap();
    let mut targets = vec![];
    let report = scan_references(document, ReferenceScanLimits::default(), |link| {
        let location = link.location.to_owned().unwrap();
        assert!(std::ptr::eq(
            location.resolve_link(document).unwrap(),
            link.link
        ));
        assert_eq!(inline_plain_text(link.label), "");
        targets.push(link.target.clone());
        std::ops::ControlFlow::Continue(())
    });
    assert!(report.complete());
    targets
}

pub(super) fn assert_targets(value: &ResolvedContent, linked: bool) {
    assert_eq!(
        targets(value),
        if linked {
            vec![LinkTarget::External { uri: URI.into() }]
        } else {
            vec![]
        }
    );
}

fn search(value: &ResolvedContent, word: &str, scope: SearchScope) -> mant_protocol::QuerySearch {
    mant_query::search_query(
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
    .unwrap()
}

fn owners(value: &ResolvedContent) -> Vec<EntryOwner<'_>> {
    struct Owners<'a>(Vec<EntryOwner<'a>>);
    impl<'ir> visit::Visit<'ir> for Owners<'ir> {
        fn visit_list_item(&mut self, item: &'ir ListItem) {
            if item.entry.is_some() {
                self.0.push(EntryOwner::List(item));
            }
            visit::walk_list_item(self, item);
        }
        fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
            if item.entry.is_some() {
                self.0.push(EntryOwner::Definition(item));
            }
            visit::walk_definition_item(self, item);
        }
    }
    let mut owners = Owners(vec![]);
    visit::Visit::visit_document(&mut owners, value.document.as_ref().unwrap());
    owners.0
}

pub(super) fn fixture(value: &ResolvedContent, label: &str) -> ResolvedContent {
    let problems = validate_document(value.document.as_ref().unwrap());
    assert!(
        problems.is_empty(),
        "{label}: invalid source fixture: {problems:#?}"
    );
    round_trip(value)
}

fn without_entry_facts(value: &ResolvedContent) -> ResolvedContent {
    struct RemoveFacts;
    impl visit::VisitMut for RemoveFacts {
        fn visit_list_item_mut(&mut self, item: &mut ListItem) {
            item.entry = None;
            visit::walk_list_item_mut(self, item);
        }
        fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
            item.entry = None;
            visit::walk_definition_item_mut(self, item);
        }
    }
    // Exact row tests do not reapprove raw generated owner HTML reading.
    // Original facts/maps/search are asserted before this presentation clone.
    // Authored Anchor, Link, text and LF roots remain untouched.
    let mut result = value.clone();
    visit::VisitMut::visit_document_mut(&mut RemoveFacts, result.document.as_mut().unwrap());
    result
}

fn assert_search(value: &ResolvedContent, leaf: Leaf, linked: bool) {
    let artifact = render_addressable_markdown_with_options(value, MarkdownOptions::ADDRESSABLE);
    let document = value.document.as_ref().unwrap();
    let originals = owners(value);
    let owner = originals.first();
    let (id, span) = if let Some(owner) = owner {
        (owner.facts().unwrap().id.as_str(), owner.source())
    } else if let Some(section) = document.sections.first() {
        (section.id.as_str(), section.source)
    } else {
        (DOCUMENT_ROOT_ID, None)
    };
    let mut queries = vec![];
    if leaf != Leaf::Rule {
        queries.extend([(BODY, SearchScope::Visible), (BODY, SearchScope::Markdown)]);
    }
    if linked {
        queries.push((URI, SearchScope::Markdown));
        assert_eq!(search(value, URI, SearchScope::Visible).total, 0);
    }
    for (word, scope) in queries {
        let found = search(value, word, scope);
        let [hit] = found.matches.as_slice() else {
            panic!("one {scope:?}/{word}: {found:#?}")
        };
        assert_eq!(hit.outline.node.id(), id);
        assert_eq!(hit.node_source, span);
        let [occurrence] = hit.occurrences.as_slice() else {
            panic!("one exact {word} occurrence")
        };
        assert_eq!(occurrence.matched_text, word);
        let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
            ..usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(&artifact.text()[range], word);
    }
}

pub(super) fn assert_original(value: &ResolvedContent, leaf: Leaf, linked: bool) {
    assert_targets(value, linked);
    assert_search(value, leaf, linked);
    let artifact = render_addressable_markdown_with_options(value, MarkdownOptions::ADDRESSABLE);
    let originals = owners(value);
    let mut mapped_count = 0;
    for node in artifact.nodes() {
        let MarkdownNode::DocumentEntry {
            owner,
            names,
            source: span,
            ..
        } = node.node()
        else {
            continue;
        };
        let facts = owner.facts().unwrap();
        mapped_count += 1;
        assert!(
            originals
                .iter()
                .any(|original| std::ptr::eq(original.facts().unwrap(), facts))
        );
        assert_eq!(facts.id, OWNER);
        assert_eq!(facts.names, [] as [String; 0]);
        assert_eq!(*names, facts.names.as_slice());
        assert_eq!(*span, Some(source(10)));
        let mapped = &artifact.text()[node.range()];
        assert_eq!(
            mapped.matches(BODY).count(),
            usize::from(leaf != Leaf::Rule)
        );
        assert_eq!(mapped.matches(URI).count(), usize::from(linked));
    }
    assert_eq!(
        mapped_count,
        originals.len(),
        "every original owner has a map"
    );
}

pub(super) fn assert_header_projection(value: &ResolvedContent, occurrences: usize) {
    let artifact = render_addressable_markdown_with_options(value, MarkdownOptions::ADDRESSABLE);
    let entries = artifact
        .nodes()
        .iter()
        .filter(|node| matches!(node.node(), MarkdownNode::DocumentEntry { .. }))
        .collect::<Vec<_>>();
    let [entry] = entries.as_slice() else {
        panic!("one original HEAD owner map")
    };
    assert_eq!(
        artifact.text()[entry.range()].matches(NAME).count(),
        occurrences
    );
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let found = search(value, NAME, scope);
        assert_eq!(
            found
                .matches
                .iter()
                .map(|hit| hit.occurrences.len())
                .sum::<usize>(),
            occurrences,
            "exact original HEAD occurrences: {found:#?}"
        );
        for hit in &found.matches {
            // A nested original inline root can produce its own hit. Its
            // semantic owner and exact byte slices remain the same contract.
            assert_eq!(hit.outline.node.id(), OWNER);
            assert_eq!(hit.node_source, Some(source(10)));
            for occurrence in &hit.occurrences {
                assert_eq!(occurrence.matched_text, NAME);
                let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
                let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
                assert_eq!(&artifact.text()[start..end], NAME);
            }
        }
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Shape {
    lists: Vec<ListKind>,
    literals: Vec<(String, Option<String>)>,
    rules: usize,
}

fn row_content(block: &Block) -> bool {
    match block {
        Block::VerticalSpace { .. } => false,
        Block::Paragraph { children, .. } => first_visible_character(children).is_some(),
        Block::DefinitionList { items, .. } => items.iter().any(|item| {
            item.terms
                .iter()
                .any(|term| first_visible_character(&term.content).is_some())
                || item.description.iter().any(row_content)
        }),
        _ => true,
    }
}

impl<'ir> visit::Visit<'ir> for Shape {
    fn visit_block(&mut self, block: &'ir Block) {
        match block {
            Block::List { kind, .. } => self.lists.push(*kind),
            Block::DefinitionList { .. } if row_content(block) => {
                self.lists.push(ListKind::Bullet);
            }
            Block::Preformatted {
                children, language, ..
            } => {
                self.literals
                    .push((inline_plain_text(children), language.clone()));
            }
            Block::ThematicBreak { .. } => self.rules += 1,
            _ => {}
        }
        visit::walk_block(self, block);
    }
}

fn shape(value: &ResolvedContent) -> Shape {
    let mut shape = Shape::default();
    visit::Visit::visit_document(&mut shape, value.document.as_ref().unwrap());
    shape
}

pub(super) fn next(value: &ResolvedContent, preserve_anchors: bool) -> (ResolvedContent, String) {
    // Whole-artifact maps/search/targets are asserted above. The public block
    // fragment isolates source rows from the frozen raw heading-anchor reader
    // policy; no source HTML, inline node or authored LF is stripped here.
    let blocks = render_blocks_fragment(
        scope_blocks(value),
        MarkdownFragmentOptions { preserve_anchors },
    );
    let markdown = format!("# Probe\n\n{}", blocks.join("\n\n"));
    let parsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
    (round_trip(&parsed), markdown)
}

pub(super) fn check_two_cycles(
    source: &ResolvedContent,
    expected: &str,
    label: &str,
    leaf: Leaf,
    linked: bool,
    failures: &mut Vec<String>,
) {
    let original = source.clone();
    let expected_shape = shape(&original);
    for preserve_anchors in [false, true] {
        let mut value = if preserve_anchors {
            without_entry_facts(source)
        } else {
            source.clone()
        };
        for cycle in 1..=2 {
            let (parsed, markdown) = next(&value, preserve_anchors);
            let actual = reading(&parsed);
            if actual != expected {
                failures.push(format!(
                    "{label}/anchors={preserve_anchors}/cycle={cycle}: expected {expected:?}, actual {actual:?}\n{markdown}"
                ));
            }
            assert_eq!(
                shape(&parsed),
                expected_shape,
                "{label}/{cycle}: {markdown}"
            );
            assert_targets(&parsed, linked);
            assert_search(&parsed, leaf, linked);
            value = parsed;
        }
    }
    assert_eq!(
        *source, original,
        "projection cannot mutate accepted public IR"
    );
}

pub(super) fn assert_two_cycles(
    source: &ResolvedContent,
    expected: &str,
    label: &str,
    leaf: Leaf,
    linked: bool,
) {
    let mut failures = vec![];
    check_two_cycles(source, expected, label, leaf, linked, &mut failures);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
