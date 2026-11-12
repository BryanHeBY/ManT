//! Commands behavior contracts.

use super::*;
#[test]
#[cfg(feature = "update")]
fn update_results_are_stable_json_documents() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--update-tldr", "--compact"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(
        output,
        "{\"schema\":\"mant.tldr-update/v1\",\"action\":\"updated\",\"cacheDir\":\"/cache/tldr\",\"revision\":\"abc123\"}\n"
    );
    assert_eq!(diagnostics.len(), 0);
    assert_eq!(host.update_calls.get(), 1);

    let (status, output, diagnostics) =
        invoke(&["--prune-docs", "--dry-run", "--compact"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(
        output,
        "{\"schema\":\"mant.sources-prune/v1\",\"config\":\"/data/mant/sources.toml\",\"dryRun\":true,\"sources\":[]}\n"
    );
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn doctor_supports_copy_friendly_text_and_stable_json_exit_statuses() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--doctor"], b"", &host);
    assert_eq!(status, 0);
    assert!(output.starts_with("ManT doctor\n\n[ok] runtime.fixture"));
    assert!(output.ends_with("1 ok, 0 info, 0 warning(s), 0 error(s)\n"));
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(
        &["--doctor", "--format", "json", "--compact"],
        b"",
        &FakeHost::with_doctor_error(),
    );
    assert_eq!(status, 1);
    let value: serde_json::Value = serde_json::from_str(&output).expect("doctor JSON");
    assert_eq!(value["schema"], "mant.doctor/v1");
    assert_eq!(value["outcome"], "error");
    assert_eq!(value["summary"]["errors"], 1);
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn catalog_lists_grouped_documents_and_emits_flat_find_records() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--list"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(output, "manual/3\n  printf\n\nsources/team\n  guide\n");
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(&["--find", "guide"], b"", &host);
    assert_eq!(status, 0);
    assert_eq!(
        output,
        "sources/team/guide\tmarkdown\nmanual/3/printf\tmanual\n"
    );
    assert_eq!(diagnostics.len(), 0);

    let (status, output, diagnostics) = invoke(
        &["--find", "guide", "--format", "json", "--compact"],
        b"",
        &host,
    );
    assert_eq!(status, 0);
    let value: serde_json::Value = serde_json::from_str(&output).expect("catalog JSON");
    assert_eq!(value["schema"], "mant.catalog/v0.14");
    assert_eq!(value["documents"][0]["address"]["path"], "guide");
    assert!(value["documents"][0].get("sourcePath").is_none());
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn usage_errors_are_concise_and_never_trigger_side_effects() {
    let host = FakeHost::new();
    let (status, output, diagnostics) = invoke(&["--unknown"], b"", &host);
    assert_eq!(status, 2);
    assert_eq!(output.len(), 0);
    assert!(diagnostics.starts_with("error: unexpected argument '--unknown'"));
    assert!(diagnostics.contains("Usage: mant"));
    assert!(diagnostics.contains("For more information, try '--help'."));
    assert_eq!(host.query_calls.get(), 0);
    assert_eq!(host.update_calls.get(), 0);
}
