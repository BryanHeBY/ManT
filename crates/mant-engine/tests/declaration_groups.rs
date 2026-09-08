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
