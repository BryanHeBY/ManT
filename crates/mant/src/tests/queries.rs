//! Queries behavior contracts.

use super::*;
#[test]
fn invalid_complete_views_are_rejected_before_the_cli_host_is_called() {
    let host = FakeHost::new();
    for view in [
        mant_protocol::QueryView::Excerpt { selectors: vec![] },
        mant_protocol::QueryView::Explain {
            entry: String::new(),
            options: mant_protocol::ExplanationOptions::default(),
        },
        mant_protocol::QueryView::Outline {
            entries: mant_protocol::EntryProjection::Kinds { kinds: vec![] },
            root: None,
            references: mant_protocol::ReferenceProjection::default(),
        },
        mant_protocol::QueryView::Search {
            pattern: "[".into(),
            syntax: mant_protocol::SearchSyntax::Regex,
            case: mant_protocol::SearchCase::Sensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    ] {
        let request = QueryRequest {
            schema: mant_protocol::RequestSchema::V0Dot14,
            input: QueryInput::Document {
                selector: "tool".into(),
                source: None,
                manual_section: None,
            },
            view,
        };
        assert!(crate::application::execute_query(&request, LoadPolicy::Combined, &host).is_err());
        assert_eq!(host.query_calls.get(), 0);
    }
    let scope = mant_protocol::ScopeQueryRequest {
        schema: mant_protocol::ScopeRequestSchema::V0Dot14,
        scope: mant_protocol::DocumentScope {
            documents: vec![mant_protocol::DocumentSelector {
                selector: "tool".into(),
                source: None,
                manual_section: None,
            }],
            traversal: mant_protocol::DocumentTraversal::default(),
        },
        view: mant_protocol::ScopeQueryView::Explain {
            entry: String::new(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    };
    assert!(crate::application::execute_scope_query(&scope, &host).is_err());
    assert_eq!(host.query_calls.get(), 0);
}

#[test]
fn catalog_addresses_reopen_the_exact_source_or_manual_section() {
    let (request, policy) = request_for_address(&DocumentAddress::Markdown {
        path: "Start-Process".to_owned(),
        origin: MarkdownOrigin::Source {
            name: "pwsh7".to_owned(),
        },
    });
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "sources/pwsh7/Start-Process".to_owned(),
            source: None,
            manual_section: None,
        }
    );
    assert_eq!(policy, LoadPolicy::Combined);

    let (request, policy) = request_for_address(&DocumentAddress::Manual {
        name: "printf".to_owned(),
        manual_section: "3".to_owned(),
    });
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "manual/3/printf".to_owned(),
            source: None,
            manual_section: None,
        }
    );
    assert_eq!(policy, LoadPolicy::ManualWithTldr);

    let (request, policy) = request_for_address(&DocumentAddress::Markdown {
        path: "en/tool".into(),
        origin: MarkdownOrigin::Documents,
    });
    assert_eq!(policy, LoadPolicy::Combined);
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "documents/en/tool".into(),
            source: None,
            manual_section: None,
        }
    );
}

#[test]
fn unqualified_manual_navigation_preserves_native_resolution_without_a_default_section() {
    let (request, policy) =
        crate::application::request_for_navigation(&mant_protocol::DocumentOpenTarget::Manual {
            name: "printf".into(),
            manual_section: None,
        });
    assert_eq!(policy, LoadPolicy::ManualWithTldr);
    assert_eq!(
        request.input,
        QueryInput::Document {
            selector: "printf".into(),
            source: None,
            manual_section: None,
        }
    );
}

