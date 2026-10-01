use super::process_support::{executable, run_with_registered_documents, scope_document_paths};
use super::support::{configure_registered_documents, registered_documents_dir};
use std::fs;
use std::io::Write as _;
use std::process::Command;
use std::process::Stdio;

#[test]
fn dynamic_newline_selectors_are_rejected_before_diagnostic_interpolation() {
    let root = std::env::temp_dir().join(format!("mant-diagnostic-newline-{}", std::process::id()));
    let manual_root = root.join("manuals");
    fs::create_dir_all(&manual_root).expect("create empty manual root");
    let mut command = Command::new(executable());
    configure_registered_documents(&mut command, &root);
    let output = command
        .arg("missing\nhint: forged advice")
        .args(["--format", "markdown", "--color", "always"])
        .env("MANT_MANPATH", &manual_root)
        .output()
        .expect("query a selector containing a newline");
    fs::remove_dir_all(root).expect("remove diagnostic fixture");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(
        diagnostic.contains("document selector must not contain control characters"),
        "{diagnostic:?}"
    );
    assert!(!diagnostic.contains("forged advice"));
    assert_eq!(diagnostic.matches("\u{1b}[1m\u{1b}[36mhint:").count(), 0);
}

#[test]
fn every_cli_explain_surface_rejects_oversized_entries_before_lookup() {
    let root = std::env::temp_dir().join(format!(
        "mant-oversized-explain-process-{}",
        std::process::id()
    ));
    let manual_root = root.join("manuals");
    fs::create_dir_all(&manual_root).expect("create empty manual root");
    let entry = "x".repeat(513);

    for arguments in [
        vec!["missing", "--explain", entry.as_str()],
        vec![
            "--document",
            "missing",
            "--follow-links",
            "--explain",
            entry.as_str(),
        ],
    ] {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        let output = command
            .args(arguments)
            .env("MANT_MANPATH", &manual_root)
            .output()
            .expect("reject oversized semantic entry");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
        assert!(
            error.contains("semantic entry must not exceed 512 Unicode scalar values"),
            "{error}"
        );
        assert!(
            !error.contains(&entry),
            "oversized input must not be echoed"
        );
    }

    let mut command = Command::new(executable());
    configure_registered_documents(&mut command, &root);
    let mut child = command
        .args([
            "--input",
            "-",
            "--input-format",
            "markdown",
            "--explain",
            entry.as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start stdin query");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(b"# Demo\n\nBody.\n")
        .expect("write Markdown stdin");
    let output = child.wait_with_output().expect("finish stdin query");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(
        error.contains("semantic entry must not exceed 512 Unicode scalar values"),
        "{error}"
    );
    assert!(
        !error.contains(&entry),
        "oversized input must not be echoed"
    );

    fs::remove_dir_all(root).expect("remove oversized selector fixture");
}

#[test]
#[cfg(feature = "roff")]
fn one_owner_explanation_page_keeps_its_context_in_every_cli_format() {
    let source = b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.TP\n.B --second\nOnly the second changes output.\n";
    for format in ["text", "markdown", "json"] {
        let mut child = Command::new(executable())
            .args([
                "--input",
                "-",
                "--input-format",
                "roff",
                "--explain=--first",
                "--limit",
                "1",
                "--format",
                format,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(source).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Only the second changes output"), "{text}");
        if format == "json" {
            let response: mant_protocol::QueryExplanation = serde_json::from_str(&text).unwrap();
            assert_eq!(response.counts.direct_entry.returned, 1);
            assert_eq!(response.supports.len(), 1);
            assert!(response.evidence[0].covered_by_support(&response.supports));
        } else {
            let heading = if format == "markdown" {
                "Declaration\\-group context"
            } else {
                "Declaration-group context"
            };
            assert!(text.contains(heading), "{text}");
            assert!(!text.contains("no independent description"), "{text}");
        }
    }
}

#[test]
fn regex_search_rejects_patterns_that_can_split_utf8_characters() {
    let path =
        std::env::temp_dir().join(format!("mant-utf8-regex-process-{}.md", std::process::id()));
    fs::write(&path, "# Unicode\n\nFollow the → arrow.\n").expect("write Unicode fixture");

    let output = Command::new(executable())
        .arg("--input")
        .arg(&path)
        .args(["--search", "(?-u:.)", "--regex", "--color", "never"])
        .output()
        .expect("run invalid regex search");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("UTF-8 character boundaries"),
        "{output:?}"
    );
    fs::remove_file(path).expect("remove Unicode fixture");
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
            "--input",
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
fn cli_and_request_outlines_report_rejected_semantic_entries() {
    let path = std::env::temp_dir().join(format!(
        "mant-semantic-outline-process-{}.md",
        std::process::id()
    ));
    fs::write(
        &path,
        "# Incomplete entries\n\n<!-- mant:entries role=option case=insensitive -->\n- `/valid`: Valid.\n- `/driver..exclude`: Invalid.\n",
    )
    .expect("write semantic outline fixture");

    let direct = Command::new(executable())
        .args([
            "--input",
            path.to_str().expect("UTF-8 path"),
            "--outline",
            "--outline-entries",
            "all",
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("query direct outline");
    assert!(direct.status.success(), "{direct:?}");
    assert!(direct.stderr.is_empty());
    let direct: serde_json::Value =
        serde_json::from_slice(&direct.stdout).expect("direct outline JSON");
    assert_eq!(direct["semanticsComplete"], false);
    assert_eq!(
        direct["diagnostics"][0]["code"],
        "markdown.semantic-entry.invalid-option-name"
    );

    let mut child = Command::new(executable())
        .args(["--request-json", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start outline request");
    let request = serde_json::json!({
        "schema": "mant.request/v0.12",
        "input": {
            "kind": "file",
            "path": path.to_str().expect("UTF-8 path"),
            "format": "markdown",
        },
        "view": { "kind": "outline", "entries": { "kind": "all" } },
    });
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(request.to_string().as_bytes())
        .expect("write outline request");
    let protocol = child.wait_with_output().expect("wait for outline request");
    fs::remove_file(path).expect("remove semantic outline fixture");

    assert!(protocol.status.success(), "{protocol:?}");
    assert!(protocol.stderr.is_empty());
    let protocol: serde_json::Value =
        serde_json::from_slice(&protocol.stdout).expect("request outline JSON");
    assert_eq!(protocol["semanticsComplete"], false);
    assert_eq!(protocol["diagnostics"], direct["diagnostics"]);
}

#[test]
fn exact_semantic_option_spellings_survive_the_cli_boundary() {
    let path = std::env::temp_dir().join(format!(
        "mant-exact-semantic-selector-process-{}.md",
        std::process::id()
    ));
    fs::write(
        &path,
        "# Exact selectors\n\n## Certificate options\n\n<!-- mant:entries role=option case=insensitive -->\n- `-ca.cert`: Retrieve a CA certificate.\n- `-ca.chain`: Retrieve a CA chain.\n- `--foo.bar=VALUE`: Select a dotted value.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `?`: Display positional help.\n\n## Help options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/?`, `-?`: Display option help.\n",
    )
    .expect("write exact-selector fixture");
    let path = path.to_str().expect("UTF-8 path");

    let outline = Command::new(executable())
        .args([
            "--input",
            path,
            "--outline",
            "--outline-entries",
            "all",
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("query exact-selector outline");
    let dotted = Command::new(executable())
        .args([
            "--input",
            path,
            "--explain=-ca.cert",
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("explain dotted option");
    let positional_help = Command::new(executable())
        .args([
            "--input",
            path,
            "--explain=?",
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("explain exact help command");
    fs::remove_file(path).expect("remove exact-selector fixture");

    assert!(outline.status.success(), "{outline:?}");
    assert!(outline.stderr.is_empty());
    let outline: serde_json::Value = serde_json::from_slice(&outline.stdout).expect("outline JSON");
    assert!(outline.get("semanticsComplete").is_none());
    let certificate = outline["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["path"] == "1")
        .expect("certificate section");
    assert_eq!(certificate["children"][0]["names"][0], "-ca.cert");
    assert_eq!(certificate["children"][1]["names"][0], "-ca.chain");
    assert_eq!(certificate["children"][2]["names"][0], "--foo.bar");

    assert!(dotted.status.success(), "{dotted:?}");
    assert!(dotted.stderr.is_empty());
    let dotted: serde_json::Value = serde_json::from_slice(&dotted.stdout).expect("dotted JSON");
    assert_eq!(dotted["evidence"][0]["entry"]["names"][0], "-ca.cert");

    assert!(positional_help.status.success(), "{positional_help:?}");
    assert!(positional_help.stderr.is_empty());
    let positional_help: serde_json::Value =
        serde_json::from_slice(&positional_help.stdout).expect("help JSON");
    assert_eq!(
        positional_help["evidence"][0]["entry"]["kind"]["kind"],
        "command"
    );
}

#[test]
fn text_outlines_include_resolved_semantic_relationships() {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-outline-relationships-process-{}",
        std::process::id()
    ));
    let documents = registered_documents_dir(&fixture_root);
    let indexes = documents.join("indexes");
    fs::create_dir_all(&indexes).expect("create outline relationship documents");
    fs::write(
        indexes.join("tools.md"),
        "# Tools\n\n<!-- mant:entries role=command case=insensitive -->\n- [`winget`](winget.md#install), [`w`](winget.md#install): See the [description](description.md).\n\n  <!-- mant:domain entries=values.md roles=value -->\n",
    )
    .expect("write outline relationship index");
    fs::write(indexes.join("winget.md"), "# Winget\n\n## Install\n")
        .expect("write linked document");
    fs::write(indexes.join("description.md"), "# Description\n")
        .expect("write description document");
    fs::write(indexes.join("values.md"), "# Values\n").expect("write value-domain document");

    let output = run_with_registered_documents(
        &fixture_root,
        &[
            "indexes/tools",
            "--outline",
            "--outline-entries",
            "all",
            "--format",
            "text",
            "--color",
            "never",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).expect("UTF-8 text outline");
    assert!(
        text.contains(
            "documents: winget → documents/indexes/winget#install, w → documents/indexes/winget#install"
        ),
        "{text}"
    );
    assert!(
        text.contains("values: value in documents/indexes/values"),
        "{text}"
    );

    let scope = run_with_registered_documents(
        &fixture_root,
        &[
            "indexes/tools",
            "--follow-links",
            "--max-depth",
            "8",
            "--max-documents",
            "2",
            "--search",
            "heading",
            "--format",
            "json",
            "--compact",
        ],
    );
    assert!(scope.status.success(), "{scope:?}");
    let scope: serde_json::Value =
        serde_json::from_slice(&scope.stdout).expect("scope relationship JSON");
    assert_eq!(
        scope_document_paths(&scope),
        ["indexes/tools", "indexes/winget"]
    );

    fs::remove_dir_all(fixture_root).expect("remove outline relationship fixture");
}
