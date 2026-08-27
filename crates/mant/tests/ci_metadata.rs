//! Guards the CI performance shortcuts without weakening verification scope.

#[test]
fn ci_reuses_only_complete_exact_sha_runs_and_avoids_release_rebuilds() {
    let workflow = include_str!("../../../.github/workflows/ci.yml");
    assert!(workflow.contains("actions: read"));
    assert!(workflow.contains("name: Plan CI"));
    assert!(workflow.contains("scripts/find-successful-ci.sh \"$GITHUB_SHA\" dev"));
    assert_eq!(
        workflow
            .matches("if: needs.scope.outputs.run_full == 'true'")
            .count(),
        7
    );
    assert!(workflow.contains("name: CI verified"));
    assert!(!workflow.contains("LIBMANDOC_RS_DENY_WARNINGS"));
    assert!(workflow.contains("bash scripts/check.sh --build-profile debug"));
    assert!(workflow.contains("./scripts/check-windows.ps1 -BuildProfile debug"));
    assert!(workflow.contains("cargo +\"$RUST_MSRV\" check --locked --workspace"));
    assert!(workflow.contains("cargo test --locked --package mantdoc --all-features"));
    assert!(!workflow.contains("name: Compile fuzz targets"));
    assert_eq!(workflow.matches("uses: Swatinem/rust-cache@").count(), 6);
    assert_eq!(workflow.matches("cache-bin: false").count(), 6);
    assert_eq!(
        workflow
            .matches("save-if: ${{ github.event_name == 'push' }}")
            .count(),
        6
    );
    assert!(!workflow.contains("uses: actions/cache@"));

    let unix = include_str!("../../../scripts/check.sh");
    assert!(!unix.contains("LIBMANDOC_RS_DENY_WARNINGS"));
    assert!(unix.contains("bash scripts/build-and-smoke.sh \"$profile\""));
    let windows = include_str!("../../../scripts/check-windows.ps1");
    assert!(!windows.contains("LIBMANDOC_RS_DENY_WARNINGS"));
    assert!(windows.contains("build-and-smoke.ps1\") -BuildProfile $BuildProfile"));
    assert!(windows.contains("test native mantdoc features"));

    let verifier = include_str!("../../../scripts/find-successful-ci.sh");
    assert!(verifier.contains(r#"[[ "$run_event" != "push" ]]"#));
    assert!(verifier.contains(".head_branch, .event, .html_url"));
    for job in [
        "Supply chain",
        "Build (Linux x64)",
        "Native roff conformance (Linux x64)",
        "Native (macOS arm64)",
        "Native (Windows x64)",
        "Rust MSRV (1.88.0)",
        "Coverage",
    ] {
        assert!(verifier.contains(job));
    }
}

#[test]
fn long_mantdoc_robustness_evidence_is_scheduled_not_a_push_gate() {
    let workflow = include_str!("../../../.github/workflows/mantdoc-robustness.yml");
    assert!(workflow.contains("name: Mantdoc robustness"));
    assert!(workflow.contains("schedule:"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("push:"));
    assert!(workflow.contains("FUZZ_JOBS=4 scripts/fuzz.sh \"$FUZZ_SECONDS\""));
    assert!(workflow.contains("cargo +nightly miri test --locked --package mantdoc --lib"));
    assert!(workflow.contains("m3_parallel_sessions_do_not_share_delayed_environment_definitions"));
}
