use super::process_support::{run_with_registered_documents, scope_document_paths};
use super::support::registered_documents_dir;
use std::fs;
use std::path::PathBuf;

#[test]
fn document_scopes_follow_typed_links_breadth_first_and_query_multiple_roots() {
    let fixture_root = document_scope_fixture("traversal");

    let search = run_with_registered_documents(
        &fixture_root,
        &[
            "alpha",
            "--follow-links",
            "--max-depth",
            "8",
            "--max-documents",
            "8",
            "--search",
            "needle",
            "--format",
            "json",
            "--compact",
        ],
    );
    assert!(search.status.success(), "{search:?}");
    assert!(search.stderr.is_empty());
    let search: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("scope search JSON");
    assert_eq!(scope_document_paths(&search), ["alpha", "beta", "gamma"]);
    assert!(search["scope"].get("frontier").is_none());
    assert_eq!(search["result"]["search"]["total"], 3);
    let groups = search["result"]["search"]["documents"]
        .as_array()
        .expect("search groups");
    assert_eq!(groups.len(), 3);
    assert!(groups.iter().all(|group| group.get("search").is_none()));
    assert!(groups.iter().all(|group| group.get("offset").is_none()));
    assert_eq!(
        groups
            .iter()
            .flat_map(|group| group["matches"].as_array().expect("search hits"))
            .map(|hit| hit["ordinal"].as_u64().expect("global ordinal"))
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );

    let explain = run_with_registered_documents(
        &fixture_root,
        &[
            "--document",
            "beta",
            "--document",
            "alpha",
            "--explain=--shared",
            "--format",
            "json",
            "--compact",
        ],
    );
    assert!(explain.status.success(), "{explain:?}");
    let explain: serde_json::Value =
        serde_json::from_slice(&explain.stdout).expect("scope explain JSON");
    let matches = explain["result"]["explanation"]["documents"]
        .as_array()
        .expect("scope explanations");
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0]["address"]["path"], "beta");
    assert_eq!(matches[1]["address"]["path"], "alpha");
    assert_eq!(explain["result"]["explanation"]["outcome"], "evidence");
    assert_eq!(
        explain["result"]["explanation"]["failures"],
        serde_json::json!([])
    );

    let missing = run_with_registered_documents(
        &fixture_root,
        &[
            "--document",
            "alpha",
            "--document",
            "beta",
            "--explain=missing-entry",
        ],
    );
    assert!(missing.status.success(), "{missing:?}");
    assert!(missing.stderr.is_empty());
    let missing = String::from_utf8(missing.stdout).expect("scope miss text");
    assert!(
        missing.contains("no-evidence; owners=0, returned=0"),
        "{missing}"
    );
    assert!(
        missing.contains("Coverage: loaded=2, unresolved=0"),
        "{missing}"
    );

    fs::remove_dir_all(fixture_root).expect("remove scope fixture");
}

#[test]
fn document_scope_frontiers_distinguish_depth_and_document_limits() {
    let fixture_root = document_scope_fixture("frontier");

    let depth_bounded = run_with_registered_documents(
        &fixture_root,
        &[
            "alpha",
            "--follow-links",
            "--max-depth",
            "0",
            "--search",
            "needle",
            "--format",
            "json",
            "--compact",
        ],
    );
    assert!(depth_bounded.status.success(), "{depth_bounded:?}");
    let depth_bounded: serde_json::Value =
        serde_json::from_slice(&depth_bounded.stdout).expect("depth-bounded scope JSON");
    assert_eq!(scope_document_paths(&depth_bounded), ["alpha"]);
    assert_eq!(depth_bounded["scope"]["frontier"][0]["limit"], "max-depth");
    assert_eq!(
        depth_bounded["scope"]["frontier"][0]["target"]["selector"],
        "documents/beta"
    );

    let bounded = run_with_registered_documents(
        &fixture_root,
        &[
            "alpha",
            "--follow-links",
            "--max-documents",
            "2",
            "--search",
            "needle",
            "--format",
            "json",
            "--compact",
        ],
    );
    assert!(bounded.status.success(), "{bounded:?}");
    let bounded: serde_json::Value =
        serde_json::from_slice(&bounded.stdout).expect("bounded scope JSON");
    assert_eq!(
        bounded["scope"]["frontier"]
            .as_array()
            .expect("scope frontier")
            .len(),
        1
    );
    assert_eq!(bounded["scope"]["frontier"][0]["limit"], "max-documents");
    assert_eq!(
        bounded["scope"]["frontier"][0]["target"]["selector"],
        "documents/gamma"
    );

    fs::remove_dir_all(fixture_root).expect("remove scope fixture");
}

fn document_scope_fixture(label: &str) -> PathBuf {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-document-scope-process-{}-{label}",
        std::process::id(),
    ));
    let documents = registered_documents_dir(&fixture_root);
    fs::create_dir_all(&documents).expect("create scope documents");
    let page = |name: &str, body: &str| {
        fs::write(documents.join(name), body).expect("write scope document");
    };
    page(
        "alpha.md",
        "# Alpha\n\n[Beta](beta.md)\n\nneedle alpha\n\n## Options\n\n<!-- mant:entries role=option -->\n- `--shared`: Alpha option.\n",
    );
    page(
        "beta.md",
        "# Beta\n\n[Gamma](gamma.md)\n\nneedle beta\n\n## Options\n\n<!-- mant:entries role=option -->\n- `--shared`: Beta option.\n",
    );
    page("gamma.md", "# Gamma\n\n[Alpha](alpha.md)\n\nneedle gamma\n");
    fixture_root
}
