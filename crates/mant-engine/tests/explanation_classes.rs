//! Classification is an owner fact, never inferred from budgeted details.
use mant_engine::{explain_query, query_markdown_text};
use mant_protocol::{
    EvidenceBasis, EvidenceClass, EvidenceOrder, ExplanationOptions, ExplanationQuery,
};

fn query() -> ExplanationQuery {
    ExplanationQuery {
        entry: "--help".into(),
        options: ExplanationOptions::default(),
    }
}

#[test]
fn classes_merge_bases_but_not_independent_owners() {
    let content = query_markdown_text(
        r#"# Probe

Context mentioning --help.

<!-- mant:entries role=option case=sensitive -->
- `-Q`: Read --help for guidance. Unrelated full-body detail.
- `--assist`: Use --help. <!-- mant:entry {"id":"assist","aliasOf":"help"} -->
- `--help`: Help one. <!-- mant:entry {"id":"help"} -->
- `--help=CLASS`: Help two.
"#,
        None,
    )
    .unwrap();
    let result = explain_query(&content, &query()).unwrap();
    assert_eq!(result.order, EvidenceOrder::ClassThenSource);
    assert_eq!(
        result.evidence.iter().map(|e| e.class).collect::<Vec<_>>(),
        [
            EvidenceClass::DirectEntry,
            EvidenceClass::DirectEntry,
            EvidenceClass::RelatedEntry,
            EvidenceClass::EntryMention,
            EvidenceClass::ContextMention
        ]
    );
    assert_eq!(result.counts.direct_entry.total, 2);
    for class in [
        EvidenceClass::RelatedEntry,
        EvidenceClass::EntryMention,
        EvidenceClass::ContextMention,
    ] {
        assert_eq!(result.counts.get(class).total, 1);
    }
    let related = &result.evidence[2];
    assert!(related.bases.contains(&EvidenceBasis::Literal));
    assert!(
        related
            .bases
            .iter()
            .any(|b| matches!(b, EvidenceBasis::Related { .. }))
    );
    assert_eq!(related.previews.len(), 1);
    let mention = &result.evidence[3];
    assert_eq!(mention.entry.as_ref().unwrap().names, ["-Q"]);
    assert_eq!(mention.bases, [EvidenceBasis::Literal]);
    let mut tiny = query();
    tiny.options.content_bytes = 1;
    let omitted = explain_query(&content, &tiny).unwrap();
    assert_eq!(omitted.counts, result.counts);
    for (full, bounded) in result.evidence.iter().zip(&omitted.evidence) {
        assert_eq!(bounded.class, full.class);
        assert!(bounded.content_omitted);
        assert!(bounded.previews_omitted);
        assert!(bounded.entry.is_none());
    }
    tiny.options.offset = 2;
    let later = explain_query(&content, &tiny).unwrap();
    assert_eq!(later.counts.direct_entry.returned, 0);
    assert!(
        mant_engine::render_explanation_text(&later)
            .contains("Direct entries exist but are not included on this page")
    );
}

#[test]
fn late_direct_and_related_evidence_displace_earlier_mentions_at_the_cap() {
    let mut source = "--help mentioned.\n\n".repeat(10_001);
    source.push_str("<!-- mant:entries role=option case=sensitive -->\n- `--assist`: Related. <!-- mant:entry {\"id\":\"assist\",\"aliasOf\":\"help\"} -->\n- `--help`: Actual help. <!-- mant:entry {\"id\":\"help\"} -->\n");
    let content = query_markdown_text(&source, None).unwrap();
    let result = explain_query(&content, &query()).unwrap();
    assert_eq!(result.total, 10_000);
    assert!(result.truncation.candidates);
    assert_eq!(result.evidence[0].class, EvidenceClass::DirectEntry);
    assert_eq!(result.evidence[1].class, EvidenceClass::RelatedEntry);
    assert_eq!(result.counts.context_mention.total, 9998);
    assert_eq!(result.evidence[2].previews[0].source.unwrap().line, 1);
}

