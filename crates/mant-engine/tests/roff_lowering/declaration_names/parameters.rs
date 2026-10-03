//! Parameter completion, independent declarations and authoritative ownership.
use super::*;

fn parameter_cases() -> Vec<Case> {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("parameter_cases.json")).unwrap();
    assert_eq!(matrix["header"]["count"], 78);
    let mut cases: Vec<Case> = serde_json::from_value(matrix["cases"].clone()).unwrap();
    let matrix: serde_json::Value = serde_json::from_str(include_str!("token_cases.json")).unwrap();
    assert_eq!(matrix["header"]["count"], 302);
    assert_eq!(matrix["header"]["positiveCount"], 234);
    assert_eq!(matrix["header"]["negativeCount"], 68);
    let additional: Vec<Case> = serde_json::from_value(matrix["cases"].clone()).unwrap();
    assert_eq!(additional.len(), 302);
    cases.extend(additional);
    cases
}

#[test]
fn complete_parameters_and_opaque_values_share_the_tp_and_hp_grammar() {
    // All 380 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint before
    // these assertions. pre_alternate supplies real operand boundaries;
    // pre_HP/post_HP and pre_IP/post_IP retain independent reading blocks.
    // Name classification is ManT's reading contract, not a mandoc output.
    for case in parameter_cases() {
        assert_case_queries(&case);
        let content = roundtrip(&case);
        let document = content.document.as_ref().unwrap();
        let entries = owners(document);
        assert_eq!(
            entries.len(),
            usize::from(case.kind != "none"),
            "{}",
            case.id
        );
        for owner in entries {
            assert_owner(&case, owner);
            assert_ne!(owner.source(), None);
            let facts = owner.facts().unwrap();
            for name in &case.names {
                assert_eq!(
                    names_from_explain(&content, name),
                    [facts.id.to_string()],
                    "{}",
                    case.id
                );
                assert!(
                    search(&content, name)
                        .matches
                        .iter()
                        .any(|hit| hit.outline.node.id() == facts.id.as_str())
                );
            }
        }
        for excluded in &case.excluded {
            assert!(
                names_from_explain(&content, excluded).is_empty(),
                "{}: {excluded}",
                case.id
            );
        }
        assert_eq!(
            text_blocks(document)
                .join("\n")
                .matches(&case.body_word)
                .count(),
            1
        );
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("parameter.1", case.source.as_bytes())
            .unwrap();
        for owner in owners(document) {
            if matches!(owner, EntryOwner::List(_)) {
                assert_hanging_source(owner, &native.document.root);
            }
        }
        let expected = case.native_rows.as_ref().expect("exact pristine rows");
        let text = mant_render::render_query_man(&content);
        assert_eq!(
            text.split_once("OPTIONS\n")
                .unwrap()
                .1
                .trim_matches('\n')
                .split('\n')
                .collect::<Vec<_>>(),
            *expected,
            "{}: exact native rows",
            case.id
        );
        if case.owner_proof.head_macro.as_deref() == Some("TP") {
            for owner in owners(document) {
                assert!(
                    has_native_head(&native.document.root, owner.source().unwrap(), "TP"),
                    "{}: actual AST HEAD",
                    case.id
                );
            }
        }
        let markdown = render_markdown(&content);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_eq!(
            flow_words(&text_blocks(read.document.as_ref().unwrap())),
            flow_words(&text_blocks(document)),
            "{}: actual reader",
            case.id
        );
    }
}