#[test]
fn direct_queries_render_outlines_and_selected_nodes_in_requested_formats() {
    let host = FakeHost::with_manual_and_tldr();
    let (status, output, diagnostics) = invoke(&["demo", "--outline"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("├─ 0 TLDR QUICK REFERENCE\n│    ID: tldr"));
    assert!(output.contains("├─ 1 NAME\n│    ID: name-1"));
    assert!(output.contains("└─ 2 OPTIONS\n     ID: options-2"));
    assert!(output.contains("└─ 2.1 Common options\n       ID: common-3"));
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(
        &["demo", "--node", "2.1", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
    assert_eq!(value["schema"], "mant.excerpt/v0.14");
    assert_eq!(value["selections"][0]["outline"]["node"]["path"], "2.1");
    assert_eq!(
        value["selections"][0]["section"]["heading"]["content"][0]["value"],
        "Common options"
    );
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(
        &["demo", "--node", "0", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("tldr excerpt JSON");
    assert_eq!(value["selections"][0]["kind"], "tldr");
    assert_eq!(value["selections"][0]["outline"]["node"]["path"], "0");
    assert_eq!(value["selections"][0]["document"]["title"], "demo");
    assert!(value.get("producer").is_none());
    assert!(value.get("diagnostics").is_none());
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(&["demo", "--tldr"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("A small demonstration."));
    assert!(!output.contains("## NAME"));
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn explains_semantic_evidence_without_turning_sections_into_entries() {
    let host = FakeHost::with_explainable_manual();
    let (status, output, diagnostics) = invoke(&["demo", "--explain", "--exclude"], b"", &host);

    assert_eq!(status, 0);
    assert!(
        output.contains("2/e1 [exclude]\nOPTIONS > --exclude\nKind: option\nMatched by:"),
        "{output}"
    );
    assert!(output.contains("--exclude=PATTERN"));
    assert!(output.contains("Exclude matching files from the archive."));
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(
        &[
            "demo",
            "--explain=--exclude",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
    assert_eq!(value["schema"], "mant.explanation/v0.14");
    assert_eq!(value["outcome"], "evidence");
    assert_eq!(value["evidence"][0]["outline"]["node"]["id"], "exclude");
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(&["demo", "--explain=2"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("no-evidence"), "{output}");
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn semantic_entries_work_through_cli_and_request_json() {
    let host = FakeHost::with_semantic_markdown();
    let (status, output, diagnostics) = invoke(
        &[
            "demo",
            "--outline",
            "--outline-entries",
            "all",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let outline: serde_json::Value = serde_json::from_str(&output).expect("outline JSON");
    assert_eq!(outline["schema"], "mant.outline/v0.14");
    let encoded = outline.to_string();
    assert!(encoded.contains("\"parameterKind\":\"option\""));
    assert!(encoded.contains("\"kind\":\"command\""));
    assert!(encoded.contains("\"kind\":\"environment-variable\""));
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(
        &[
            "demo",
            "--outline",
            "--outline-entries",
            "option",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let outline: serde_json::Value = serde_json::from_str(&output).expect("alias outline");
    assert_eq!(outline["entries"]["kind"], "kinds");
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn semantic_entry_selectors_work_through_cli_and_request_json() {
    let host = FakeHost::with_semantic_markdown();
    let (status, output, diagnostics) = invoke(
        &["demo", "--explain=query", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let evidence: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(evidence["evidence"].as_array().unwrap().iter().any(|item| {
        item["entry"]["kind"]["kind"] == "command"
            && item["bases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|basis| basis["kind"] == "name")
    }));
    assert_eq!(diagnostics.len(), 0);

    let (status, output, _) = invoke(
        &[
            "demo",
            "--explain=command-query",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
    assert_eq!(excerpt["evidence"][0]["entry"]["kind"]["kind"], "command");

    let (status, output, _) = invoke(
        &["demo", "--node=id:query", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("section excerpt");
    assert_eq!(excerpt["selections"][0]["kind"], "document-section");
    assert_eq!(excerpt["selections"][0]["outline"]["node"]["id"], "query");

    for (selector, role) in [("/s", "option"), ("$ENV:PATH", "environment-variable")] {
        let argument = format!("--explain={selector}");
        let (status, output, diagnostics) = invoke(
            &["demo", &argument, "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("role explanation");
        let kind = &excerpt["evidence"][0]["entry"]["kind"];
        if role == "option" {
            assert_eq!(kind["kind"], "parameter");
            assert_eq!(kind["parameterKind"], "option");
        } else {
            assert_eq!(kind["kind"], role);
        }
        assert_eq!(diagnostics.len(), 0);
    }

    let (status, output, diagnostics) = invoke(
        &["--request-json", "--format", "json", "--compact"],
        br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"demo"},"view":{"kind":"explain","entry":"command-query"}}"#,
        &host,
    );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("request excerpt");
    assert_eq!(excerpt["evidence"][0]["entry"]["kind"]["kind"], "command");
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn ambiguous_semantic_entries_remain_addressable_by_returned_id() {
    let host = FakeHost::with_semantic_markdown();
    let (status, output, diagnostics) = invoke(&["demo", "--explain=/f"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.contains("option-f"));
    assert!(output.contains("owners=2"), "{output}");
    assert_eq!(diagnostics.len(), 0);
    let multiple = output;

    let (status, outline, outline_diagnostics) = invoke(
        &[
            "demo",
            "--outline",
            "--outline-entries",
            "all",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    assert_eq!(outline_diagnostics.len(), 0);
    let outline: serde_json::Value = serde_json::from_str(&outline).expect("outline JSON");
    let qualified_id = outline["nodes"][5]["children"][0]["id"]
        .as_str()
        .expect("returned semantic ID");
    assert!(qualified_id.starts_with("option-f-"));
    assert!(multiple.contains(qualified_id));

    let qualified_argument = format!("--explain={qualified_id}");
    let (status, output, diagnostics) = invoke(
        &["demo", &qualified_argument, "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("qualified entry");
    assert_eq!(
        excerpt["evidence"][0]["outline"]["node"]["id"],
        qualified_id
    );
    assert_eq!(diagnostics.len(), 0);
}

#[test]
#[cfg(feature = "roff")]
fn manual_option_reaches_the_resolution_policy_without_stderr_noise() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) = invoke(&["demo", "--outline", "--manual"], b"", &host);

    assert_eq!(status, 0);
    assert!(output.contains("1 NAME\n│    ID: name-1"));
    assert_eq!(diagnostics.len(), 0);
    assert_eq!(host.last_policy.get(), LoadPolicy::ManualOnly);
}

#[test]
fn searches_report_markdown_coordinates_and_reusable_outline_nodes() {
    let host = FakeHost::with_manual_and_tldr();
    let (status, output, diagnostics) = invoke(
        &[
            "demo",
            "--search",
            "common details",
            "--format",
            "json",
            "--compact",
        ],
        b"",
        &host,
    );

    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
    assert_eq!(value["schema"], "mant.search/v0.14");
    assert_eq!(value["total"], 1);
    assert_eq!(value["matches"][0]["outline"]["node"]["path"], "2.1");
    assert_eq!(value["matches"][0]["outline"]["node"]["id"], "common-3");
    assert!(value["matches"][0]["occurrences"][0]["markdown"]["startLine"].as_u64() > Some(1));
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(&["demo", "--grep", "missing"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(output, "No matches for \"missing\" in demo(1).\n");
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn unknown_nodes_are_concise_usage_failures() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) =
        invoke(&["demo", "--node", "9", "--format", "text"], b"", &host);

    assert_eq!(status, 2);
    assert_eq!(output.len(), 0);
    assert!(diagnostics.contains("document 'demo' has no outline node 'path:9'"));
    assert!(diagnostics.contains("mant demo --outline --outline-entries all --format json"));
}

#[test]
fn explain_reports_ordinary_support_without_inventing_a_definition() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) = invoke(&["demo", "--explain=details"], b"", &host);

    assert_eq!(status, 0);
    assert!(output.contains("2.1"), "{output}");
    assert!(output.contains("Common options"), "{output}");
    assert!(output.contains("Matched by: text mention"), "{output}");
    assert_eq!(diagnostics.len(), 0);
}
