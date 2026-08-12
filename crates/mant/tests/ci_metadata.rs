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
        6
    );
    assert!(workflow.contains("name: CI verified"));
    assert!(workflow.contains("bash scripts/check.sh --build-profile debug"));
    assert!(workflow.contains("./scripts/check-windows.ps1 -BuildProfile debug"));
    assert!(workflow.contains("cargo +\"$RUST_MSRV\" check --locked --workspace"));
    assert!(!workflow.contains("name: Compile fuzz targets"));

    let unix = include_str!("../../../scripts/check.sh");
    assert!(unix.contains("bash scripts/build-and-smoke.sh \"$profile\""));
    let windows = include_str!("../../../scripts/check-windows.ps1");
    assert!(windows.contains("build-and-smoke.ps1\") -BuildProfile $BuildProfile"));

    let verifier = include_str!("../../../scripts/find-successful-ci.sh");
    assert!(verifier.contains(r#"[[ "$run_event" != "push" ]]"#));
    assert!(verifier.contains(".head_branch, .event, .html_url"));
    for job in [
        "Supply chain",
        "Build (Linux x64)",
        "Native (macOS arm64)",
        "Native (Windows x64)",
        "Rust MSRV (1.88.0)",
        "Coverage",
    ] {
        assert!(verifier.contains(job));
    }
}
