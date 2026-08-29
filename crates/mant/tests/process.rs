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

fn registered_data_root(home: &std::path::Path) -> PathBuf {
    registered_documents_dir(home)
        .parent()
        .expect("registered documents always have an application data root")
        .to_owned()
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

const PROTOCOL_REFERENCE: &str = include_str!("../../../docs/manuals/mant-protocol.md");

#[test]
fn help_groups_the_public_query_surface() {
    let output = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run mant");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("mant <SELECTOR> [OPTIONS]"));
    assert!(help.contains("mant --input README.md"));
    assert!(help.contains("cat guide.md | mant --input - --input-format markdown"));
    assert!(help.contains("Document selection:"));
    assert!(help.contains("Search:"));
    assert!(help.contains("Integration:"));
    assert!(help.contains("Diagnostics:"));
    assert!(help.contains("Reading:"));
    assert!(help.contains("-h, --help"));
    assert!(help.contains("--ui"));
    assert!(help.contains("-V, --version"));
    assert!(help.contains("--format <FORMAT>"));
    assert!(help.contains("--preserve-anchors"));
    assert!(help.contains("--color <COLOR>"));
    assert!(help.contains("--update-tldr"));
    assert!(help.contains("--update-docs"));
    assert!(help.contains("--prune-docs"));
    assert!(help.contains("--doctor"));
    assert!(help.contains("--dry-run"));
    assert!(help.contains("--source <SOURCE>"));
    assert!(help.contains("--protocol-version"));
    assert!(help.contains("--schema <CONTRACT>"));
    assert!(help.contains("--mcp"));
    assert!(help.contains("--explain <ENTRY>"));
    assert!(help.contains("--search <PATTERN>"));
    assert!(help.contains("--manual"));
    assert!(help.contains("--tldr"));
    assert!(!help.contains("--force-libmandoc"));
    assert!(!help.contains("--force-groff"));
    assert!(!help.contains("--json"));
    assert!(!help.contains("update tldr"));
}

#[test]
fn clap_color_is_terminal_aware_and_explicitly_controllable() {
    let run = |arguments: &[&str]| {
        Command::new(executable())
            .args(arguments)
            .output()
            .expect("run mant color fixture")
    };

    let automatic = run(&["--help"]);
    assert!(automatic.status.success());
    assert!(!automatic.stdout.contains(&0x1b));

    let colored_help = run(&["--help", "--color", "always"]);
    assert!(colored_help.status.success());
    assert!(colored_help.stderr.is_empty());
    assert!(colored_help.stdout.contains(&0x1b));

    let plain_help = run(&["--help", "--color", "never"]);
    assert!(plain_help.status.success());
    assert!(!plain_help.stdout.contains(&0x1b));

    let colored_error = run(&["--color", "always"]);
    assert_eq!(colored_error.status.code(), Some(2));
    assert!(colored_error.stdout.is_empty());
    assert!(colored_error.stderr.contains(&0x1b));

    let colored_semantic_error = run(&["git", "--no-pager", "--color", "always"]);
    assert_eq!(colored_semantic_error.status.code(), Some(2));
    assert!(colored_semantic_error.stdout.is_empty());
    assert!(colored_semantic_error.stderr.contains(&0x1b));

    let colored_runtime_error = run(&[
        "definitely-not-a-real-manual-for-colour",
        "--manual",
        "--color",
        "always",
    ]);
    assert_eq!(colored_runtime_error.status.code(), Some(1));
    assert!(colored_runtime_error.stdout.is_empty());
    assert!(colored_runtime_error.stderr.contains(&0x1b));

    let plain_runtime_error = run(&[
        "definitely-not-a-real-manual-for-colour",
        "--manual",
        "--color",
        "never",
    ]);
    assert_eq!(plain_runtime_error.status.code(), Some(1));
    assert!(!plain_runtime_error.stderr.contains(&0x1b));

    let protocol = run(&["--protocol-version", "--compact", "--color", "always"]);
    assert!(protocol.status.success());
    assert!(!protocol.stdout.contains(&0x1b));
    serde_json::from_slice::<serde_json::Value>(&protocol.stdout).expect("plain protocol JSON");
}

#[test]
fn dynamic_newlines_cannot_forge_colored_diagnostic_lines() {
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

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(
        diagnostic.contains("missing�hint: forged advice"),
        "{diagnostic:?}"
    );
    assert!(!diagnostic.contains("\nhint: forged advice"));
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

    fs::remove_dir_all(root).expect("remove oversized selector fixture");
}

#[test]
fn partial_query_text_is_colored_only_when_the_stream_policy_allows_it() {
    let path = std::env::temp_dir().join(format!(
        "mant-colored-query-process-{}.md",
        std::process::id()
    ));
    fs::write(
        &path,
        "# demo\n\n## Options\n\n<!-- mant:entries role=option -->\n- `--color WHEN`: Select terminal color.\n",
    )
    .expect("write colored query fixture");
    let run = |arguments: &[&str]| {
        Command::new(executable())
            .arg("--input")
            .arg(&path)
            .args(arguments)
            .output()
            .expect("run colored query fixture")
    };

    let automatic = run(&["--explain=--color"]);
    assert!(automatic.status.success(), "{automatic:?}");
    assert!(!automatic.stdout.contains(&0x1b));

    let colored = run(&["--explain=--color", "--color", "always"]);
    assert!(colored.status.success(), "{colored:?}");
    assert!(colored.stdout.contains(&0x1b));

    let plain = run(&["--search", "color", "--color", "never"]);
    assert!(plain.status.success(), "{plain:?}");
    assert!(!plain.stdout.contains(&0x1b));

    let json = run(&[
        "--search",
        "color",
        "--format",
        "json",
        "--compact",
        "--color",
        "always",
    ]);
    assert!(json.status.success(), "{json:?}");
    assert!(!json.stdout.contains(&0x1b));
    serde_json::from_slice::<serde_json::Value>(&json.stdout).expect("plain search JSON");

    fs::remove_file(path).expect("remove colored query fixture");
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
fn doctor_is_offline_read_only_and_supports_stable_json() {
    let home = std::env::temp_dir().join(format!("mant-doctor-process-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).expect("doctor home");
    let data_root = registered_data_root(&home);
    let manual_root = home.join("manuals");
    let tldr_root = home.join("tldr");

    let run = |arguments: &[&str]| {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &home);
        command
            .env("MANT_MANPATH", &manual_root)
            .env("MANT_TLDR_DIR", &tldr_root)
            .args(arguments)
            .output()
            .expect("run isolated doctor")
    };

    let text = run(&["--doctor", "--color", "never"]);
    assert!(text.status.success());
    assert!(text.stderr.is_empty());
    assert!(!text.stdout.contains(&0x1b));
    let text = String::from_utf8(text.stdout).expect("doctor text");
    assert!(text.starts_with("ManT doctor\n\n"));
    assert!(text.contains("runtime.libmandoc"));
    assert!(text.contains("sources.configuration"));
    assert!(text.contains("manuals.index"));
    assert!(text.contains("tldr.cache"));

    let json = run(&[
        "--doctor",
        "--format",
        "json",
        "--compact",
        "--color",
        "always",
    ]);
    assert!(json.status.success());
    assert!(json.stderr.is_empty());
    assert!(!json.stdout.contains(&0x1b));
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).expect("doctor JSON");
    assert_eq!(report["schema"], "mant.doctor/v1");
    assert_eq!(
        report["environment"]["dataRoot"],
        data_root.to_string_lossy().as_ref()
    );
    assert_eq!(
        report["environment"]["configPath"],
        data_root.join("sources.toml").to_string_lossy().as_ref()
    );
    assert_eq!(
        report["environment"]["documentsRoot"],
        registered_documents_dir(&home).to_string_lossy().as_ref()
    );
    assert_eq!(
        report["environment"]["sourcesRoot"],
        data_root.join("sources").to_string_lossy().as_ref()
    );
    assert_eq!(
        report["environment"]["manualRoots"],
        serde_json::json!([manual_root.to_string_lossy()])
    );
    assert_eq!(
        report["environment"]["tldrRoots"],
        serde_json::json!([tldr_root.to_string_lossy()])
    );
    let checks = report["checks"].as_array().expect("doctor checks");
    let status = |code: &str| {
        checks
            .iter()
            .find(|check| check["code"] == code)
            .unwrap_or_else(|| panic!("missing doctor check {code}"))["status"]
            .as_str()
            .expect("doctor status")
    };
    assert_eq!(status("paths.data-root"), "info");
    assert_eq!(status("sources.configuration"), "info");
    assert_eq!(
        status("manuals.index"),
        if cfg!(windows) { "info" } else { "warning" }
    );
    assert_eq!(status("tldr.cache"), "warning");
    assert!(!data_root.exists(), "doctor must not create the data root");

    let schema = run(&["--schema", "doctor", "--compact"]);
    assert!(schema.status.success());
    let schema: serde_json::Value = serde_json::from_slice(&schema.stdout).expect("doctor schema");
    assert_eq!(schema["$id"], "urn:mant:doctor:v1");

    fs::remove_dir_all(home).expect("doctor cleanup");
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
fn command_sections_qualify_tldr_topics_without_becoming_part_of_the_name() {
    let root =
        std::env::temp_dir().join(format!("mant-section-tldr-process-{}", std::process::id()));
    let tldr_root = root.join("tldr");
    fs::create_dir_all(tldr_root.join("pages/common")).expect("create tldr root");
    fs::write(
        tldr_root.join("pages/common/tar.md"),
        "# tar\n\n> Archive files.\n\n- List an archive:\n\n`tar tf {{archive.tar}}`\n",
    )
    .expect("write tldr page");
    fs::write(
        tldr_root.join("pages/common/command.1.md"),
        "# command.1\n\n> Dotted exact topic.\n\n- Run it:\n\n`command.1`\n",
    )
    .expect("write dotted tldr page");

    for arguments in [
        ["1", "tar", "--tldr"].as_slice(),
        ["tar(1)", "--tldr"].as_slice(),
        ["manual/1/tar", "--tldr"].as_slice(),
    ] {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        let output = command
            .args(arguments)
            .env("MANT_TLDR_DIR", &tldr_root)
            .output()
            .expect("run section-qualified tldr query");

        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).expect("UTF-8 tldr output");
        assert!(text.contains("Archive files."));
        assert!(!text.contains("1-tar"));
    }

    let mut non_command = Command::new(executable());
    configure_registered_documents(&mut non_command, &root);
    let output = non_command
        .args(["5", "tar", "--tldr"])
        .env("MANT_TLDR_DIR", &tldr_root)
        .output()
        .expect("run non-command section tldr query");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.contains("section '5'"), "{diagnostic}");
    assert!(
        diagnostic.contains("section families 1 and 8"),
        "{diagnostic}"
    );

    let mut dotted = Command::new(executable());
    configure_registered_documents(&mut dotted, &root);
    let output = dotted
        .args(["command.1", "--tldr"])
        .env("MANT_TLDR_DIR", &tldr_root)
        .output()
        .expect("run exact dotted tldr query");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).expect("UTF-8 tldr output");
    assert!(text.contains("Dotted exact topic."), "{text}");

    fs::remove_dir_all(root).expect("remove section tldr fixture");
}

