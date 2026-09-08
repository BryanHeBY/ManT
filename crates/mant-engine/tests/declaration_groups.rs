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
        assert_eq!(
            json["evidence"][0]["content"]["block"]["items"][0]["description"],
            serde_json::json!([])
        );
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