#[test]
fn empty_names_still_have_a_real_owner_and_invalid_bindings_never_match_names() {
    use mant_ir::visit::{self, VisitMut};
    struct Invalidate;
    impl VisitMut for Invalidate {
        fn visit_list_item_mut(&mut self, item: &mut mant_ir::ListItem) {
            if let Some(facts) = &mut item.entry {
                facts.name_bindings[0].occurrences.clear();
            }
            visit::walk_list_item_mut(self, item);
        }
    }
    struct NoNames;
    impl VisitMut for NoNames {
        fn visit_definition_item_mut(&mut self, item: &mut mant_ir::DefinitionItem) {
            if let Some(facts) = &mut item.entry {
                facts.names.clear();
                facts.name_bindings.clear();
            }
            visit::walk_definition_item_mut(self, item);
        }
    }
    let mut content = query_markdown_text(
        "<!-- mant:entries role=option case=sensitive -->\n- `--help`: Read --help.\n",
        None,
    )
    .unwrap();
    Invalidate.visit_document_mut(content.document.as_mut().unwrap());
    let result = explain_query(&content, &query()).unwrap();
    assert!(!result.semantics_complete);
    assert_eq!(result.evidence[0].class, EvidenceClass::DirectEntry);
    assert!(
        matches!(&result.evidence[0].bases[..], [EvidenceBasis::Form { matches }, EvidenceBasis::Literal]
        if matches.len() == 1 && matches[0].text == "--help")
    );
    assert!(result.evidence[0].entry.as_ref().unwrap().names.is_empty());
    let mut content = mant_engine::query_roff_bytes(
        b".TH PROBE 1\n.SH DESCRIPTION\n.TP\n.B A\nRead --help here.\n",
    )
    .unwrap();
    NoNames.visit_document_mut(content.document.as_mut().unwrap());
    let result = explain_query(&content, &query()).unwrap();
    assert_eq!(result.evidence[0].class, EvidenceClass::EntryMention);
    assert!(result.evidence[0].entry.as_ref().unwrap().names.is_empty());
}

#[test]
fn unrecorded_or_invalid_forms_preserve_literal_ownership_and_nested_entries() {
    use mant_ir::{Block, EntryForms, EntryOwner};
    for invalid in [false, true] {
        let mut content = query_markdown_text(
            "<!-- mant:entries role=command case=sensitive -->\n- `run`: Read TOKEN here.\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: CHILD.\n", None,
        ).unwrap();
        let Block::List { items, .. } = &mut content.document.as_mut().unwrap().blocks[0] else {
            panic!("list")
        };
        let parent = &mut items[0];
        let facts = parent.entry.as_mut().unwrap();
        let id = facts.id.clone();
        if invalid {
            facts.forms[0].parts[0].path = vec![usize::MAX];
        } else {
            facts.forms.clear();
        }
        let owner = EntryOwner::List(parent);
        assert_eq!(owner.forms().is_none(), invalid);
        if !invalid {
            assert!(matches!(owner.forms(), Some(EntryForms::Unrecorded)));
        }
        let evidence = mant_engine::select_explanation(&content, "TOKEN").unwrap();
        assert_eq!(evidence.total, 1);
        assert_eq!(evidence.evidence[0].outline.node.id(), id.as_str());
        assert!(
            evidence.evidence[0]
                .entry
                .as_ref()
                .unwrap()
                .forms
                .is_empty()
        );
        assert!(
            evidence.evidence[0]
                .entry
                .as_ref()
                .unwrap()
                .names
                .is_empty()
        );
        assert!(mant_engine::select_excerpt(&content, &[id.as_str()]).is_ok());
        assert!(mant_engine::select_excerpt(&content, &["run"]).is_err());
        assert!(mant_engine::select_excerpt(&content, &["root/e1/e1"]).is_ok());
        let index = mant_ir::SemanticIndex::build(content.document.as_ref().unwrap());
        assert_eq!(index.root()[0].children.len(), 1);
    }
}
