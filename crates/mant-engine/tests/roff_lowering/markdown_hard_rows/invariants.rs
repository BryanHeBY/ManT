//! Mutations prove that carrier, owner, occurrence and byte checks are active.
use super::*;
use mant_ir::{LinkTarget, visit::Visit};
use mant_protocol::{EntryProjection, QuerySearch, ReferenceProjection, ReferenceProjectionMode};

fn core(id: &str) -> Case {
    cases().into_iter().find(|case| case.id == id).unwrap()
}

fn alter_inlines(children: &mut [Inline], change: &mut impl FnMut(&mut Inline)) {
    for child in children {
        change(child);
        match child {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => alter_inlines(children, change),
            _ => {}
        }
    }
}

fn paragraph_children(content: &mut ResolvedContent) -> &mut [Inline] {
    let Block::Paragraph { children, .. } =
        &mut content.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        panic!("mutable paragraph")
    };
    children
}

fn reader(case: &Case) -> ResolvedContent {
    let source = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let markdown = render_markdown_with_options(
        &source,
        MarkdownOptions {
            native_text: true,
            ..Default::default()
        },
    );
    mant_loader::load_markdown_text(&markdown, None).unwrap()
}

#[test]
fn core_carrier_and_owner_checks_reject_wrong_style_target_source_and_body() {
    // These are mutated IR consumers of the already pristine-bound sources;
    // they never alter native expectations or bless candidate output.
    let case = core("M-core-paragraph-em-leading-one");
    let mut content = reader(&case);
    assert!(carrier_is_preserved(&case, &content, "AFTER"));
    let Block::Paragraph { children, .. } =
        &mut content.document.as_mut().unwrap().sections[1].blocks[0]
    else {
        panic!("description paragraph")
    };
    alter_inlines(children, &mut |node| {
        if let Inline::Emphasis { children } = node {
            *node = Inline::Strong {
                children: std::mem::take(children),
            };
        }
    });
    assert!(!carrier_is_preserved(&case, &content, "AFTER"));

    let case = core("M-core-paragraph-lk-leading-one");
    let mut content = reader(&case);
    assert!(carrier_is_preserved(&case, &content, "AFTER"));
    let Block::Paragraph { children, .. } =
        &mut content.document.as_mut().unwrap().sections[1].blocks[0]
    else {
        panic!("description paragraph")
    };
    alter_inlines(children, &mut |node| {
        if let Inline::Link { target, .. } = node {
            *target = LinkTarget::External {
                uri: "https://wrong.example".into(),
            };
        }
    });
    assert!(!carrier_is_preserved(&case, &content, "AFTER"));

    let case = core("M-core-hang-no-interior-one");
    let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    assert!(owner_is_preserved(&content));
    for change_source in [false, true] {
        let mut wrong = content.clone();
        let Block::DefinitionList { items, .. } =
            &mut wrong.document.as_mut().unwrap().sections[1].blocks[0]
        else {
            panic!("definition owner")
        };
        if change_source {
            items[0].source.as_mut().unwrap().line = 99;
        } else {
            items[0].terms[0].push(Inline::Text {
                value: "BodyWord".into(),
            });
            items[0].description.clear();
        }
        assert!(!owner_is_preserved(&wrong));
    }
}

fn search(content: &ResolvedContent, scope: SearchScope) -> QuerySearch {
    mant_query::search_query(
        content,
        &SearchQuery {
            pattern: "中e\u{301}🦀".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope,
            word: false,
            context_lines: 0,
            limit: 20,
            offset: 0,
        },
    )
    .unwrap()
}

fn exact_slices(found: &QuerySearch, artifact: &str) -> bool {
    found
        .matches
        .iter()
        .map(|hit| hit.occurrence_count)
        .sum::<u32>()
        == 2
        && found
            .matches
            .iter()
            .map(|hit| hit.occurrences.len())
            .sum::<usize>()
            == 2
        && found
            .matches
            .iter()
            .flat_map(|hit| &hit.occurrences)
            .all(|occurrence| {
                let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                    ..usize::try_from(occurrence.markdown.end_byte).unwrap();
                artifact.get(range.clone()) == Some(occurrence.matched_text.as_str())
                    && artifact.get(..range.start).is_some_and(|prefix| {
                        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
                        let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
                        line == usize::try_from(occurrence.markdown.start_line).unwrap()
                            && column == usize::try_from(occurrence.markdown.start_column).unwrap()
                    })
            })
}

