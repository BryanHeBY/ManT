//! Independent evidence oracle: multiple owners are not navigation ambiguity.
use mant_engine::{explain_query, query_markdown_text};
use mant_protocol::{EvidenceBasis, ExplanationOptions, ExplanationOutcome, ExplanationQuery};

fn query(entry: &str) -> ExplanationQuery {
    ExplanationQuery {
        entry: entry.into(),
        options: ExplanationOptions::default(),
    }
}

#[test]
fn independent_owners_and_literal_support_survive_without_selector_shadowing() {
    let input = "# Probe\n\nUse `--help` for guidance.\n\n<!-- mant:entries role=option case=sensitive -->\n- `--help`: General help.\n- `--help=CLASS`: Class help.\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `--help`: Nested value.\n";
    let content = query_markdown_text(input, None).unwrap();
    let before = content.clone();
    let found = explain_query(&content, &query("--help")).unwrap();
    assert_eq!(found.outcome, ExplanationOutcome::Evidence);
    assert_eq!(found.total, 4);
    assert!(found.evidence[3].entry.is_none());
    assert!(found.evidence[3].block_path.is_some());
    assert_eq!(found.evidence[0].entry.as_ref().unwrap().names, ["--help"]);
    assert_eq!(found.evidence[1].entry.as_ref().unwrap().names, ["--help"]);
    assert_eq!(found.evidence[2].entry.as_ref().unwrap().names, ["--help"]);
    assert!(
        found.evidence[0]
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
    );
    assert!(
        found.evidence[0]
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Form { .. }))
    );
    assert!(mant_engine::select_excerpt(&content, &["--help"]).is_err());
    for evidence in &found.evidence[..3] {
        assert!(mant_engine::select_excerpt(&content, &[evidence.outline.path()]).is_ok());
    }
    assert_eq!(before, content);
    assert_eq!(
        serde_json::from_str::<mant_protocol::QueryExplanation>(
            &serde_json::to_string(&found).unwrap()
        )
        .unwrap(),
        found
    );
}

#[test]
fn explicit_relationships_add_independent_content_not_inherited_domains() {
    let source = r#"# Probe
<!-- mant:entries role=option case=sensitive -->
- `--data-ascii DATA`: ASCII only. <!-- mant:entry {"id":"ascii","aliasOf":"data"} -->
- `-d DATA`, `--data DATA`: Submit. <!-- mant:entry {"id":"data","aliasGroups":[["-d","--data"]]} -->

  <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->

- `-S`, `--since`, `-U`, `--until`: Independent bounds. <!-- mant:entry {"id":"bounds","aliasGroups":[["-S","--since"],["-U","--until"]]} -->
"#;
    let content = query_markdown_text(source, None).unwrap();
    assert!(content.document.as_ref().unwrap().diagnostics.is_empty());
    let found = explain_query(&content, &query("--data")).unwrap();
    assert_eq!(found.total, 2);
    assert!(found.evidence[1].bases.iter().any(|b| matches!(b, EvidenceBasis::Related { from, declarations } if from.as_str() == "data" && declarations.iter().map(mant_ir::NodeId::as_str).collect::<Vec<_>>() == ["ascii"])));
    assert!(
        found.evidence[1]
            .entry
            .as_ref()
            .unwrap()
            .value_domain
            .is_none()
    );
    assert!(
        found.evidence[0]
            .entry
            .as_ref()
            .unwrap()
            .value_domain
            .is_some()
    );
    assert!(
        found.evidence[0]
            .bases
            .contains(&EvidenceBasis::AliasGroup {
                members: vec!["-d".into(), "--data".into()]
            })
    );
    let reverse = explain_query(&content, &query("--data-ascii")).unwrap();
    assert_eq!(reverse.total, 2);
    assert!(
        reverse.evidence[1]
            .bases
            .iter()
            .any(|b| matches!(b, EvidenceBasis::Related { .. }))
    );
    assert_eq!(explain_query(&content, &query("--since")).unwrap().total, 1);
}

#[test]
fn form_case_literal_and_no_evidence_are_distinct() {
    let content = query_markdown_text("<!-- mant:entries role=option case=sensitive -->\n- `-I DIR`: Include uppercase.\n- `-i`: Lowercase.\n- `--all`: All.\n\nTOKEN only in ordinary prose.\n", None).unwrap();
    let upper = explain_query(&content, &query("-I")).unwrap();
    let lower = explain_query(&content, &query("-i")).unwrap();
    assert_ne!(
        upper.evidence[0].outline.node.id(),
        lower.evidence[0].outline.node.id()
    );
    let form = explain_query(&content, &query("-I DIR")).unwrap();
    assert!(
        form.evidence[0]
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Form { .. }))
    );
    assert!(
        !form.evidence[0]
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
    );
    let missing = explain_query(&content, &query("-a")).unwrap();
    assert_eq!(missing.outcome, ExplanationOutcome::NoEvidence);
    assert_eq!(missing.total, 0);
    let prose = explain_query(&content, &query("TOKEN")).unwrap();
    assert_eq!(prose.total, 1);
    assert!(prose.evidence[0].entry.is_none());
    assert_eq!(prose.evidence[0].bases, [EvidenceBasis::Literal]);
}

