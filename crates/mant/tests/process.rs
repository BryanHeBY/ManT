//! Black-box checks for the executable's stdout, stderr, and exit-code contract.

mod support;

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use support::{configure_registered_documents, registered_documents_dir};

fn executable() -> &'static str {
    env!("CARGO_BIN_EXE_mant")
}

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

fn run_with_registered_documents(
    home: &std::path::Path,
    arguments: &[&str],
) -> std::process::Output {
    let mut command = Command::new(executable());
    configure_registered_documents(&mut command, home);
    command.args(arguments).output().expect("run isolated mant")
}

const PROTOCOL_REFERENCE: &str = include_str!("../../../docs/protocol.md");

#[test]
fn help_groups_the_public_query_surface() {
    let output = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("mant <NAME|MARKDOWN|-> [OPTIONS]"));
    assert!(help.contains("mant README.md"));
    assert!(help.contains("cat guide.md | mant -"));
    assert!(help.contains("Document selection:"));
    assert!(help.contains("Search:"));
    assert!(help.contains("Integration:"));
    assert!(help.contains("Reading:"));
    assert!(help.contains("-h, --help"));
    assert!(help.contains("--ui"));
    assert!(help.contains("-V, --version"));
    assert!(help.contains("--format <FORMAT>"));
    assert!(help.contains("--preserve-anchors"));
    assert!(help.contains("--update-tldr"));
    assert!(help.contains("--update-docs"));
    assert!(help.contains("--source <SOURCE>"));
    assert!(help.contains("--protocol-version"));
    assert!(help.contains("--schema <CONTRACT>"));
    assert!(help.contains("--mcp"));
    assert!(help.contains("--explain <ENTRY>"));
    assert!(help.contains("--search <PATTERN>"));
    assert!(help.contains("--manual"));
    assert!(!help.contains("--force-libmandoc"));
    assert!(!help.contains("--force-groff"));
    assert!(!help.contains("--json"));
    assert!(!help.contains("update tldr"));
}

#[test]
fn version_uses_the_standard_successful_clap_boundary() {
    let output = Command::new(executable())
        .arg("--version")
        .output()
        .expect("run mant --version");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 version"),
        format!("mant {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn short_help_alias_matches_long_help() {
    let short = Command::new(executable())
        .arg("-h")
        .output()
        .expect("run mant -h");
    let long = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run mant --help");

    assert!(short.status.success());
    assert!(short.stderr.is_empty());
    assert_eq!(short.stdout, long.stdout);
    assert!(long.status.success());
    assert!(long.stderr.is_empty());
}

#[test]
fn explicit_ui_requires_a_real_terminal_before_loading_a_document() {
    let output = Command::new(executable())
        .args(["definitely-not-a-real-manual", "--ui"])
        .output()
        .expect("run redirected mant UI");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.contains("interactive view requires"));
    assert!(!diagnostic.contains("No manual entry"));
}

#[test]
fn request_schema_is_discoverable_without_host_state() {
    let output = Command::new(executable())
        .args(["--schema", "request", "--compact"])
        .output()
        .expect("run mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("request schema");
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(value["additionalProperties"], false);
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 schema")
            .contains("mant.request/v5")
    );
}

#[test]
fn protocol_version_is_a_clean_json_document() {
    let output = Command::new(executable())
        .args(["--protocol-version", "--compact"])
        .output()
        .expect("run mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("protocol JSON");
    assert_eq!(value["protocol"], "mant.cli/v5");
    assert_eq!(value["requestSchema"], "mant.request/v5");
    assert_eq!(value["querySchema"], "mant.query/v4");
    assert_eq!(value["outlineSchema"], "mant.outline/v4");
    assert_eq!(value["excerptSchema"], "mant.excerpt/v4");
    assert_eq!(value["searchSchema"], "mant.search/v4");

    for (field, marker) in value.as_object().expect("protocol descriptor") {
        let documented = format!(
            "\"{field}\": {}",
            serde_json::to_string(marker).expect("protocol marker")
        );
        assert!(
            PROTOCOL_REFERENCE.contains(&documented),
            "the protocol reference must document {field}"
        );
    }
    for tool in [
        "mant_documents_list",
        "mant_document_outline",
        "mant_document_get",
        "mant_document_explain",
        "mant_document_search",
    ] {
        assert!(
            PROTOCOL_REFERENCE.contains(tool),
            "the protocol reference must document {tool}"
        );
    }
}

#[test]
fn invalid_stdin_request_uses_status_two_without_runtime_noise() {
    let mut child = Command::new(executable())
        .args(["--request-json", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"git"},"view":{"kind":"full"},"futureField":true}"#,
        )
        .expect("write request");
    let output = child.wait_with_output().expect("wait for mant");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.starts_with("mant: invalid query request JSON:"));
    assert!(!diagnostic.contains("panicked at"));
    assert!(!diagnostic.contains("stack backtrace"));
}