fn source_ranges_match(found: &QuerySearch, source: &str) -> bool {
    found.matches.iter().all(|hit| {
        hit.node_source.is_some_and(|span| {
            // Search reports its nearest outline owner's provenance, the TEXT
            // heading. The paragraph's canonical range is checked independently
            // by codec's real source-offset tests; those are distinct owners.
            span.line == 3
                && span.column == 1
                && span.byte_range.is_some_and(|range| {
                    source
                        .get(
                            usize::try_from(range.start.get()).unwrap()
                                ..usize::try_from(range.end.get()).unwrap(),
                        )
                        .is_some_and(|raw| raw.starts_with("## TEXT"))
                })
        })
    })
}

fn distinct_references_match(
    records: &[mant_protocol::ReferenceRecord],
    document: &mant_ir::Document,
) -> bool {
    records.len() == 2 && records[0].origin != records[1].origin
        && records.iter().all(|record| {
            matches!(record.origin.resolve_link(document), Some(Inline::Link { target, children, .. })
                if target == &record.target && mant_ir::inline_plain_text(children) == "中e\u{301}🦀"
                    && record.label == "中e\u{301}🦀")
        })
}

#[test]
fn canonical_search_uses_real_unicode_artifact_bytes_and_distinct_link_occurrences() {
    let source = "# Tool\n\n## TEXT\n\n<br />\n[中e\u{301}🦀](https://example.org)<br>\n<br>\n[中e\u{301}🦀](https://example.org)\n";
    let content = mant_loader::load_markdown_text(source, Some("unicode.md".into())).unwrap();
    let outline = mant_query::build_outline_with_references(
        &content,
        EntryProjection::None,
        None,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![mant_ir::ReferenceTargetType::External],
            ..Default::default()
        },
    )
    .unwrap();
    let records = &outline.references.records;
    assert!(distinct_references_match(
        records,
        content.document.as_ref().unwrap()
    ));
    let mut forged = records.clone();
    forged[1].origin = forged[0].origin.clone();
    assert!(!distinct_references_match(
        &forged,
        content.document.as_ref().unwrap()
    ));
    let mut links = Links(vec![]);
    links.visit_document(content.document.as_ref().unwrap());
    assert_eq!(links.0.len(), 2);
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let artifact = render_addressable_markdown_with_options(
            &content,
            MarkdownOptions {
                native_text: scope == SearchScope::Visible,
                ..MarkdownOptions::ADDRESSABLE
            },
        );
        let found = search(&content, scope);
        assert_eq!(
            found
                .matches
                .iter()
                .map(|hit| hit.occurrence_count)
                .sum::<u32>(),
            2
        );
        assert!(exact_slices(&found, artifact.text()));
        assert!(source_ranges_match(&found, source));
        let mut forged = found.clone();
        let range = forged.matches[0]
            .node_source
            .as_mut()
            .unwrap()
            .byte_range
            .as_mut()
            .unwrap();
        range.start = mant_ir::TextSize::new(range.start.get() + 1);
        assert!(!source_ranges_match(&forged, source));
        for delta in [1, 3] {
            let mut forged = found.clone();
            forged.matches[0].occurrences[0].markdown.start_byte += delta;
            assert!(!exact_slices(&forged, artifact.text()));
        }
        let mut forged = found.clone();
        forged.matches[0].occurrences[0].markdown.start_column += 1;
        assert!(!exact_slices(&forged, artifact.text()));
        let mut forged = found.clone();
        forged.matches[0].occurrences.clear();
        assert!(!exact_slices(&forged, artifact.text()));
    }
    let mut wrong = content.clone();
    for node in paragraph_children(&mut wrong).iter_mut() {
        if let Inline::Link { children, .. } = node {
            *node = Inline::Text {
                value: mant_ir::inline_plain_text(children),
            };
        }
    }
    let mut missing = Links(vec![]);
    missing.visit_document(wrong.document.as_ref().unwrap());
    assert_ne!(missing.0, links.0, "occurrence loss must be observable");
}

#[test]
fn canonical_inline_breaks_are_real_word_boundaries_for_visible_search() {
    let source = "# Tool\n\n## TEXT\n\n<br />\nAlpha<br>Beta<br>\nGamma\n";
    let content = mant_loader::load_markdown_text(source, None).unwrap();
    for (pattern, count) in [("Beta", 1), ("AlphaBeta", 0)] {
        let query = mant_query::search_query(
            &content,
            &SearchQuery {
                pattern: pattern.into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope: SearchScope::Visible,
                word: true,
                context_lines: 0,
                limit: 20,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(query.total, count, "hard row became a word join");
    }
}
