//! Production ownership is independent of development harness dependencies.

use std::collections::BTreeSet;

fn production_edges(manifest: &str) -> BTreeSet<String> {
    production_edges_with_workspace(manifest, include_str!("../../../Cargo.toml"))
}

fn production_edges_with_workspace(manifest: &str, workspace: &str) -> BTreeSet<String> {
    fn collect(table: &toml::Value, workspace: &toml::Value, edges: &mut BTreeSet<String>) {
        for kind in ["dependencies", "build-dependencies"] {
            if let Some(dependencies) = table.get(kind).and_then(toml::Value::as_table) {
                for (alias, spec) in dependencies {
                    let spec = if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true)
                    {
                        &workspace["workspace"]["dependencies"][alias]
                    } else {
                        spec
                    };
                    let package = spec
                        .get("package")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(alias);
                    if package.starts_with("mant-")
                        || package == "libmandoc-rs"
                        || package == "mant"
                    {
                        edges.insert(package.to_owned());
                    }
                }
            }
        }
    }
    let manifest: toml::Value = toml::from_str(manifest).expect("valid manifest");
    let workspace: toml::Value = toml::from_str(workspace).expect("valid workspace manifest");
    let mut edges = BTreeSet::new();
    collect(&manifest, &workspace, &mut edges);
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            collect(target, &workspace, &mut edges);
        }
    }
    edges
}

#[test]
fn production_graph_keeps_each_component_at_its_authority_boundary() {
    let cases: &[(&str, &str, &[&str])] = &[
        ("mant-ir", include_str!("../../mant-ir/Cargo.toml"), &[]),
        (
            "mant-protocol",
            include_str!("../../mant-protocol/Cargo.toml"),
            &["mant-ir"],
        ),
        (
            "libmandoc-rs",
            include_str!("../../libmandoc-rs/Cargo.toml"),
            &[],
        ),
        (
            "mant-sources",
            include_str!("../../mant-sources/Cargo.toml"),
            &[],
        ),
        (
            "mant-codec",
            include_str!("../../mant-codec/Cargo.toml"),
            &["mant-ir", "libmandoc-rs"],
        ),
        (
            "mant-loader",
            include_str!("../../mant-loader/Cargo.toml"),
            &[
                "mant-ir",
                "mant-protocol",
                "mant-codec",
                "mant-sources",
                "libmandoc-rs",
            ],
        ),
        (
            "mant-query",
            include_str!("../../mant-query/Cargo.toml"),
            &["mant-ir", "mant-protocol", "mant-codec"],
        ),
        (
            "mant-render",
            include_str!("../../mant-render/Cargo.toml"),
            &["mant-ir", "mant-protocol", "mant-codec"],
        ),
        (
            "mant-engine",
            include_str!("../../mant-engine/Cargo.toml"),
            &["mant-ir", "mant-protocol", "mant-loader", "mant-query"],
        ),
        (
            "mant-ui",
            include_str!("../../mant-ui/Cargo.toml"),
            &["mant-ir", "mant-protocol", "mant-render"],
        ),
    ];
    for (name, manifest, allowed) in cases {
        let expected = allowed.iter().map(|value| (*value).to_owned()).collect();
        assert_eq!(
            production_edges(manifest),
            expected,
            "{name} production boundary drifted"
        );
    }
}

#[test]
fn graph_guard_includes_renamed_optional_and_target_build_dependencies() {
    let manifest = r#"
[dev-dependencies]
mant-engine = "^0.11"
[dependencies]
codec = { package = "mant-codec", version = "^0.11", optional = true }
[target.'cfg(windows)'.build-dependencies]
mant-loader = "^0.11"
"#;
    assert_eq!(
        production_edges(manifest),
        BTreeSet::from(["mant-codec".to_owned(), "mant-loader".to_owned()])
    );
}

#[test]
fn graph_guard_resolves_renames_inherited_from_the_workspace() {
    let workspace = r#"
[workspace.dependencies]
application = { package = "mant-engine", version = "^0.11" }
"#;
    let manifest = r"
[target.'cfg(windows)'.build-dependencies]
application = { workspace = true, optional = true }
";
    assert_eq!(
        production_edges_with_workspace(manifest, workspace),
        BTreeSet::from(["mant-engine".to_owned()])
    );
}