#[test]
fn cached_tldr_requires_an_explicit_tldr_query_when_the_document_is_missing() {
    let root = std::env::temp_dir().join(format!("mant-tldr-only-process-{}", std::process::id()));
    let manual_root = root.join("manuals");
    let tldr_root = root.join("tldr");
    fs::create_dir_all(&manual_root).expect("create empty manual root");
    fs::create_dir_all(tldr_root.join("pages/common")).expect("create tldr root");
    fs::write(
        tldr_root.join("pages/common/quick-only.md"),
        "# quick-only\n\n> Cached quick reference.\n\n- Run it:\n\n`quick-only`\n",
    )
    .expect("write tldr page");

    let run = |arguments: &[&str]| {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        command
            .arg("quick-only")
            .args(arguments)
            .env("MANT_MANPATH", &manual_root)
            .env("MANT_TLDR_DIR", &tldr_root)
            .output()
            .expect("query tldr-only topic")
    };

    let ordinary = run(&["--format", "markdown"]);
    assert_eq!(ordinary.status.code(), Some(1));
    assert!(ordinary.stdout.is_empty());
    let diagnostic = String::from_utf8(ordinary.stderr).expect("ordinary diagnostic");
    assert!(diagnostic.contains("could not load manual 'quick-only'"));
    assert!(diagnostic.contains("a tldr entry is available"));
    assert!(diagnostic.contains("mant quick-only --tldr"));

    let colored = run(&["--format", "markdown", "--color", "always"]);
    assert_eq!(colored.status.code(), Some(1));
    let diagnostic = String::from_utf8(colored.stderr).expect("colored diagnostic");
    assert!(diagnostic.contains("\u{1b}[1m\u{1b}[31mmant:\u{1b}[0m"));
    assert!(diagnostic.contains("\u{1b}[1m\u{1b}[36mhint:\u{1b}[0m"));

    let explicit = run(&["--tldr"]);
    assert!(explicit.status.success(), "{explicit:?}");
    assert!(explicit.stderr.is_empty());
    assert!(
        String::from_utf8(explicit.stdout)
            .expect("tldr output")
            .contains("Cached quick reference.")
    );

    fs::remove_dir_all(root).expect("remove tldr-only fixture");
}