#[test]
fn two_hanging_parameter_declarations_keep_query_ranges_and_bodies_disjoint() {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("parameter_cases.json")).unwrap();
    let case = &matrix["multipleOwners"];
    let original =
        mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
    let wire: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let content: ResolvedContent = wire.into();
    assert_eq!(content, original);
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("two-owners.1", case["source"].as_str().unwrap().as_bytes())
        .unwrap();
    let entries = owners(content.document.as_ref().unwrap());
    assert_eq!(entries.len(), 2);
    let artifact = render_addressable_markdown_with_options(&content, MarkdownOptions::ADDRESSABLE);
    for (index, owner) in entries.iter().enumerate() {
        let names: Vec<String> = serde_json::from_value(case["ownerNames"][index].clone()).unwrap();
        let body = case["bodyWords"][index].as_str().unwrap();
        let other = case["bodyWords"][1 - index].as_str().unwrap();
        assert_hanging_source(*owner, &native.document.root);
        let facts = owner.facts().unwrap();
        assert_eq!(facts.names, names);
        assert_eq!(facts.alias_groups.len(), 0);
        for binding in &facts.name_bindings {
            for occurrence in &binding.occurrences {
                assert_eq!(
                    mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
                    names[binding.name]
                );
            }
        }
        for name in &names {
            assert_name_positions(&content, *owner, name);
            let excerpt = crate::semantic_test_read::semantic_excerpt(&content, &[name]).unwrap();
            let text = mant_render::render_excerpt_text(&excerpt);
            assert!(text.contains(body));
            assert!(!text.contains(other));
            let found = search(&content, name);
            let hit = found
                .matches
                .iter()
                .find(|hit| hit.outline.node.id() == facts.id.as_str())
                .unwrap();
            assert_ne!(hit.occurrences.len(), 0);
            for occurrence in &hit.occurrences {
                let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
                let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
                assert_eq!(visible_range(artifact.text(), start..end), *name);
                assert_eq!(
                    position(artifact.text(), start),
                    (
                        occurrence.markdown.start_line,
                        occurrence.markdown.start_column
                    )
                );
                assert_eq!(
                    position(artifact.text(), end),
                    (occurrence.markdown.end_line, occurrence.markdown.end_column)
                );
            }
        }
    }
}

fn assert_hanging_source(owner: EntryOwner<'_>, root: &libmandoc_rs::Node) {
    let EntryOwner::List(item) = owner else {
        panic!("hanging owner")
    };
    let mut pairs = Vec::new();
    crate::hanging_owners::ast_pairs(root, &mut pairs);
    let head_source = mant_ir::geometry::block_source(&item.blocks[0]).unwrap();
    let Block::DefinitionList { items, .. } = &item.blocks[1] else {
        panic!("actual IP body")
    };
    let source = items[0].source.unwrap();
    assert!(pairs.iter().any(|(line, column, head)| (*line, *column)
        == (source.line, source.column)
        && crate::hanging_owners::descendant_has_source(head, head_source)));
}

fn manual_links(document: &Document) -> Vec<String> {
    struct Links(Vec<String>);
    impl<'a> Visit<'a> for Links {
        fn visit_inline(&mut self, inline: &'a mant_ir::Inline) {
            if let mant_ir::Inline::Link {
                target:
                    mant_ir::LinkTarget::Manual {
                        name,
                        manual_section,
                    },
                ..
            } = inline
            {
                self.0
                    .push(format!("{name}({})", manual_section.as_deref().unwrap()));
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut links = Links(Vec::new());
    links.visit_document(document);
    links.0
}

#[test]
fn parameter_owner_bodies_keep_hard_lines_and_original_manual_targets() {
    for case in parameter_cases()
        .into_iter()
        .filter(|case| case.id.ends_with("hard-line-link"))
    {
        let content = roundtrip(&case);
        let artifact = mant_codec::encode::render_addressable_markdown(&content);
        let read = mant_loader::load_markdown_text(artifact.text(), None).unwrap();
        assert!(
            text_blocks(read.document.as_ref().unwrap())
                .iter()
                .any(|text| text.contains("DescriptionWord.\nprintf(3)")),
            "{}: actual reader hard line",
            case.id
        );
        assert_eq!(
            manual_links(content.document.as_ref().unwrap()),
            ["printf(3)"]
        );
        // Whole-document export deliberately omits Manual wrappers; the
        // original JSON retains their target while the reader keeps glyphs
        // and hard rows. Do not infer a missing target from visible text.
        assert_eq!(manual_links(read.document.as_ref().unwrap()).len(), 0);
    }
}
