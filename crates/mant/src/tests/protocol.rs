//! Protocol behavior contracts.

use super::*;
#[test]
fn stdin_protocol_emits_only_compact_query_json() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git","manualSection":"1"},"view":{"kind":"full"}}"#,
            &host,
        );

    assert_eq!(status, 0);
    assert_eq!(
        output,
        "{\"schema\":\"mant.query/v0.14\",\"label\":\"git\"}\n"
    );
    assert_eq!(diagnostics.len(), 0);
    assert_eq!(host.query_calls.get(), 1);
}

#[test]
fn malformed_or_extended_requests_fail_before_querying_the_host() {
    for input in [
            br"not-json".as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"full"},"futureField":true}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"   "},"view":{"kind":"full"}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"excerpt","nodes":[]}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"excerpt","selectors":[]}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"excerpt","selectors":["1","2","3","4","5","6","7","8","9","10","11","12","13","14","15","16","17"]}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"outline","root":"   "}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"outline","entries":{"kind":"kinds","kinds":[]}}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"","limit":10}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"git","limit":0}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"git","contextLines":101}}"#.as_slice(),
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"git"},"view":{"kind":"search","pattern":"[","syntax":"regex"}}"#.as_slice(),
        ] {
            let host = FakeHost::new();
            let (status, output, diagnostics) = invoke(
                &["--request-json", "--format", "json", "--compact"],
                input,
                &host,
            );
            assert_eq!(status, 2);
            assert_eq!(output.len(), 0);
            assert!(diagnostics.starts_with("mant: "));
            assert_eq!(host.query_calls.get(), 0);
        }
}

#[test]
fn stdin_requests_select_outline_and_excerpt_projections() {
    let host = FakeHost::with_manual_and_tldr();
    let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"demo"},"view":{"kind":"outline","entries":{"kind":"none"}}}"#,
            &host,
        );
    assert_eq!(status, 0);
    let outline: serde_json::Value = serde_json::from_str(&output).expect("outline JSON");
    assert_eq!(outline["schema"], "mant.outline/v0.14");
    assert_eq!(outline["entries"]["kind"], "none");
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"demo"},"view":{"kind":"excerpt","selectors":[{"kind":"path","path":"2.1"}]}}"#,
            &host,
        );
    assert_eq!(status, 0);
    let excerpt: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
    assert_eq!(excerpt["schema"], "mant.excerpt/v0.14");
    assert_eq!(excerpt["selections"][0]["outline"]["node"]["path"], "2.1");
    assert_eq!(diagnostics.len(), 0);
    assert_eq!(host.query_calls.get(), 2);
}

#[test]
fn stdin_search_requests_use_the_same_projection_contract() {
    let host = FakeHost::with_manual();
    let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v0.14","input":{"kind":"document","selector":"demo"},"view":{"kind":"search","pattern":"options","limit":10}}"#,
            &host,
        );

    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
    assert_eq!(value["schema"], "mant.search/v0.14");
    assert_eq!(value["query"]["syntax"], "literal");
    assert_eq!(value["query"]["scope"], "visible");
    assert!(
        value["matches"]
            .as_array()
            .is_some_and(|matches| !matches.is_empty())
    );
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn protocol_results_are_stable_json_documents_without_optional_capabilities() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--protocol-version", "--compact"], b"", &host);
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("protocol JSON");
    assert_eq!(value["protocol"], CLI_PROTOCOL_VERSION);
    assert_eq!(value["nativeApiVersion"], "0.14");
    assert_eq!(value["requestSchema"], "mant.request/v0.14");
    assert_eq!(value["outlineSchema"], "mant.outline/v0.14");
    assert_eq!(value["excerptSchema"], "mant.excerpt/v0.14");
    assert_eq!(value["searchSchema"], "mant.search/v0.14");
    assert_eq!(value["catalogSchema"], "mant.catalog/v0.14");
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn generated_schemas_are_json_only_and_side_effect_free() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--schema", "request", "--compact"], b"", &host);

    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("request schema");
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(value["additionalProperties"], false);
    assert!(output.contains("mant.request/v0.14"));
    assert_eq!(diagnostics.len(), 0);
    assert_eq!(host.query_calls.get(), 0);
    assert_eq!(host.update_calls.get(), 0);

    let (status, output, diagnostics) = invoke(&["--schema", "all"], b"", &host);
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("schema catalog");
    assert!(value["request"].is_object());
    assert!(value["query"].is_object());
    assert!(value["outline"].is_object());
    assert!(value["excerpt"].is_object());
    assert!(value["search"].is_object());
    assert!(value["doctor"].is_object());
    assert!(value["tldr-update"].is_object());
    assert_eq!(diagnostics.len(), 0);
    assert_eq!(host.query_calls.get(), 0);
    assert_eq!(host.update_calls.get(), 0);
}

#[test]
fn deep_generated_responses_reach_the_top_level_schema_diagnostic() {
    let input = format!(
        "{{\"schema\":\"mant.query/v0.14\",\"result\":{}}}",
        "[".repeat(140) + "0" + &"]".repeat(140)
    );
    let Err(error) = read_native_request(&mut input.as_bytes()) else {
        panic!("a response schema must not be accepted as a request");
    };

    assert!(
        error
            .into_message()
            .contains("unsupported request schema 'mant.query/v0.14'"),
    );
}