#[test]
fn explicit_tldr_queries_follow_document_source_priority() {
    let root =
        std::env::temp_dir().join(format!("mant-tldr-priority-process-{}", std::process::id()));
    let documents = registered_documents_dir(&root);
    let data_root = registered_data_root(&root);
    let preferred = data_root.join("sources/preferred");
    let fallback = data_root.join("sources/fallback");
    let tldr_root = root.join("tldr");
    let manual_root = root.join("manuals");
    for directory in [
        &preferred,
        &fallback,
        &manual_root,
        &tldr_root.join("pages/common"),
    ] {
        fs::create_dir_all(directory).expect("create tldr priority fixture");
    }
    fs::write(
        data_root.join("sources.toml"),
        "[preferred]\nrepo = 'https://example.invalid/preferred.git'\nbranch = 'main'\npriority = 2\n\n[fallback]\nrepo = 'https://example.invalid/fallback.git'\nbranch = 'main'\npriority = -1\n",
    )
    .expect("write source configuration");
    for (directory, source) in [(&preferred, "preferred"), (&fallback, "fallback")] {
        fs::write(
            directory.join(".mant-source.toml"),
            format!("source = '{source}'\n"),
        )
        .expect("write installed-source marker");
    }
    fs::write(
        preferred.join("tool.md"),
        "# Preferred tool\n\nFull body only.\n",
    )
    .expect("write preferred document");
    fs::write(
        fallback.join("tool.md"),
        embedded_tldr_fixture("Fallback quick reference."),
    )
    .expect("write fallback document");
    let cached = tldr_root.join("pages/common/tool.md");
    fs::write(
        &cached,
        "# tool\n\n> Cached quick reference.\n\n- Run it:\n\n`tool`\n",
    )
    .expect("write cached tldr page");

    let run = || {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        command
            .args(["tool", "--tldr", "--color", "never"])
            .env("MANT_MANPATH", &manual_root)
            .env("MANT_TLDR_DIR", &tldr_root)
            .output()
            .expect("query prioritized tldr")
    };

    let cached_result = run();
    assert!(cached_result.status.success(), "{cached_result:?}");
    assert!(String::from_utf8_lossy(&cached_result.stdout).contains("Cached quick reference."));

    fs::write(
        preferred.join("tool.md"),
        embedded_tldr_fixture("Preferred quick reference."),
    )
    .expect("add preferred embedded tldr");
    let preferred_result = run();
    assert!(preferred_result.status.success(), "{preferred_result:?}");
    assert!(
        String::from_utf8_lossy(&preferred_result.stdout).contains("Preferred quick reference.")
    );

    fs::create_dir_all(&documents).expect("create personal documents");
    fs::write(
        documents.join("tool.md"),
        embedded_tldr_fixture("Personal quick reference."),
    )
    .expect("write personal embedded tldr");
    let personal_result = run();
    assert!(personal_result.status.success(), "{personal_result:?}");
    assert!(String::from_utf8_lossy(&personal_result.stdout).contains("Personal quick reference."));
    fs::remove_file(documents.join("tool.md")).expect("remove personal embedded tldr");

    fs::write(
        preferred.join("tool.md"),
        "# Preferred tool\n\nFull body only.\n",
    )
    .expect("restore preferred document");
    fs::remove_file(cached).expect("remove cached tldr");
    let fallback_result = run();
    assert!(fallback_result.status.success(), "{fallback_result:?}");
    assert!(String::from_utf8_lossy(&fallback_result.stdout).contains("Fallback quick reference."));

    fs::remove_dir_all(root).expect("remove tldr priority fixture");
}

