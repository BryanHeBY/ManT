use super::process_support::{executable, registered_data_root, run_with_registered_documents};
use super::support::{configure_registered_documents, registered_documents_dir};
use std::fs;
use std::process::Command;

fn plain_document_heading(value: &serde_json::Value) -> &serde_json::Value {
    &value["document"]["heading"]["content"][0]["value"]
}

#[cfg(feature = "update")]
fn run_git(directory: &std::path::Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git fixture command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(feature = "update")]
fn document_sources_update_on_demand_and_support_explicit_selection() {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-document-source-process-{}",
        std::process::id()
    ));
    let repository = fixture_root.join("repository");
    let data_root = registered_data_root(&fixture_root);
    fs::create_dir_all(repository.join("docs/reference")).expect("create repository fixture");
    fs::write(
        repository.join("docs/reference/source-tool.md"),
        "# Source tool\n\nFirst revision.\n",
    )
    .expect("write selected Markdown");
    fs::write(repository.join("README.md"), "# Outside configured path\n")
        .expect("write outer readme");
    run_git(&repository, &["init", "--initial-branch=main"]);
    run_git(&repository, &["config", "user.name", "ManT Test"]);
    run_git(
        &repository,
        &["config", "user.email", "mant-test@example.invalid"],
    );
    run_git(&repository, &["add", "."]);
    run_git(&repository, &["commit", "-m", "initial"]);

    fs::create_dir_all(&data_root).expect("create application data root");
    let repository_url = repository.to_string_lossy();
    fs::write(
        data_root.join("sources.toml"),
        format!(
            "[team]\nrepo = {repository_url:?}\nbranch = \"main\"\npath = \"docs\"\ninclude = [\"reference\"]\npriority = 10\n"
        ),
    )
    .expect("write source config");
    assert!(!data_root.join("sources").exists());

    let mut update = Command::new(executable());
    configure_registered_documents(&mut update, &fixture_root);
    let first = update
        .args(["--update-docs", "--compact"])
        .output()
        .expect("update document source");
    assert!(first.status.success(), "{first:?}");
    let result: serde_json::Value = serde_json::from_slice(&first.stdout).expect("update JSON");
    assert_eq!(result["schema"], "mant.sources-update/v2");
    assert_eq!(result["orphaned"], serde_json::json!([]));
    assert_eq!(result["sources"][0]["source"], "team");
    assert_eq!(result["sources"][0]["action"], "updated");
    assert_eq!(result["sources"][0]["documents"], 1);
    let installed = data_root.join("sources/team");
    assert!(installed.join("reference/source-tool.md").is_file());
    assert!(installed.join(".mant-source.toml").is_file());
    assert!(!installed.join("README.md").exists());

    let mut unchanged = Command::new(executable());
    configure_registered_documents(&mut unchanged, &fixture_root);
    let unchanged = unchanged
        .args(["--update-docs", "--compact"])
        .output()
        .expect("check unchanged source");
    assert!(unchanged.status.success(), "{unchanged:?}");
    let result: serde_json::Value =
        serde_json::from_slice(&unchanged.stdout).expect("unchanged JSON");
    assert_eq!(result["sources"][0]["action"], "unchanged");

    let mut query = Command::new(executable());
    configure_registered_documents(&mut query, &fixture_root);
    let query = query
        .args([
            "source-tool",
            "--source",
            "team",
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("query installed source");
    assert!(query.status.success(), "{query:?}");
    let result: serde_json::Value = serde_json::from_slice(&query.stdout).expect("query JSON");
    assert_eq!(plain_document_heading(&result), "Source tool");

    let documents = registered_documents_dir(&fixture_root);
    fs::create_dir_all(&documents).expect("create root documents");
    fs::write(
        documents.join("source-tool.md"),
        "# Root tool\n\nRoot document wins.\n",
    )
    .expect("write root document");
    let mut fallback = Command::new(executable());
    configure_registered_documents(&mut fallback, &fixture_root);
    let fallback = fallback
        .args(["source-tool", "--format", "json", "--compact"])
        .output()
        .expect("query fallback chain");
    assert!(fallback.status.success(), "{fallback:?}");
    let result: serde_json::Value =
        serde_json::from_slice(&fallback.stdout).expect("fallback JSON");
    assert_eq!(plain_document_heading(&result), "Root tool");

    let unknown =
        run_with_registered_documents(&fixture_root, &["source-tool", "--source", "missing"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("is not configured"));

    fs::remove_dir_all(fixture_root).expect("remove document source fixture");
}

#[test]
#[cfg(feature = "update")]
fn document_source_pruning_is_explicit_and_preserves_personal_documents() {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-document-source-prune-process-{}",
        std::process::id()
    ));
    let documents = registered_documents_dir(&fixture_root);
    let data_root = registered_data_root(&fixture_root);
    let installed = data_root.join("sources/removed");
    fs::create_dir_all(&installed).expect("create installed source fixture");
    fs::create_dir_all(&documents).expect("create personal documents fixture");
    fs::write(data_root.join("sources.toml"), "").expect("write empty source config");
    fs::write(
        installed.join(".mant-source.toml"),
        "version = 1\nsource = 'removed'\nrevision = 'abc123'\ndocuments = 1\n",
    )
    .expect("write source identity");
    fs::write(installed.join("tool.md"), "# Removed source\n").expect("write source document");
    fs::write(documents.join("personal.md"), "# Personal\n").expect("write personal document");

    let orphan_report =
        run_with_registered_documents(&fixture_root, &["--update-docs", "--compact"]);
    assert!(orphan_report.status.success(), "{orphan_report:?}");
    let report: serde_json::Value =
        serde_json::from_slice(&orphan_report.stdout).expect("orphan report JSON");
    assert_eq!(report["sources"], serde_json::json!([]));
    assert_eq!(report["orphaned"][0]["source"], "removed");
    assert_eq!(report["orphaned"][0]["removable"], true);
    assert!(installed.is_dir());

    let dry_run =
        run_with_registered_documents(&fixture_root, &["--prune-docs", "--dry-run", "--compact"]);
    assert!(dry_run.status.success(), "{dry_run:?}");
    let report: serde_json::Value =
        serde_json::from_slice(&dry_run.stdout).expect("prune dry-run JSON");
    assert_eq!(report["schema"], "mant.sources-prune/v1");
    assert_eq!(report["dryRun"], true);
    assert_eq!(report["sources"][0]["action"], "would-remove");
    assert!(installed.is_dir());

    let prune = run_with_registered_documents(&fixture_root, &["--prune-docs", "--compact"]);
    assert!(prune.status.success(), "{prune:?}");
    let report: serde_json::Value = serde_json::from_slice(&prune.stdout).expect("prune JSON");
    assert_eq!(report["dryRun"], false);
    assert_eq!(report["sources"][0]["action"], "removed");
    assert!(!installed.exists());
    assert!(documents.join("personal.md").is_file());

    fs::remove_dir_all(fixture_root).expect("remove prune fixture");
}

#[test]
#[cfg(feature = "update")]
fn document_source_failures_keep_a_complete_json_report() {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-document-source-failure-process-{}",
        std::process::id()
    ));
    let data_root = registered_data_root(&fixture_root);
    fs::create_dir_all(&data_root).expect("create application data root");
    fs::write(
        data_root.join("sources.toml"),
        format!(
            "[broken]\nrepo = {:?}\nbranch = \"main\"\n",
            fixture_root.join("missing.git").to_string_lossy()
        ),
    )
    .expect("write failing source config");

    let mut update = Command::new(executable());
    configure_registered_documents(&mut update, &fixture_root);
    let output = update
        .args(["--update-docs", "--compact"])
        .output()
        .expect("run failing document update");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stderr.len(), 0);
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("failure report JSON");
    assert_eq!(report["sources"][0]["source"], "broken");
    assert_eq!(report["sources"][0]["action"], "failed");
    assert!(report["sources"][0]["error"].as_str().is_some());
    assert!(!data_root.join("sources/broken").exists());

    fs::remove_dir_all(fixture_root).expect("remove failure fixture");
}
