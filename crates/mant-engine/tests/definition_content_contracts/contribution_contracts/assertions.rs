use super::*;

fn links(value: &ResolvedContent) -> Vec<(ContentLocation, LinkTarget, Option<SourceSpan>)> {
    let document = value.document.as_ref().unwrap();
    let mut found = vec![];
    let report = scan_references(document, ReferenceScanLimits::default(), |link| {
        let address = link.location.to_owned().unwrap();
        assert!(std::ptr::eq(
            address.resolve_link(document).unwrap(),
            link.link
        ));
        assert_eq!(inline_plain_text(link.label), "");
        assert_eq!(link.source, Some(source(12)));
        if let Some(semantic) = link.semantic_owner {
            assert_eq!(semantic.owner.facts().unwrap().id, OWNER);
            assert_eq!(semantic.owner.source(), Some(source(10)));
        }
        found.push((address, link.target.clone(), link.source));
        std::ops::ControlFlow::Continue(())
    });
    assert!(report.complete());
    found
}

fn original_owner(value: &ResolvedContent) -> Option<EntryOwner<'_>> {
    struct Owners<'a>(Vec<EntryOwner<'a>>);
    impl<'ir> visit::Visit<'ir> for Owners<'ir> {
        fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
            if item.entry.is_some() {
                self.0.push(EntryOwner::Definition(item));
            }
            visit::walk_definition_item(self, item);
        }
        fn visit_list_item(&mut self, item: &'ir ListItem) {
            if item.entry.is_some() {
                self.0.push(EntryOwner::List(item));
            }
            visit::walk_list_item(self, item);
        }
    }
    let mut owners = Owners(vec![]);
    visit::Visit::visit_document(&mut owners, value.document.as_ref().unwrap());
    assert!(owners.0.len() <= 1);
    owners.0.pop()
}