fn embedded_tldr_fixture(description: &str) -> String {
    format!(
        "<!-- mant:tldr:start -->\n# tool\n\n> {description}\n\n- Run it:\n\n`tool`\n<!-- mant:tldr:end -->\n\n# Tool\n\nFull body.\n"
    )
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
            .contains("mant.request/v0.10")
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
    assert_eq!(value["protocol"], "mant.cli/v0.10");
    assert_eq!(value["requestSchema"], "mant.request/v0.10");
    assert_eq!(value["querySchema"], "mant.query/v0.10");
    assert_eq!(value["outlineSchema"], "mant.outline/v0.10");
    assert_eq!(value["excerptSchema"], "mant.excerpt/v0.10");
    assert_eq!(value["searchSchema"], "mant.search/v0.10");

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
        "mant_find",
        "mant_outline",
        "mant_read",
        "mant_explain",
        "mant_search",
    ] {
        assert!(
            PROTOCOL_REFERENCE.contains(&format!("`{tool}`")),
            "the protocol reference must document MCP tool {tool}"
        );
    }
    for retired in [
        "mant_documents_list",
        "mant_document_outline",
        "mant_document_get",
        "mant_document_explain",
        "mant_document_search",
    ] {
        assert!(
            !PROTOCOL_REFERENCE.contains(retired),
            "the protocol reference must not retain retired MCP tool {retired}"
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
            br#"{"schema":"mant.request/v0.10","input":{"kind":"document","selector":"git"},"view":{"kind":"full"},"futureField":true}"#,
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
        .args([
            "--input",
            "-",
            "--input-format",
            "markdown",
            "--format",
            "json",
            "--compact",
        ])
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
fn explicit_roff_files_and_stdin_use_the_native_parser() {
    let path =
        std::env::temp_dir().join(format!("mant-direct-roff-process-{}.1", std::process::id()));
    let source = b".TH DIRECT-ROFF 1\n.SH NAME\ndirect-roff \\- standalone input\n";
    fs::write(&path, source).expect("write roff input");

    let file = Command::new(executable())
        .args([
            "--input",
            path.to_str().expect("UTF-8 path"),
            "--format",
            "json",
            "--compact",
        ])
        .output()
        .expect("query roff file");
    fs::remove_file(path).expect("remove roff input");
    assert!(file.status.success(), "{file:?}");
    assert!(file.stderr.is_empty());
    let file: serde_json::Value = serde_json::from_slice(&file.stdout).expect("roff file JSON");
    assert_eq!(file["document"]["source"]["format"], "man");
    assert_eq!(file["document"]["meta"]["manualSection"], "1");

    let mut child = Command::new(executable())
        .args([
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            "json",
            "--compact",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start roff stdin query");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source)
        .expect("write roff stdin");
    let stdin = child.wait_with_output().expect("wait for roff stdin query");
    assert!(stdin.status.success(), "{stdin:?}");
    assert!(stdin.stderr.is_empty());
    let stdin: serde_json::Value = serde_json::from_slice(&stdin.stdout).expect("roff stdin JSON");
    assert_eq!(stdin["label"], "DIRECT-ROFF");
    assert_eq!(stdin["document"]["source"]["format"], "man");
}

#[test]
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
            "--input",
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
        "schema": "mant.request/v0.10",
        "input": {
            "kind": "file",
            "path": path.to_str().expect("UTF-8 path"),
            "format": "markdown",
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
    assert_eq!(direct["entriesComplete"], false);
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
        "schema": "mant.request/v0.10",
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
    assert_eq!(protocol["entriesComplete"], false);
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
    assert!(outline.get("entriesComplete").is_none());
    assert_eq!(outline["nodes"][0]["children"][0]["aliases"][0], "-ca.cert");
    assert_eq!(
        outline["nodes"][0]["children"][1]["aliases"][0],
        "-ca.chain"
    );
    assert_eq!(
        outline["nodes"][0]["children"][2]["aliases"][0],
        "--foo.bar"
    );

    assert!(dotted.status.success(), "{dotted:?}");
    assert!(dotted.stderr.is_empty());
    let dotted: serde_json::Value = serde_json::from_slice(&dotted.stdout).expect("dotted JSON");
    assert_eq!(
        dotted["selections"][0]["entry"]["identity"]["names"][0],
        "-ca.cert"
    );

    assert!(positional_help.status.success(), "{positional_help:?}");
    assert!(positional_help.stderr.is_empty());
    let positional_help: serde_json::Value =
        serde_json::from_slice(&positional_help.stdout).expect("help JSON");
    assert_eq!(
        positional_help["selections"][0]["entry"]["identity"]["role"],
        "command"
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
    let matches = explain["result"]["matches"]
        .as_array()
        .expect("scope explanations");
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0]["address"]["path"], "beta");
    assert_eq!(matches[1]["address"]["path"], "alpha");
    assert_eq!(explain["result"]["missed"], 0);

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
        missing.contains("No semantic entry 'missing-entry' across 2 resolved documents."),
        "{missing}"
    );
    assert!(
        missing.contains("mant documents/alpha --outline --outline-entries all --format json"),
        "{missing}"
    );
    assert!(missing.contains("use `--search`"), "{missing}");

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

fn scope_document_paths(response: &serde_json::Value) -> Vec<&str> {
    response["scope"]["documents"]
        .as_array()
        .expect("scope documents")
        .iter()
        .map(|document| document["address"]["path"].as_str().expect("Markdown path"))
        .collect()
}

#[cfg(windows)]
fn windows_suffix_fixture() -> PathBuf {
    let fixture_root = std::env::temp_dir().join(format!(
        "mant-windows-suffix-process-{}",
        std::process::id()
    ));
    let documents = registered_documents_dir(&fixture_root);
    let data_root = registered_data_root(&fixture_root);
    let sources = data_root.join("sources");
    let alpha = sources.join("alpha");
    let beta = sources.join("beta");
    fs::create_dir_all(&documents).expect("create personal documents");
    fs::create_dir_all(&alpha).expect("create alpha source");
    fs::create_dir_all(&beta).expect("create beta source");
    fs::write(
        data_root.join("sources.toml"),
        "[alpha]\nrepo = \"https://example.invalid/alpha.git\"\nbranch = \"main\"\n\n[beta]\nrepo = \"https://example.invalid/beta.git\"\nbranch = \"main\"\n",
    )
    .expect("write source config");
    fs::write(alpha.join(".mant-source.toml"), "revision = \"alpha\"\n")
        .expect("mark alpha installed");
    fs::write(beta.join(".mant-source.toml"), "revision = \"beta\"\n")
        .expect("mark beta installed");

    let document = |name: &str, title: &str| {
        fs::write(documents.join(name), format!("# {title}\n\nBody.\n"))
            .expect("write root suffix fixture");
    };
    document("priority.md", "Exact");
    document("priority.vbs.md", "Priority VBS");
    for (suffix, title) in [
        ("vbs", "VBS"),
        ("msc", "MSC"),
        ("exe", "EXE"),
        ("com", "COM"),
    ] {
        document(&format!("ordered.{suffix}.md"), title);
    }
    document("defaulted.cmd.md", "Default CMD");
    fs::write(alpha.join("scoped.vbs.md"), "# Alpha VBS\n\nBody.\n")
        .expect("write alpha suffix fixture");
    fs::write(beta.join("foreign.vbs.md"), "# Beta VBS\n\nBody.\n")
        .expect("write beta suffix fixture");
    fixture_root
}

#[cfg(windows)]
fn query_windows_suffix(
    fixture_root: &std::path::Path,
    name: &str,
    pathext: Option<&str>,
    source: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(executable());
    configure_registered_documents(&mut command, fixture_root);
    match pathext {
        Some(value) => {
            command.env("PATHEXT", value);
        }
        None => {
            command.env_remove("PATHEXT");
        }
    }
    command.arg(name);
    if let Some(source) = source {
        command.args(["--source", source]);
    }
    command
        .args(["--format", "json", "--compact"])
        .output()
        .expect("query Windows suffix fixture")
}

#[cfg(windows)]
fn document_title(output: &std::process::Output) -> String {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .expect("suffix query JSON")["document"]["meta"]["title"]
        .as_str()
        .expect("document title")
        .to_owned()
}

#[cfg(windows)]
fn request_windows_suffix(fixture_root: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(executable());
    configure_registered_documents(&mut child, fixture_root);
    let mut child = child
        .env("PATHEXT", ".MSC;.VBS;.EXE;.COM")
        .args(["--request-json", "--format", "json", "--compact"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Windows suffix request");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"schema":"mant.request/v0.10","input":{"kind":"document","selector":"ordered"},"view":{"kind":"full"}}"#,
        )
        .expect("write Windows suffix request");
    child.wait_with_output().expect("wait for suffix request")
}

#[cfg(windows)]
#[test]
fn windows_frontends_follow_pathext_without_crossing_source_boundaries() {
    let fixture_root = windows_suffix_fixture();
    let query = |name, pathext, source| query_windows_suffix(&fixture_root, name, pathext, source);

    assert_eq!(
        document_title(&query("priority", Some(".VBS;.EXE"), None)),
        "Exact"
    );
    for (first, expected) in [
        (".VBS;.MSC;.EXE;.COM", "VBS"),
        (".MSC;.EXE;.COM;.VBS", "MSC"),
        (".EXE;.COM;.VBS;.MSC", "EXE"),
        (".COM;.VBS;.MSC;.EXE", "COM"),
    ] {
        assert_eq!(
            document_title(&query("ordered", Some(first), None)),
            expected
        );
    }
    assert_eq!(
        document_title(&query("ordered.EXE", Some(".VBS;.MSC"), None)),
        "EXE"
    );
    assert_eq!(
        document_title(&query("defaulted", None, None)),
        "Default CMD"
    );
    assert_eq!(
        document_title(&query("defaulted", Some(""), None)),
        "Default CMD"
    );
    assert_eq!(
        document_title(&query("scoped", Some(".VBS;.EXE"), Some("alpha"))),
        "Alpha VBS"
    );
    let foreign = query("foreign", Some(".VBS"), Some("alpha"));
    assert_eq!(foreign.status.code(), Some(1));

    assert_eq!(
        document_title(&request_windows_suffix(&fixture_root)),
        "MSC"
    );

    fs::remove_dir_all(fixture_root).expect("remove Windows suffix fixture");
}

#[cfg(windows)]
#[test]
fn windows_script_host_accepts_documented_double_slash_options() {
    let fixture =
        std::env::temp_dir().join(format!("mant-wsh-options-process-{}", std::process::id()));
    fs::create_dir_all(&fixture).expect("create WSH fixture");
    let script = fixture.join("check.vbs");
    let marker = fixture.join("executed.txt");
    fs::write(
        &script,
        concat!(
            "Set output = CreateObject(\"Scripting.FileSystemObject\")",
            ".CreateTextFile(WScript.Arguments(0), True)\r\n",
            "output.Write \"mant-wsh-double-slash\"\r\n",
            "output.Close\r\n",
        ),
    )
    .expect("write WSH fixture");
    let output = Command::new("cscript.exe")
        .args(["//B", "//Nologo"])
        .arg(&script)
        .arg(&marker)
        .output()
        .expect("run Windows Script Host");

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(&marker).expect("read WSH marker"),
        "mant-wsh-double-slash"
    );
    fs::remove_dir_all(fixture).expect("remove WSH fixture");
}

#[test]
fn manual_option_bypasses_registered_markdown_with_the_same_name() {
    let root = std::env::temp_dir().join(format!(
        "mant-manual-source-policy-process-{}",
        std::process::id()
    ));
    let manual_root = root.join("manuals");
    let tldr_root = root.join("tldr");
    let documents = registered_documents_dir(&root);
    fs::create_dir_all(&documents).expect("create registration root");
    fs::create_dir_all(manual_root.join("man1")).expect("create manual root");
    fs::create_dir_all(tldr_root.join("pages/common")).expect("create tldr root");
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
    fs::write(
        tldr_root.join("pages/common/source-policy.md"),
        "# source-policy\n\n> Cached quick reference.\n\n- Show the quick reference:\n\n`source-policy --quick`\n",
    )
    .expect("write tldr page");

    let run = |manual: bool| {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        command
            .arg("source-policy")
            .args(manual.then_some("--manual"))
            .args(["--format", "json", "--compact"])
            .env("MANT_MANPATH", &manual_root)
            .env("MANT_TLDR_DIR", &tldr_root);
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
    assert_eq!(manual["document"]["meta"]["manualSection"], "1");
    assert!(manual["tldr"].is_null());

    fs::remove_dir_all(root).expect("remove source-policy fixture");
}

#[test]
fn document_and_quick_reference_policies_remain_orthogonal() {
    let root = std::env::temp_dir().join(format!(
        "mant-explicit-content-process-{}",
        std::process::id()
    ));
    let manual_root = root.join("manuals");
    let tldr_root = root.join("tldr");
    fs::create_dir_all(manual_root.join("man1")).expect("create manual root");
    fs::create_dir_all(tldr_root.join("pages/common")).expect("create tldr root");
    fs::write(
        manual_root.join("man1/content-policy.1"),
        ".TH CONTENT-POLICY 1\n.SH NAME\ncontent-policy \\- native manual body\n",
    )
    .expect("write native manual");
    fs::write(
        tldr_root.join("pages/common/content-policy.md"),
        "# content-policy\n\n> Cached quick reference.\n\n- Show the quick reference:\n\n`content-policy --quick`\n",
    )
    .expect("write tldr page");

    let run = |arguments: &[&str]| {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        command
            .arg("content-policy")
            .args(arguments)
            .env("MANT_MANPATH", &manual_root)
            .env("MANT_TLDR_DIR", &tldr_root);
        command.output().expect("query explicit content")
    };

    let combined = run(&["--format", "json", "--compact"]);
    assert!(combined.status.success(), "{combined:?}");
    let combined: serde_json::Value =
        serde_json::from_slice(&combined.stdout).expect("combined JSON");
    assert_eq!(combined["document"]["source"]["format"], "man");
    assert!(!combined["tldr"].is_null());

    let manual_only = run(&["--manual", "--format", "json", "--compact"]);
    assert!(manual_only.status.success(), "{manual_only:?}");
    let manual_only: serde_json::Value =
        serde_json::from_slice(&manual_only.stdout).expect("manual-only JSON");
    assert_eq!(manual_only["document"]["source"]["format"], "man");
    assert!(manual_only["tldr"].is_null());

    let selected_section = run(&["--man-section", "1", "--format", "json", "--compact"]);
    assert!(selected_section.status.success(), "{selected_section:?}");
    let selected_section: serde_json::Value =
        serde_json::from_slice(&selected_section.stdout).expect("section-qualified JSON");
    assert_eq!(selected_section["document"]["meta"]["manualSection"], "1");
    assert!(!selected_section["tldr"].is_null());

    let removed = run(&["--section", "1"]);
    assert_eq!(removed.status.code(), Some(2));
    let diagnostic = String::from_utf8(removed.stderr).expect("removed option diagnostic");
    assert!(diagnostic.contains("--section was removed in ManT 0.7.0"));
    assert!(diagnostic.contains("--man-section <MAN_SECTION>"));
    assert!(diagnostic.contains("--node <SELECTOR>"));

    let unavailable = run(&["--man-section", "3"]);
    assert_eq!(unavailable.status.code(), Some(1));
    let diagnostic = String::from_utf8(unavailable.stderr).expect("section diagnostic");
    assert!(
        diagnostic.contains("manual section '3' is unavailable"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("available sections: 1"), "{diagnostic}");
    assert!(
        !diagnostic.contains("--man-section selects"),
        "{diagnostic}"
    );

    let tldr = run(&["--tldr"]);
    assert!(tldr.status.success(), "{tldr:?}");
    assert!(tldr.stderr.is_empty());
    let tldr = String::from_utf8(tldr.stdout).expect("plain tldr output");
    assert!(tldr.contains("Cached quick reference."));
    assert!(!tldr.contains("native manual body"));
    assert!(!tldr.contains("\u{1b}["));

    let colored = run(&["--tldr", "--color", "always"]);
    assert!(colored.status.success(), "{colored:?}");
    assert!(colored.stderr.is_empty());
    assert!(
        String::from_utf8(colored.stdout)
            .expect("colored tldr output")
            .contains("\u{1b}[")
    );

    for selectors in [
        vec!["1", "content-policy"],
        vec!["content-policy(1)"],
        vec!["manual/1/content-policy"],
    ] {
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &root);
        let output = command
            .args(selectors)
            .args(["--format", "json", "--compact"])
            .env("MANT_MANPATH", &manual_root)
            .env("MANT_TLDR_DIR", &tldr_root)
            .output()
            .expect("query man-style selector");
        assert!(output.status.success(), "{output:?}");
        let value: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("man-style selector JSON");
        assert_eq!(value["document"]["meta"]["manualSection"], "1");
    }

    fs::remove_dir_all(root).expect("remove explicit-content fixture");
}

#[cfg(unix)]
#[test]
fn registered_names_ignore_directory_symlinks() {
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
    assert_eq!(value["document"]["meta"]["manualSection"], "1");
    assert_eq!(value["document"]["source"]["format"], "man");

    fs::remove_dir_all(root).expect("remove native manual fixture");
}

#[test]
fn manual_queries_accept_flat_user_man_roots() {
    let root = std::env::temp_dir().join(format!(
        "mant-flat-native-manual-process-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create flat manual root");
    fs::write(
        root.join("flat-native.1"),
        ".TH FLAT-NATIVE 1\n.SH NAME\nflat-native \\- indexed from a flat root\n",
    )
    .expect("write flat manual source");

    let output = Command::new(executable())
        .args(["flat-native", "--manual", "--format", "json", "--compact"])
        .env("MANT_MANPATH", &root)
        .output()
        .expect("query flat native manual");

    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("query JSON");
    assert_eq!(value["document"]["meta"]["manualSection"], "1");
    assert_eq!(value["document"]["source"]["format"], "man");

    let canonical = Command::new(executable())
        .args(["manual/1/flat-native", "--format", "json", "--compact"])
        .env("MANT_MANPATH", &root)
        .output()
        .expect("query canonical flat native manual");
    assert!(canonical.status.success(), "{canonical:?}");
    let canonical: serde_json::Value =
        serde_json::from_slice(&canonical.stdout).expect("canonical manual JSON");
    assert_eq!(canonical["address"]["kind"], "manual");
    assert_eq!(canonical["address"]["manualSection"], "1");

    let catalog = Command::new(executable())
        .args([
            "--find",
            "flat-native",
            "--kind",
            "manual",
            "--format",
            "json",
            "--compact",
        ])
        .env("MANT_MANPATH", &root)
        .output()
        .expect("discover flat native manual");
    assert!(catalog.status.success(), "{catalog:?}");
    let catalog: serde_json::Value =
        serde_json::from_slice(&catalog.stdout).expect("manual catalog JSON");
    assert_eq!(catalog["documents"][0]["address"]["name"], "flat-native");
    assert_eq!(catalog["documents"][0]["address"]["manualSection"], "1");
    assert!(catalog["documents"][0].get("catalogPath").is_none());
    assert!(catalog["documents"][0].get("sourcePath").is_none());

    fs::remove_dir_all(root).expect("remove flat manual fixture");
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

    let outline = run_json(&["--input", path, "--outline", "--outline-entries", "none"]);
    assert_eq!(outline["nodes"][0]["kind"], "document-root");
    assert_eq!(outline["nodes"][0]["path"], "root");
    assert_eq!(outline["nodes"].as_array().map(Vec::len), Some(1));

    let excerpt = run_json(&["--input", path, "--node", "root"]);
    assert_eq!(excerpt["selections"][0]["kind"], "document-root");
    assert_eq!(
        excerpt["selections"][0]["blocks"][0]["children"][0]["value"],
        "Read the preface needle first."
    );

    let search = run_json(&["--input", path, "--search", "preface needle"]);
    assert_eq!(search["total"], 1);
    assert_eq!(
        search["matches"][0]["outline"]["node"]["kind"],
        "document-root"
    );
    assert_eq!(search["matches"][0]["outline"]["node"]["path"], "root");

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