#[test]
fn direct_stdin_reads_markdown_without_extending_the_request_schema() {
    let mut child = Command::new(executable())
        .args(["-", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mant");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"# Piped\n\n## Options\n\n- `--help`: Show help.\n")
        .expect("write Markdown");
    let output = child.wait_with_output().expect("wait for mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("query JSON");
    assert_eq!(value["label"], "stdin");
    assert_eq!(value["document"]["source"]["format"], "markdown");
    assert!(value["document"]["source"].get("path").is_none());
    assert!(value.get("tldr").is_none());
    assert_eq!(
        value["document"]["sections"][0]["blocks"][0]["items"][0]["identity"]["names"][0],
        "--help"
    );
}

#[test]
fn document_sources_update_on_demand_and_support_explicit_selection() {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-document-source-process-{}",
        std::process::id()
    ));
    let repository = fixture_root.join("repository");
    let data_root = registered_documents_dir(&fixture_root)
        .parent()
        .expect("application data root")
        .to_owned();
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

    let mut update = Command::new(executable());
    configure_registered_documents(&mut update, &fixture_root);
    let first = update
        .args(["--update-docs", "--compact"])
        .output()
        .expect("update document source");
    assert!(first.status.success(), "{first:?}");
    assert!(first.stderr.is_empty());
    let result: serde_json::Value = serde_json::from_slice(&first.stdout).expect("update JSON");
    assert_eq!(result["schema"], "mant.sources-update/v1");
    assert_eq!(result["sources"][0]["source"], "team");
    assert_eq!(result["sources"][0]["action"], "updated");
    assert_eq!(result["sources"][0]["documents"], 1);
    assert!(data_root.join("sources/team/source-tool.md").is_file());
    assert!(data_root.join("sources/team/.mant-source.toml").is_file());
    assert!(!data_root.join("sources/team/README.md").exists());

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
    assert_eq!(result["document"]["meta"]["title"], "Source tool");

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
    assert_eq!(result["document"]["meta"]["title"], "Root tool");

    let unknown =
        run_with_registered_documents(&fixture_root, &["source-tool", "--source", "missing"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("is not configured"));

    fs::remove_dir_all(fixture_root).expect("remove document source fixture");
}

#[test]
fn document_source_failures_keep_a_complete_json_report() {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-document-source-failure-process-{}",
        std::process::id()
    ));
    let data_root = registered_documents_dir(&fixture_root)
        .parent()
        .expect("application data root")
        .to_owned();
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
    assert!(output.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("failure report JSON");
    assert_eq!(report["sources"][0]["source"], "broken");
    assert_eq!(report["sources"][0]["action"], "failed");
    assert!(report["sources"][0]["error"].as_str().is_some());
    assert!(!data_root.join("sources/broken").exists());

    fs::remove_dir_all(fixture_root).expect("remove failure fixture");
}

#[test]
fn direct_and_protocol_queries_read_local_markdown_files_by_path() {
    let path = markdown_fixture_path();
    fs::write(&path, "# Local\n\nBody.\n").expect("write Markdown fixture");

    let direct = Command::new(executable())
        .args([
            path.to_str().expect("UTF-8 path"),
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("query Markdown file");
    assert!(direct.status.success());
    assert!(direct.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&direct.stdout).expect("query JSON");
    assert_eq!(value["document"]["meta"]["title"], "Local");
    assert_eq!(
        value["document"]["source"]["path"],
        path.to_str().expect("UTF-8 path")
    );
    assert!(value.get("tldr").is_none());

    let mut child = Command::new(executable())
        .args(["--request-json", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start protocol query");
    let request = serde_json::json!({
        "schema": "mant.request/v5",
        "input": {
            "kind": "markdown-file",
            "path": path.to_str().expect("UTF-8 path"),
        },
        "view": { "kind": "full" },
    });
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(request.to_string().as_bytes())
        .expect("write request");
    let protocol = child.wait_with_output().expect("wait for protocol query");
    let _ = fs::remove_file(&path);

    assert!(protocol.status.success());
    assert!(protocol.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&protocol.stdout).expect("query JSON");
    assert_eq!(
        value["label"].as_str(),
        Some(
            path.file_name()
                .expect("filename")
                .to_str()
                .expect("UTF-8 filename")
        )
    );
    assert_eq!(value["document"]["source"]["format"], "markdown");
}

#[test]
fn cli_json_remains_the_lowering_diagnostic_surface() {
    let path = std::env::temp_dir().join(format!(
        "mant-markdown-diagnostic-process-{}.md",
        std::process::id()
    ));
    fs::write(
        &path,
        "# Diagnostic fixture\n\n> preserved unsupported quote\n",
    )
    .expect("write diagnostic Markdown fixture");

    let output = Command::new(executable())
        .args([
            path.to_str().expect("UTF-8 path"),
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("query diagnostic Markdown file");
    fs::remove_file(path).expect("remove diagnostic fixture");

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("query JSON");
    assert_eq!(
        value["document"]["diagnostics"][0]["code"],
        "markdown.unsupported"
    );
}

#[test]
fn unqualified_names_prefer_registered_markdown() {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-registered-document-process-{}",
        std::process::id()
    ));
    let documents = registered_documents_dir(&fixture_root);
    fs::create_dir_all(&documents).expect("create registered document directory");
    let path = documents.join("process-registered.md");
    fs::write(
        &path,
        "# Registered\n\nBody from the registered document.\n",
    )
    .expect("write registered document");

    let mut command = Command::new(executable());
    configure_registered_documents(&mut command, &fixture_root);
    let output = command
        .args(["process-registered", "--format", "json", "--compact"])
        .output()
        .expect("query registered document");

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("query JSON");
    assert_eq!(value["label"], "process-registered");
    assert_eq!(value["document"]["meta"]["title"], "Registered");
    assert_eq!(value["document"]["source"]["format"], "markdown");
    let source_path = value["document"]["source"]["path"]
        .as_str()
        .expect("registered source path");
    assert_eq!(
        fs::canonicalize(source_path).expect("canonical source path"),
        fs::canonicalize(&path).expect("canonical fixture path")
    );

    fs::remove_dir_all(fixture_root).expect("remove registered document fixture");
}

#[cfg(unix)]
#[test]
fn manual_option_bypasses_registered_markdown_with_the_same_name() {
    let root = std::env::temp_dir().join(format!(
        "mant-manual-source-policy-process-{}",
        std::process::id()
    ));
    let manual_root = root.join("manuals");
    let documents = registered_documents_dir(&root);
    fs::create_dir_all(&documents).expect("create registration root");
    fs::create_dir_all(manual_root.join("man1")).expect("create manual root");
    fs::write(
        documents.join("source-policy.md"),
        "# Registered document\n\nRegistered body.\n",
    )
    .expect("write registered document");
    fs::write(
        manual_root.join("man1/source-policy.1"),
        ".TH SOURCE-POLICY 1\n.SH NAME\nsource-policy \\- native manual\n",
    )
    .expect("write native manual");

    let run = |manual: bool| {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        command
            .arg("source-policy")
            .args(manual.then_some("--manual"))
            .args(["--format", "json", "--compact"])
            .env("MANT_MANPATH", &manual_root);
        command.output().expect("query source policy")
    };

    let registered = run(false);
    assert!(registered.status.success(), "{registered:?}");
    let registered: serde_json::Value =
        serde_json::from_slice(&registered.stdout).expect("registered JSON");
    assert_eq!(registered["document"]["source"]["format"], "markdown");

    let manual = run(true);
    assert!(manual.status.success(), "{manual:?}");
    assert!(manual.stderr.is_empty());
    let manual: serde_json::Value = serde_json::from_slice(&manual.stdout).expect("manual JSON");
    assert_eq!(manual["document"]["source"]["format"], "man");
    assert_eq!(manual["document"]["meta"]["section"], "1");

    fs::remove_dir_all(root).expect("remove source-policy fixture");
}

#[cfg(windows)]
#[test]
fn manual_option_explains_the_windows_capability_boundary() {
    let output = Command::new(executable())
        .args(["process-native-manual", "--manual"])
        .output()
        .expect("query an unavailable native manual");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(
        stderr.contains("native manual pages are unavailable on this platform"),
        "{stderr}"
    );
    assert!(
        stderr.contains("register a Markdown document named 'process-native-manual'"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn registered_names_ignore_nested_directories_and_symlinks() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "mant-linked-document-process-{}",
        std::process::id()
    ));
    let provider = root.join("provider-docs");
    let documents = registered_documents_dir(&root);
    fs::create_dir_all(&documents).expect("create registration root");
    fs::create_dir_all(&provider).expect("create provider directory");
    fs::write(
        provider.join("process-linked.md"),
        "# Linked\n\nBody from another tool.\n",
    )
    .expect("write provider document");
    symlink(&provider, documents.join("provider")).expect("link provider directory");

    let mut command = Command::new(executable());
    configure_registered_documents(&mut command, &root);
    let output = command
        .args(["process-linked", "--format", "json", "--compact"])
        .output()
        .expect("query ignored linked document");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("# Linked"));

    fs::remove_dir_all(root).expect("remove linked document fixture");
}

#[cfg(unix)]
#[test]
fn manual_queries_use_native_paths_without_a_man_executable() {
    let root =
        std::env::temp_dir().join(format!("mant-native-manual-process-{}", std::process::id()));
    let section = root.join("man1");
    fs::create_dir_all(&section).expect("create manual section");
    fs::write(
        section.join("native-only.1"),
        ".TH NATIVE-ONLY 1\n.SH NAME\nnative-only \\- indexed without man\n",
    )
    .expect("write manual source");

    let output = Command::new(executable())
        .args(["native-only", "--format", "json", "--compact"])
        .env("MANT_MANPATH", &root)
        .env("PATH", "")
        .output()
        .expect("query native manual index");

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("query JSON");
    assert_eq!(value["label"], "native-only");
    assert_eq!(value["document"]["meta"]["section"], "1");
    assert_eq!(value["document"]["source"]["format"], "man");

    fs::remove_dir_all(root).expect("remove native manual fixture");
}

#[test]
fn markdown_root_content_is_discoverable_selectable_and_searchable() {
    let path = std::env::temp_dir().join(format!(
        "mant-markdown-root-process-{}.md",
        std::process::id()
    ));
    fs::write(
        &path,
        "Read the preface needle first.\n\n# Guide\n\nSection body.\n",
    )
    .expect("write Markdown fixture");
    let path = path.to_str().expect("UTF-8 path");

    let run_json = |arguments: &[&str]| {
        let output = Command::new(executable())
            .args(arguments)
            .args(["--format", "json", "--compact"])
            .output()
            .expect("query Markdown projection");
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("projection JSON")
    };

    let outline = run_json(&[path, "--outline=sections"]);
    assert_eq!(outline["nodes"][0]["kind"], "document-root");
    assert_eq!(outline["nodes"][0]["path"], "root");
    assert_eq!(outline["nodes"].as_array().map(Vec::len), Some(1));

    let excerpt = run_json(&[path, "--node", "root"]);
    assert_eq!(excerpt["selections"][0]["kind"], "document-root");
    assert_eq!(
        excerpt["selections"][0]["blocks"][0]["children"][0]["value"],
        "Read the preface needle first."
    );

    let search = run_json(&[path, "--search", "preface needle"]);
    assert_eq!(search["total"], 1);
    assert_eq!(search["matches"][0]["node"]["kind"], "document-root");
    assert_eq!(search["matches"][0]["node"]["path"], "root");

    fs::remove_file(path).expect("remove Markdown fixture");
}

fn markdown_fixture_path() -> PathBuf {
    std::env::temp_dir().join(format!("mant-markdown-process-{}.md", std::process::id()))
}

#[test]
fn unknown_options_do_not_expose_rust_source_excerpts() {
    let output = Command::new(executable())
        .arg("--not-an-option")
        .output()
        .expect("run mant");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.starts_with("error: unexpected argument '--not-an-option'"));
    assert!(diagnostic.contains("Usage: mant"));
    assert!(diagnostic.contains("For more information, try '--help'."));
}