pub(super) fn assert_snapshot(original: &ResolvedContent, restored: &ResolvedContent) {
    assert_eq!(restored, original);
    assert_eq!(links(restored), links(original));
    let Some(owner) = original_owner(restored) else {
        return;
    };
    assert_eq!(owner.source(), Some(source(10)));
    let facts = owner.facts().unwrap();
    assert_eq!(facts.id, OWNER);
    assert_eq!(owner.validated_names().unwrap(), facts.names);
    if let Some(binding) = facts.name_bindings.first() {
        assert_eq!(
            inline_plain_text(&owner.form(&binding.occurrences[0]).unwrap()),
            NAME
        );
        let content = owner
            .inline_content_root(&binding.occurrences[0].parts[0].root)
            .unwrap();
        let Inline::Anchor {
            id,
            fragment_aliases,
            owner_source,
        } = &content.content[1]
        else {
            panic!("original name root anchor")
        };
        assert_eq!(id, "contribution-anchor");
        assert_eq!(
            fragment_aliases,
            &[FragmentAlias::from("Contribution.Anchor")]
        );
        assert_eq!(*owner_source, Some(source(10)));
    }
    let artifact = render_addressable_markdown_with_options(restored, MarkdownOptions::ADDRESSABLE);
    let entries = artifact
        .nodes()
        .iter()
        .filter_map(|node| match node.node() {
            MarkdownNode::DocumentEntry {
                owner,
                names,
                source: span,
                ..
            } => Some((node.range(), owner, names, span)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [(range, mapped, names, span)] = entries.as_slice() else {
        panic!("one original mapped owner")
    };
    assert!(std::ptr::eq(mapped.facts().unwrap(), facts));
    assert_eq!(**names, facts.names.as_slice());
    assert_eq!(**span, Some(source(10)));
    if !facts.names.is_empty() {
        assert_eq!(artifact.text()[range.clone()].matches(NAME).count(), 1);
    }
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

fn search_owner(
    value: &ResolvedContent,
    word: &str,
    scope: SearchScope,
    id: &str,
    span: Option<SourceSpan>,
) {
    let found = search(value, word, scope);
    let [hit] = found.matches.as_slice() else {
        panic!("one {scope:?} {word}: {found:#?}")
    };
    assert_eq!(hit.outline.node.id(), id);
    assert_eq!(hit.node_source, span);
    let [occurrence] = hit.occurrences.as_slice() else {
        panic!("one exact occurrence")
    };
    let artifact = render_addressable_markdown_with_options(value, MarkdownOptions::ADDRESSABLE);
    let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
        ..usize::try_from(occurrence.markdown.end_byte).unwrap();
    assert_eq!(&artifact.text()[range], word);
}

pub(super) fn assert_queries(
    value: &ResolvedContent,
    context: Context,
    has_body: bool,
    has_link: bool,
) {
    let document = value.document.as_ref().unwrap();
    let (id, span) = match context {
        Context::Root => (DOCUMENT_ROOT_ID, None),
        Context::Section => (
            document.sections[0].id.as_str(),
            document.sections[0].source,
        ),
        Context::List | Context::Definition | Context::Nested => (OWNER, Some(source(10))),
    };
    if has_body {
        for scope in [SearchScope::Visible, SearchScope::Markdown] {
            search_owner(value, BODY, scope, id, span);
        }
    }
    if has_link {
        search_owner(value, URI, SearchScope::Markdown, id, span);
        assert_eq!(search(value, URI, SearchScope::Visible).total, 0);
    }
}

pub(super) fn assert_uri_owner(value: &ResolvedContent) {
    search_owner(value, URI, SearchScope::Markdown, OWNER, Some(source(10)));
}

fn block_shape(blocks: &[Block]) -> Vec<String> {
    let mut output = vec![];
    let mut pending = 0_u16;
    for block in blocks {
        let gap = pending.saturating_add(geometry::block_gap(block));
        if matches!(block, Block::VerticalSpace { .. })
            || matches!(block, Block::Paragraph { children, .. } if inline_plain_text(children).is_empty())
        {
            pending = gap;
            continue;
        }
        pending = 0;
        match block {
            Block::Paragraph { children, .. } => {
                output.push(format!("prose/{gap}:{:?}", inline_plain_text(children)));
            }
            Block::Preformatted {
                children, language, ..
            } => output.push(format!(
                "literal/{gap}/{language:?}:{:?}",
                inline_plain_text(children)
            )),
            Block::List {
                kind,
                compact,
                items,
                ..
            } => {
                output.push(format!("list/{gap}/{kind:?}/{compact}"));
                for item in items {
                    output.push(format!("item/{:?}", item.layout.spacing_before_lines));
                    output.extend(block_shape(&item.blocks));
                    output.push("/item".into());
                }
                output.push("/list".into());
            }
            Block::ThematicBreak { .. } => output.push(format!("rule/{gap}")),
            Block::Unsupported { name, text, .. } => {
                output.push(format!("unsupported/{gap}/{name:?}:{text:?}"));
            }
            _ => panic!("unexpected readback type: {block:#?}"),
        }
    }
    if pending > 0 {
        output.push(format!("gap/{pending}"));
    }
    output
}

fn shape(value: &ResolvedContent) -> Vec<String> {
    fn sections(values: &[Section], output: &mut Vec<String>) {
        for section in values {
            output.push(format!("section:{:?}", section.heading.single_line_text()));
            output.extend(block_shape(&section.blocks));
            sections(&section.children, output);
            output.push("/section".into());
        }
    }
    let document = value.document.as_ref().unwrap();
    let mut result = vec![format!(
        "heading:{:?}",
        document.heading.as_ref().map(Heading::single_line_text)
    )];
    result.extend(block_shape(&document.blocks));
    sections(&document.sections, &mut result);
    result
}

pub(super) fn assert_control(
    value: &ResolvedContent,
    baseline: &ResolvedContent,
    options: MarkdownOptions,
    has_link: bool,
) -> ResolvedContent {
    let actual = render_markdown_with_options(value, options);
    let control = render_markdown_with_options(baseline, options);
    let imported = mant_loader::load_markdown_text(&actual, None).unwrap();
    let expected = mant_loader::load_markdown_text(&control, None).unwrap();
    assert_eq!(
        shape(&imported),
        shape(&expected),
        "actual:\n{actual}\ncontrol:\n{control}"
    );
    assert_eq!(
        mant_render::render_query_man(&imported),
        mant_render::render_query_man(&expected),
        "actual:\n{actual}\ncontrol:\n{control}"
    );
    let mut targets = vec![];
    let report = scan_references(
        imported.document.as_ref().unwrap(),
        ReferenceScanLimits::default(),
        |link| {
            targets.push((link.target.clone(), inline_plain_text(link.label)));
            std::ops::ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    let wanted = if has_link {
        vec![(LinkTarget::External { uri: URI.into() }, String::new())]
    } else {
        vec![]
    };
    assert_eq!(targets, wanted, "{actual}");
    assert_eq!(
        actual.matches(URI).count(),
        usize::from(has_link),
        "{actual}"
    );
    imported
}

pub(super) fn assert_rule(value: &ResolvedContent) {
    struct Rules(usize);
    impl<'ir> visit::Visit<'ir> for Rules {
        fn visit_block(&mut self, block: &'ir Block) {
            self.0 += usize::from(matches!(block, Block::ThematicBreak { .. }));
            visit::walk_block(self, block);
        }
    }
    let mut rules = Rules(0);
    visit::Visit::visit_document(&mut rules, value.document.as_ref().unwrap());
    assert_eq!(rules.0, 1, "a thematic rule is not a setext heading");
}

pub(super) fn assert_types(value: &ResolvedContent, physical: Physical, empty_codes: usize) {
    struct Types {
        literals: Vec<(String, Option<String>)>,
        body: Vec<String>,
        rules: usize,
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
                Block::Paragraph { children, .. } => {
                    let text = inline_plain_text(children);
                    if text.contains(BODY) {
                        self.body.push(text);
                    }
                }
                Block::ThematicBreak { .. } => self.rules += 1,
                Block::List { .. } => self.lists += 1,
                _ => {}
            }
            visit::walk_block(self, block);
        }
    }
    let mut types = Types {
        literals: vec![],
        body: vec![],
        rules: 0,
        lists: 0,
    };
    visit::Visit::visit_document(&mut types, value.document.as_ref().unwrap());
    assert_eq!(
        types
            .literals
            .iter()
            .filter(|(text, lang)| text.is_empty() && lang.as_deref() == Some("empty"))
            .count(),
        empty_codes
    );
    let other = types
        .literals
        .iter()
        .filter(|(_, lang)| lang.as_deref() != Some("empty"))
        .cloned()
        .collect::<Vec<_>>();
    let expected = if physical == Physical::Fence {
        vec![("Contribution中\nLiteralEnd".into(), Some("contract".into()))]
    } else {
        vec![]
    };
    assert_eq!(
        other, expected,
        "no-row literal owners cannot create a code block"
    );
    assert_eq!(
        types.literals.len(),
        empty_codes + usize::from(physical == Physical::Fence)
    );
    assert_eq!(types.rules, usize::from(physical == Physical::Rule));
    assert_eq!(types.body.len(), usize::from(physical != Physical::Fence));
    if let Some(body) = types.body.first() {
        assert!(body.ends_with(physical.phrase()), "{body}");
    }
    if physical == Physical::List {
        assert!(types.lists >= 2);
    }
    assert_eq!(
        mant_render::render_query_man(value).matches(BODY).count(),
        1
    );
}

pub(super) fn assert_positive_gap(value: &ResolvedContent, physical: Physical) {
    let plain = mant_render::render_query_man(value);
    let head = plain.find(NAME).unwrap() + NAME.len();
    let first = plain
        .find(if physical == Physical::Rule {
            "---"
        } else {
            physical.phrase()
        })
        .unwrap();
    assert!(
        plain[head..first].matches('\n').count() >= 2,
        "one completed blank before real BODY: {plain}"
    );
}

pub(super) fn assert_boundary(
    value: &ResolvedContent,
    physical: Physical,
    relation: HeadBodyRelation,
) {
    let plain = mant_render::render_query_man(value);
    let head = plain.find(NAME).unwrap() + NAME.len();
    let first = plain
        .find(if physical == Physical::Rule {
            "---"
        } else {
            physical.phrase()
        })
        .unwrap();
    let breaks = plain[head..first].matches('\n').count();
    if physical == Physical::Paragraph && relation != HeadBodyRelation::Separate {
        assert_eq!(breaks, 0, "shared prose remains one row: {plain}");
    } else {
        assert!(
            breaks >= 1,
            "Separate/structural BODY cannot become a soft LF: {plain}"
        );
    }
}
