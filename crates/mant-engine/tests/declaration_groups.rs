//! A physical owner and its useful explanation are independent contracts.
use mant_engine::{explain_query, query_roff_bytes};
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery};

fn explained(source: &str, name: &str) -> mant_protocol::QueryExplanation {
    explain_query(
        &query_roff_bytes(source.as_bytes()).unwrap(),
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap()
}

#[test]
fn declaration_boundaries_follow_executed_requests_not_physical_source_lines() {
    for (between, grouped) in [
        ("", true),
        (".PP\n", false),
        (".if n .PP\n", false),
        (".if 1 .PP\n", false),
        (".if 0 .PP\n", true),
        (".if 0 \\{\\\n.PP\n.\\}\n", true),
        (".de UN\n.PP\n..\n", true),
        (".BREAK\n", false),
        (".ie 0 .PP\n.el .PP\n", false),
        (".ie 1 .PD 0\n.el .PP\n", true),
        (".if 1 .if 1 .PP\n", false),
        (".if 1 .if 0 .PP\n", true),
    ] {
        let source = format!(
            ".TH PROBE 1\n.de BREAK\n.PP\n..\n.SH OPTIONS\n.TP\n.B -a\n{between}.TP\n.B -b\nShared description.\n"
        );
        let result = explained(&source, "-a");
        assert_eq!(!result.supports.is_empty(), grouped, "{between:?}");
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source.as_bytes())
            .unwrap();
        let detached = mant_engine::lower_mandoc_document(std::path::Path::new("probe.1"), &native);
        let detached = mant_ir::ResolvedContent {
            label: "probe".into(),
            address: None,
            document: Some(detached),
            tldr: None,
        };
        let detached = explain_query(
            &detached,
            &ExplanationQuery {
                entry: "-a".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(
            !detached.supports.is_empty(),
            grouped,
            "detached {between:?}"
        );
    }
}

#[test]
fn nested_group_context_is_a_source_reference_not_a_second_body_copy() {
    let body = "unique_body_word ".repeat(1600);
    let source = format!(
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a\n.It Fl b\nOuter description.\n.Bl -tag -width Ds\n.It Fl a\n.It Fl c\n{body}\n.El\n.El\n"
    );
    let content = query_roff_bytes(source.as_bytes()).unwrap();
    for bytes in [32768, 40000, 65536] {
        let response = explain_query(
            &content,
            &ExplanationQuery {
                entry: "-a".into(),
                options: ExplanationOptions {
                    content_bytes: bytes,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        assert_eq!(response.counts.direct_entry.returned, 2);
        assert_eq!(response.supports.len(), 2);
        assert!(
            response
                .evidence
                .iter()
                .all(|e| !e.support_omitted && !e.content_omitted),
            "both bodies fit budget {bytes}"
        );
        if bytes >= 40000 {
            assert!(!response.truncation.content);
        }
        assert!(
            response
                .evidence
                .iter()
                .all(|e| e.covered_by_support(&response.supports))
        );
        response.validate_references().unwrap();
        let encoded = serde_json::to_string(&response).unwrap();
        assert_eq!(encoded.matches(body.trim()).count(), 1);
        let decoded: mant_protocol::QueryExplanation = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            mant_engine::render_explanation_text(&decoded)
                .matches(body.trim())
                .count(),
            1
        );
        assert_eq!(
            mant_engine::render_explanation_markdown(&decoded)
                .matches(body.trim())
                .count(),
            1
        );
    }
    for offset in [0, 1] {
        let response = explain_query(
            &content,
            &ExplanationQuery {
                entry: "-a".into(),
                options: ExplanationOptions {
                    offset,
                    limit: 1,
                    content_bytes: 40000,
                },
            },
        )
        .unwrap();
        assert_eq!(response.supports.len(), 1);
        assert!(
            response.supports[0].items().is_some(),
            "each isolated page owns its own context"
        );
        let text = mant_engine::render_explanation_text(&response);
        assert_eq!(text.matches(body.trim()).count(), 1);
        if offset == 1 {
            assert!(!text.contains("Outer description"));
        }
    }
}

#[test]
fn ordinary_parent_reuses_nested_owners_and_groups_by_source_location() {
    for head in [".It Fl a\n", ".It Fl a\n.It Fl c\n"] {
        let source = format!(
            ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a\nOuter description.\n.Bl -tag -width Ds\n{head}Unique nested café 日本 body.\n.El\n.El\n"
        );
        let response = explained(&source, "-a");
        assert_eq!(response.counts.direct_entry.returned, 2);
        response.validate_references().unwrap();
        assert_eq!(
            serde_json::to_string(&response)
                .unwrap()
                .matches("Unique nested café 日本 body.")
                .count(),
            1
        );
        for reversed in [false, true] {
            let mut response = response.clone();
            if reversed {
                response.evidence.reverse();
            }
            let wire = serde_json::to_string(&response).unwrap();
            let decoded: mant_protocol::QueryExplanation = serde_json::from_str(&wire).unwrap();
            for text in [
                mant_engine::render_explanation_text(&decoded),
                mant_engine::render_explanation_markdown(&decoded),
            ] {
                assert_eq!(
                    text.matches("Unique nested café 日本 body.").count(),
                    1,
                    "{text}"
                );
                assert!(text.contains("Outer description"));
            }
        }
        // Even valid pool indices and resolvable paths must not authorize
        // a different physical owner or rewritten forms.
        let value = serde_json::to_value(&response).unwrap();
        for mode in 0..5 {
            let mut bad = value.clone();
            match mode {
                0 => bad["evidence"][0]["content"]["itemIndex"] = 1.into(),
                1 => {
                    bad["evidence"][0]["content"]["path"] =
                        serde_json::json!([{"kind":"definition-item","index":0}]);
                }
                2 => bad["evidence"][0]["entry"]["forms"] = serde_json::json!([]),
                3 => bad["evidence"][0]["class"] = "entry-mention".into(),
                _ => bad["evidence"][0]["contentOmitted"] = true.into(),
            }
            assert!(
                serde_json::from_value::<mant_protocol::QueryExplanation>(bad).is_err(),
                "accepted mutation {mode}"
            );
        }
    }
}

#[test]
fn deep_shared_fragments_are_deterministic_at_small_copy_budgets() {
    let mut source = String::from(".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n");
    for _ in 0..12 {
        source.push_str(".Bl -tag -width Ds\n.It Fl a\nLayer body.\n");
    }
    source.push_str("Unique leaf café 日本.\n");
    source.push_str(&".El\n".repeat(12));
    let content = query_roff_bytes(source.as_bytes()).unwrap();
    for bytes in [600, 2000, 5000, 12000, 32768, 65536] {
        let query = ExplanationQuery {
            entry: "-a".into(),
            options: ExplanationOptions {
                content_bytes: bytes,
                ..ExplanationOptions::default()
            },
        };
        let expected = explain_query(&content, &query).unwrap();
        expected.validate_references().unwrap();
        for _ in 0..4 {
            assert_eq!(
                explain_query(&content, &query).unwrap(),
                expected,
                "budget {bytes}"
            );
        }
        if !expected.truncation.content {
            assert_eq!(
                serde_json::to_string(&expected)
                    .unwrap()
                    .matches("Unique leaf café 日本.")
                    .count(),
                1
            );
            assert_eq!(
                mant_engine::render_explanation_text(&expected)
                    .matches("Unique leaf café 日本.")
                    .count(),
                1
            );
        }
    }
}

#[test]
fn valid_pool_indices_do_not_authorize_wrong_group_or_wrong_evidence_class() {
    let response = explained(
        ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -a\n.TP\n.B -b\nFIRST_CONTEXT\n.TP\n.B -a\n.TP\n.B -c\nSECOND_CONTEXT\n",
        "-a",
    );
    assert_eq!(response.supports.len(), 2);
    for mode in 0..4 {
        let mut altered = response.clone();
        let first = &mut altered.evidence[0];
        match mode {
            0 => first.support = Some(1),
            1 => {
                first.support = Some(1);
                first.content = Some(mant_protocol::ExplanationContent::Entry {
                    block: mant_ir::Block::DefinitionList {
                        items: vec![response.supports[0].items().unwrap()[0].clone()],
                        declaration_groups: vec![],
                        compact: false,
                        layout: mant_ir::LayoutHint::default(),
                        source: None,
                    },
                });
            }
            2 => {
                first.support = Some(1);
                first.content = None;
                first.content_omitted = true;
            }
            _ => first.class = EvidenceClass::EntryMention,
        }
        assert!(altered.validate_references().is_err(), "mode {mode}");
        assert!(
            serde_json::from_str::<mant_protocol::QueryExplanation>(
                &serde_json::to_string(&altered).unwrap()
            )
            .is_err()
        );
        altered.evidence.truncate(1);
        assert!(
            !mant_engine::render_explanation_text(&altered).contains("SECOND_CONTEXT"),
            "in-memory invalid reference mode {mode}"
        );
    }
}

#[test]
fn provider_fallback_is_reused_when_its_complete_declaration_group_does_not_fit() {
    let body = "unique_inner_body_word ".repeat(300);
    let source = format!(
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl b Ar {}\n.It Fl a\nOuter description.\n.Bl -tag -width Ds\n.It Fl a\n.It Fl c\n{body}\n.El\n.El\n",
        "X".repeat(15000)
    );
    let content = query_roff_bytes(source.as_bytes()).unwrap();
    for bytes in [12000, 18000, 20000, 24000, 30000] {
        let response = explain_query(
            &content,
            &ExplanationQuery {
                entry: "-a".into(),
                options: ExplanationOptions {
                    content_bytes: bytes,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        assert_eq!(response.counts.direct_entry.returned, 2);
        assert!(
            !response.evidence[1].support_omitted,
            "inner group already present at {bytes}"
        );
        response.validate_references().unwrap();
        assert_eq!(
            serde_json::to_string(&response)
                .unwrap()
                .matches(body.trim())
                .count(),
            1
        );
        for reversed in [false, true] {
            let mut response = response.clone();
            if reversed {
                response.evidence.reverse();
            }
            let text = mant_engine::render_explanation_text(&response);
            assert!(text.contains("Outer description"));
            assert_eq!(text.matches(body.trim()).count(), 1);
            assert_eq!(
                mant_engine::render_explanation_markdown(&response)
                    .matches(body.trim())
                    .count(),
                1
            );
        }
    }
}

#[test]
fn group_highlights_only_the_matched_member_in_offline_presentation() {
    let original = explained(
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a\n.It Fl b\nUnicode café 日本.\n.Lk https://example.org More\n.Pp\nTrailing paragraph.\n.El\n",
        "-a",
    );
    let response: mant_protocol::QueryExplanation =
        serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
    let runs = std::cell::RefCell::new(Vec::new());
    let text = mant_engine::render_explanation_text_with(&response, |style, text| {
        runs.borrow_mut().push((style, text.to_owned()));
        text.to_owned()
    });
    assert_eq!(text, mant_engine::render_explanation_text(&original));
    assert!(text.contains("café 日本") && text.contains("Trailing paragraph"));
    let runs = runs.into_inner();
    let matched: String = runs
        .iter()
        .filter(|(s, _)| s.matched)
        .map(|(_, t)| t.as_str())
        .collect();
    assert_eq!(matched, "-a");
    let other: String = runs
        .iter()
        .filter(|(s, _)| !s.matched && s.inline.entry_kind.is_some())
        .map(|(_, t)| t.as_str())
        .collect();
    assert_eq!(other, "-b");
    assert!(
        runs.iter()
            .any(|(s, t)| s.inline.link && !s.matched && t == "More")
    );
}

#[test]
fn invalid_group_heads_and_overlapping_ranges_are_not_semantically_complete() {
    let source = ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.TP\n.B --last\nBody.\n";
    for empty_head in [false, true] {
        let mut content = query_roff_bytes(source.as_bytes()).unwrap();
        let document = content.document.as_mut().unwrap();
        let mant_ir::Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = &mut document.sections[0].blocks[0]
        else {
            panic!("definition list")
        };
        if empty_head {
            items[0].terms = vec![vec![mant_ir::Inline::anchor("only-anchor")]];
        } else {
            declaration_groups.push(declaration_groups[0]);
        }
        let response = explain_query(
            &content,
            &ExplanationQuery {
                entry: "--first".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert!(!response.semantics_complete);
        assert!(
            response
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("ir.invalid-declaration-group"))
        );
    }
}

#[test]
fn large_context_is_omitted_atomically_without_retrying_each_match() {
    use std::fmt::Write;
    let mut source = String::from(".TH PROBE 1\n.SH OPTIONS\n");
    for index in 0..300 {
        writeln!(source, ".TP\n.B --mode={index}").unwrap();
    }
    source.push_str(&"Large original context.\n".repeat(4000));
    let content = query_roff_bytes(source.as_bytes()).unwrap();
    for bytes in [1, 1024, 4_194_304] {
        let result = explain_query(
            &content,
            &ExplanationQuery {
                entry: "--mode".into(),
                options: ExplanationOptions {
                    limit: 256,
                    content_bytes: bytes,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.returned, 256);
        assert!(
            result.supports.is_empty(),
            "group member bound is independent of bytes"
        );
        assert!(result.evidence.iter().all(|e| e.support_omitted));
        assert!(result.truncation.content);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("Large original context")
        );
        result.validate_references().unwrap();
    }
}

#[test]
fn scope_supports_are_document_local_even_when_node_ids_coincide() {
    use mant_protocol::{ScopeExplanation, ScopedExplanation, ScopedExplanationEvidence};
    let first = explained(
        ".TH FIRST 1\n.SH OPTIONS\n.TP\n.B -a\n.TP\n.B -b\nFirst context.\n",
        "-a",
    );
    let second = explained(
        ".TH SECOND 1\n.SH OPTIONS\n.TP\n.B -a\n.TP\n.B -c\nSecond context.\n",
        "-a",
    );
    assert_eq!(
        first.evidence[0].outline.node.id(),
        second.evidence[0].outline.node.id()
    );
    let documents = [&first, &second]
        .into_iter()
        .enumerate()
        .map(|(i, r)| ScopedExplanation {
            supports: r.supports.clone(),
            address: mant_ir::DocumentAddress::Manual {
                name: format!("probe-{i}"),
                manual_section: "1".into(),
            },
            depth: 0,
            label: r.label.clone(),
            producer: None,
            diagnostics: r.diagnostics.clone(),
            semantics_complete: r.semantics_complete,
            outcome: r.outcome,
            total: r.total,
            returned: r.returned,
            counts: r.counts,
            truncation: r.truncation,
        })
        .collect();
    let mut counts = first.counts;
    counts.direct_entry.total = 2;
    counts.direct_entry.returned = 2;
    let result = ScopeExplanation {
        order: first.order,
        counts,
        query: first.query.clone(),
        outcome: first.outcome,
        total: 2,
        returned: 2,
        next_offset: None,
        truncation: first.truncation,
        documents,
        evidence: vec![
            ScopedExplanationEvidence {
                document_index: 0,
                evidence: first.evidence[0].clone(),
            },
            ScopedExplanationEvidence {
                document_index: 1,
                evidence: second.evidence[0].clone(),
            },
        ],
        failures: Vec::new(),
    };
    let wire = serde_json::to_value(&result).unwrap();
    let decoded: ScopeExplanation = serde_json::from_value(wire.clone()).unwrap();
    let rendered = mant_engine::render_scope_explanation_text(&decoded);
    assert_eq!(rendered.matches("First context").count(), 1);
    assert_eq!(rendered.matches("Second context").count(), 1);
    for mode in 0..4 {
        let mut invalid = wire.clone();
        let e = &mut invalid["evidence"][0]["evidence"];
        match mode {
            0 => e["class"] = "entry-mention".into(),
            1 => e["supportOmitted"] = true.into(),
            2 => {
                e["content"] = serde_json::Value::Null;
                e["contentOmitted"] = true.into();
            }
            _ => e["content"]["itemIndex"] = 1.into(),
        }
        assert!(
            serde_json::from_value::<ScopeExplanation>(invalid).is_err(),
            "scoped cross-field mutation {mode}"
        );
    }
    // Another document's pool cannot satisfy a dangling reference in this one.
    let mut invalid = wire;
    invalid["documents"][1]["supports"] = serde_json::json!([]);
    assert!(serde_json::from_value::<ScopeExplanation>(invalid).is_err());
}

#[test]
fn native_boundaries_stop_context_and_unclosed_heads_stay_independent() {
    for barrier in [
        ".PP\n",
        ".sp\n",
        ".SH OTHER\n",
        "Intervening visible text.\n",
        ".RS 4\n.TP\n.B --nested\nNested body.\n.RE\n",
    ] {
        let source = format!(
            ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n{barrier}.TP\n.B --second\nSecond body.\n"
        );
        let response = explained(&source, "--first");
        assert!(response.supports.is_empty(), "crossed {barrier:?}");
    }
    let response = explained(
        ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.TP\n.B --last\n",
        "--first",
    );
    assert!(response.supports.is_empty());
}

#[test]
fn different_behaviors_share_context_without_alias_or_extra_direct_matches() {
    let response = explained(
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl a\n.It Fl b\nOnly b changes the output.\n.El\n",
        "-a",
    );
    assert_eq!(response.counts.direct_entry.total, 1);
    assert_eq!(response.counts.related_entry.total, 0);
    assert_eq!(response.supports.len(), 1);
    let text = mant_engine::render_explanation_text(&response);
    assert!(text.contains("Only b changes the output."));
    assert!(text.contains("Source of description: -b;"));
}

#[test]
fn consecutive_declarations_supply_context_without_borrowing_ownership() {
    for source in [
        ".TH PROBE 1\n.SH OPTIONS\n.IP \"-x language\" 4\n.PD 0\n.IP \"--language=language\" 4\n.PD\nChoose the source language.\n.PP\nTrailing language note.\n.IP \"-x none\" 4\nDisable language selection.\n",
        ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --init-file\n.PD 0\n.TP\n.B --rcfile\n.PD\nRead the startup file.\n",
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl 1\n.It Fl 2\n.It Fl 9\nCompression levels differ in speed and size.\n.El\n",
    ] {
        let name = if source.contains("-x language") {
            "-x"
        } else if source.contains("--init-file") {
            "--init-file"
        } else {
            "-2"
        };
        let mut content = query_roff_bytes(source.as_bytes()).unwrap();
        // Query after an IR wire round trip: support must not depend on native
        // pointers or the engine's operation-local recognition witnesses.
        content.document = Some(
            serde_json::from_value(
                serde_json::to_value(content.document.as_ref().unwrap()).unwrap(),
            )
            .unwrap(),
        );
        let response = explain_query(
            &content,
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions {
                    limit: 1,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        let first = &response.evidence[0];
        assert_eq!(first.class, EvidenceClass::DirectEntry);
        assert!(first.entry.as_ref().unwrap().alias_groups.is_empty());
        let json = serde_json::to_value(&response).unwrap();
        let owner = first
            .content
            .as_ref()
            .unwrap()
            .referenced_owner(&response.supports)
            .unwrap();
        assert!(
            matches!(owner, mant_ir::EntryOwner::Definition(item) if item.description.is_empty())
        );
        assert!(first.covered_by_support(&response.supports));
        assert!(
            json["supports"].as_array().is_some_and(|v| !v.is_empty()),
            "{name}: missing declaration-group explanation"
        );
        let decoded: mant_protocol::QueryExplanation = serde_json::from_value(json).unwrap();
        let text = mant_engine::render_explanation_text(&decoded);
        assert!(text.contains("Declaration-group context"), "{text}");
        assert!(!text.contains("no independent description"), "{text}");
        assert_eq!(text, mant_engine::render_explanation_text(&response));
        assert_eq!(
            mant_engine::render_explanation_markdown(&decoded),
            mant_engine::render_explanation_markdown(&response)
        );
    }
}

#[test]
fn shared_context_is_copied_once_with_valid_owner_local_positions() {
    let response = explained(
        ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --mode=A\n.TP\n.B --mode=B\nShared body with café and a trailing note.\n",
        "--mode",
    );
    assert_eq!(response.counts.direct_entry.returned, 2);
    assert_eq!(response.supports.len(), 1);
    assert!(
        response
            .evidence
            .iter()
            .all(|e| e.covered_by_support(&response.supports))
    );
    response.validate_references().unwrap();
    let json = serde_json::to_string(&response).unwrap();
    assert_eq!(json.matches("Shared body with").count(), 1);
    let decoded: mant_protocol::QueryExplanation = serde_json::from_str(&json).unwrap();
    let text = mant_engine::render_explanation_text(&decoded);
    assert_eq!(text.matches("Shared body with").count(), 1);
    assert!(!text.contains("Forms:") && !text.contains("Definition:"));
    assert!(text.contains("see support 0"));
    for (index, e) in decoded.evidence.iter().enumerate() {
        let mant_protocol::ExplanationContent::DeclarationMember { item_index, .. } =
            e.content.as_ref().unwrap()
        else {
            panic!()
        };
        assert_eq!(*item_index, index);
        for basis in &e.bases {
            if let mant_protocol::EvidenceBasis::Name { matches } = basis {
                for occurrence in matches.iter().flat_map(|m| &m.occurrences) {
                    assert!(!occurrence.content.is_empty());
                    for range in &occurrence.content {
                        let root = e
                            .content
                            .as_ref()
                            .unwrap()
                            .resolve_range(&decoded.supports, range)
                            .unwrap();
                        assert!(
                            root.safe_text()
                                .contains(if index == 0 { "=A" } else { "=B" })
                        );
                    }
                }
            }
        }
    }
    let value = serde_json::to_value(&decoded).unwrap();
    for path in ["missing", "wrong-owner", "range", "position"] {
        let mut bad = value.clone();
        match path {
            "missing" => bad["evidence"][0]["content"]["support"] = 99.into(),
            "wrong-owner" => bad["evidence"][0]["content"]["itemIndex"] = 1.into(),
            "range" => bad["supports"][0]["group"]["endItem"] = 999.into(),
            _ => {
                bad["evidence"][0]["bases"][0]["matches"][0]["occurrences"][0]["content"][0]["endChar"] =
                    999.into();
            }
        }
        assert!(
            serde_json::from_value::<mant_protocol::QueryExplanation>(bad).is_err(),
            "accepted {path}"
        );
    }
}

#[test]
fn support_omission_is_explicit_and_each_page_carries_its_context() {
    let source = ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --mode=A\n.TP\n.B --mode=B\nShared context.\n";
    let content = query_roff_bytes(source.as_bytes()).unwrap();
    for offset in 0..2 {
        for bytes in [1, 200, 2000, 16_384] {
            let response = explain_query(
                &content,
                &ExplanationQuery {
                    entry: "--mode".into(),
                    options: ExplanationOptions {
                        offset,
                        limit: 1,
                        content_bytes: bytes,
                    },
                },
            )
            .unwrap();
            assert_eq!(response.returned, 1);
            let e = &response.evidence[0];
            assert_eq!(e.ordinal, offset);
            assert!(e.covered_by_support(&response.supports) || e.support_omitted);
            if e.support_omitted {
                assert!(response.truncation.content);
            }
            if bytes == 16_384 {
                assert!(e.covered_by_support(&response.supports));
            }
            response.validate_references().unwrap();
            assert!(
                !mant_engine::render_explanation_text(&response)
                    .contains("no independent description")
            );
        }
    }
}

#[test]
fn explicit_tq_is_one_owner_inside_a_reading_group() {
    let response = explained(
        ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.TP\n.B --second\n.TQ\n.B --third\nFinal explanation.\n",
        "--first",
    );
    assert_eq!(response.supports.len(), 1);
    let items = response.supports[0].items().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].terms.len(), 2);
    assert_eq!(response.counts.direct_entry.total, 1);
    assert_eq!(response.counts.related_entry.total, 0);
}

#[test]
fn empty_ip_after_nested_options_keeps_tail_notes_in_the_provider() {
    let source = ".TH PROBE 1\n.SH COMMANDS\n.TP\n.B first\n.TP\n.B second\nOpening description.\n.RS\n.TP\n.B -c\nNested callback option.\n.RE\n.IP\nCallback timing note.\n.IP\nReturn status note.\n.TP\n.B next\nIndependent next command.\n";
    let response = explained(source, "first");
    let text = mant_engine::render_explanation_text(&response);
    for witness in [
        "Opening description",
        "Nested callback option",
        "Callback timing note",
        "Return status note",
    ] {
        assert!(text.contains(witness), "missing {witness}: {text}");
    }
    assert!(!text.contains("Independent next command"));
    let provider = response.supports[0].items().unwrap().last().unwrap();
    assert_eq!(
        provider.description.len(),
        4,
        "{}",
        serde_json::to_string(&provider.description).unwrap()
    );
}

#[test]
fn prose_bullets_and_independent_markdown_items_do_not_gain_context() {
    for source in [
        ".TH PROBE 1\n.SH DESCRIPTION\n.TP\nThis is a complete prose sentence.\n.TP\nAnother ordinary sentence.\nBody.\n",
        ".TH PROBE 1\n.SH DESCRIPTION\n.IP \\(bu\n.IP \\(bu\nBody.\n",
    ] {
        let content = query_roff_bytes(source.as_bytes()).unwrap();
        assert!(
            !serde_json::to_string(content.document.as_ref().unwrap())
                .unwrap()
                .contains("declarationGroups")
        );
    }
    let content = mant_engine::query_markdown_text("# Probe\n## Options\n<!-- mant:entries role=option -->\n- `--first`\n- `--second`: Second body.\n", None).unwrap();
    assert!(
        !serde_json::to_string(content.document.as_ref().unwrap())
            .unwrap()
            .contains("declarationGroups")
    );
}

#[test]
fn template_heads_keep_group_context_without_a_prefix_name() {
    let source = ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev DEMO_ Ns Ar NAME\n.It Ev DEMO_HOME\nEnvironment family explanation.\n.El\n";
    let response = explained(source, "DEMO_NAME");
    assert_eq!(response.supports.len(), 1);
    assert!(
        response.evidence[0]
            .entry
            .as_ref()
            .unwrap()
            .names
            .is_empty()
    );
    assert!(
        response.evidence[0]
            .bases
            .iter()
            .any(|b| matches!(b, mant_protocol::EvidenceBasis::Form { .. }))
    );
    assert!(
        !response.evidence[0]
            .bases
            .iter()
            .any(|b| matches!(b, mant_protocol::EvidenceBasis::Name { .. }))
    );
}

#[test]
fn provider_tables_code_links_and_tail_are_not_preview_clipped() {
    let source = ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.TP\n.B --second\nOpening body.\n.TS\nl l.\nCELL_A\tCELL_B\n.TE\n.IP\n.nf\nexample --unrelated\n.fi\n.IP\nSee\n.MR printf 3\nfor the final note.\n.TP\n.B --next\nIndependent body.\n";
    let response = explained(source, "--first");
    let support = &response.supports[0];
    let body = serde_json::to_value(support).unwrap();
    let encoded = body.to_string();
    for witness in [
        "CELL_A",
        "CELL_B",
        "example --unrelated",
        "final note",
        "table",
        "preformatted",
        "link",
    ] {
        assert!(encoded.contains(witness), "missing {witness}: {encoded}");
    }
    assert!(!encoded.contains("Independent body"));
    assert_eq!(response.counts.direct_entry.total, 1);
    response.validate_references().unwrap();
}