#[test]
fn paging_and_body_omission_are_independent_of_outcome() {
    let content = query_markdown_text("<!-- mant:entries role=option case=sensitive -->\n- `--help`: First.\n- `--help=KIND`: Second.\n", None).unwrap();
    let mut request = query("--help");
    request.options = ExplanationOptions {
        limit: 1,
        offset: 0,
        content_bytes: 1,
    };
    let first = explain_query(&content, &request).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.next_offset, Some(1));
    assert!(first.truncation.content);
    assert!(first.evidence[0].content.is_none());
    assert!(first.evidence[0].content_omitted && first.evidence[0].details_omitted);
    request.options.offset = 1;
    let second = explain_query(&content, &request).unwrap();
    assert_eq!(second.evidence[0].ordinal, 1);
    assert_ne!(
        first.evidence[0].outline.node.id(),
        second.evidence[0].outline.node.id()
    );
    request.options.offset = u32::MAX;
    let end = explain_query(&content, &request).unwrap();
    assert_eq!(end.outcome, ExplanationOutcome::Evidence);
    assert_eq!(end.returned, 0);
    assert_eq!(end.next_offset, None);
    for entry in ["", "  ", "bad\nvalue", &"界".repeat(513)] {
        assert!(explain_query(&content, &query(entry)).is_err());
    }
    request.options.limit = 257;
    assert!(explain_query(&content, &request).is_err());
}

#[test]
fn native_and_markdown_owners_share_the_same_evidence_rules() {
    let content = mant_engine::query_roff_bytes(
        b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --help\nGeneral.\n.TP\n.B --help=CLASS\nSpecific.\n",
    )
    .unwrap();
    let found = explain_query(&content, &query("--help")).unwrap();
    assert_eq!(found.total, 2);
    for result in &found.evidence {
        assert!(
            result
                .bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        );
        assert_eq!(result.class, mant_protocol::EvidenceClass::DirectEntry);
        assert!(result.previews.is_empty());
        assert!(!result.previews_omitted);
        assert!(matches!(
            result.content,
            Some(mant_protocol::ExplanationContent::Entry { .. })
        ));
    }
}

#[test]
fn public_ir_invalid_relations_do_not_traverse_or_claim_complete_semantics() {
    use mant_ir::visit::{self, VisitMut};
    struct BreakRelation;
    impl VisitMut for BreakRelation {
        fn visit_list_item_mut(&mut self, item: &mut mant_ir::ListItem) {
            if let Some(facts) = &mut item.entry {
                facts.alias_of = Some(facts.id.clone());
            }
            visit::walk_list_item_mut(self, item);
        }
    }
    let mut content = query_markdown_text(
        "<!-- mant:entries role=option case=sensitive -->\n- `--help`: Help.\n",
        None,
    )
    .unwrap();
    BreakRelation.visit_document_mut(content.document.as_mut().unwrap());
    let result = explain_query(&content, &query("--help")).unwrap();
    assert_eq!(result.total, 1);
    assert!(!result.semantics_complete);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("ir.invalid-entry-alias-of"))
    );
    assert!(result.evidence.iter().all(|e| {
        !e.bases
            .iter()
            .any(|b| matches!(b, EvidenceBasis::Related { .. }))
    }));
}

#[test]
fn relation_depth_and_matching_owner_caps_report_incomplete_collection() {
    use std::fmt::Write;
    let mut source = "<!-- mant:entries role=option case=sensitive -->\n".to_owned();
    for index in 0..40 {
        writeln!(
            source,
            "- `--n{index}`: Body. <!-- mant:entry {{\"id\":\"n{index}\",\"aliasOf\":\"n{}\"}} -->",
            index + 1
        )
        .unwrap();
    }
    source.push_str("- `--n40`: Body. <!-- mant:entry {\"id\":\"n40\"} -->\n");
    let content = query_markdown_text(&source, None).unwrap();
    assert!(content.document.as_ref().unwrap().diagnostics.is_empty());
    let result = explain_query(&content, &query("--n0")).unwrap();
    assert_eq!(result.total, 33);
    assert!(result.truncation.relations);
    assert!(result.evidence.iter().all(|e| e.bases.iter().all(
        |b| !matches!(b, EvidenceBasis::Related { declarations, .. } if declarations.len() > 32)
    )));
    let content = query_markdown_text(&"TOKEN.\n\n".repeat(10_001), None).unwrap();
    let result = explain_query(&content, &query("TOKEN.")).unwrap();
    assert_eq!(result.total, 10_000);
    assert!(result.truncation.candidates);
    assert_eq!(result.returned, 50);
}
