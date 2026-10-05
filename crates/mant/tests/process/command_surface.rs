use super::process_support::{executable, registered_data_root};
use super::support::{configure_registered_documents, registered_documents_dir};
use std::fs;
use std::process::Command;

#[test]
fn help_groups_the_public_query_surface() {
    let output = Command::new(executable())
        .arg("--help")
        .output()
        .expect("run mant");

    assert!(output.status.success());
    assert_eq!(output.stderr.len(), 0);
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("mant <SELECTOR> [OPTIONS]"));
    assert!(help.contains(
        "mant --input ./tool.md --outline --outline-entries all --format json --compact"
    ));
    assert!(help.contains("mant --input <PATH|-> [--input-format <FORMAT>] [OPTIONS]"));
    assert!(help.contains("Document selection:"));
    assert!(help.contains("Search:"));
    assert!(help.contains("Integration:"));
    assert!(help.contains("Diagnostics:"));
    assert!(help.contains("-h, --help"));
    assert!(help.contains("--display"));
    assert!(!help.contains("--ui"));
    assert!(!help.contains("--no-pager"));
    assert!(help.contains("-V, --version"));
    assert!(help.contains("--format <FORMAT>"));
    assert!(help.contains("--preserve-anchors"));
    assert!(help.contains("--color <COLOR>"));
    assert_eq!(help.contains("--update-tldr"), cfg!(feature = "update"));
    assert_eq!(help.contains("--update-docs"), cfg!(feature = "update"));
    assert_eq!(help.contains("--prune-docs"), cfg!(feature = "update"));
    assert!(help.contains("--doctor"));
    assert_eq!(help.contains("--dry-run"), cfg!(feature = "update"));
    assert!(help.contains("--source <SOURCE>"));
    assert!(help.contains("--protocol-version"));
    assert!(help.contains("--schema <CONTRACT>"));
    assert_eq!(help.contains("--mcp"), cfg!(feature = "mcp"));
    assert!(help.contains("--explain <ENTRY>"));
    assert!(help.contains("--search <PATTERN>"));
    assert_eq!(help.contains("--manual"), cfg!(feature = "roff"));
    assert!(help.contains("--tldr"));
    assert!(!help.contains("--force-libmandoc"));
    assert!(!help.contains("--force-groff"));
    assert!(!help.contains("--json"));
    assert!(!help.contains("update tldr"));
}

#[test]
fn help_and_empty_invocations_offer_the_manual_without_reading_it() {
    for flags in [vec!["--help"], vec!["-h"], vec![]] {
        let missing_home =
            std::env::temp_dir().join(format!("mant-help-no-manual-{}", std::process::id()));
        assert!(!missing_home.exists());
        let mut command = Command::new(executable());
        configure_registered_documents(&mut command, &missing_home);
        command.env("MANT_MANPATH", missing_home.join("man"));
        let output = command.args(&flags).output().expect("run help entry point");
        let text = if flags.is_empty() {
            assert_eq!(output.status.code(), Some(2));
            assert_eq!(output.stdout.len(), 0);
            String::from_utf8(output.stderr).expect("usage diagnostic")
        } else {
            assert!(output.status.success());
            assert_eq!(output.stderr.len(), 0);
            let help = String::from_utf8(output.stdout).expect("help text");
            assert!(help.starts_with("Read or query structured local manuals and Markdown\n"));
            assert!(help.find("Document selection:").unwrap() < help.find("TLDR:").unwrap());
            help
        };
        assert!(text.contains("mant mant            Read the full manual"));
        assert!(text.contains("mant mant --outline"));
        assert!(text.contains("When the self manual is installed:"));
        assert!(text.find("Usage:").unwrap() < text.find("ManT manual:").unwrap());
        assert!(text.find("TLDR:").unwrap() < text.find("ManT manual:").unwrap());
        assert!(text.trim_end().ends_with("Explore its outline"));
        assert!(!text.contains("Examples:"));
        assert!(text.contains(
            "mant --find '^git' --regex --kind manual --limit 20 --format json --compact"
        ));
        if flags.is_empty() {
            assert!(!text.contains("Document selection:"));
        }
        assert!(!text.contains('\x1b'));
        assert!(!text.contains("## Description"));
        assert!(
            !missing_home.exists(),
            "help must not initialize document storage"
        );
    }
}

#[test]
fn version_uses_the_standard_successful_clap_boundary() {
    let output = Command::new(executable())
        .arg("--version")
        .output()
        .expect("run mant --version");

    assert!(output.status.success());
    assert_eq!(output.stderr.len(), 0);
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
    assert_eq!(text.stderr.len(), 0);
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
    assert_eq!(json.stderr.len(), 0);
    assert!(!json.stdout.contains(&0x1b));
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).expect("doctor JSON");
    assert_eq!(report["schema"], "mant.doctor/v1");
    assert_eq!(
        report["environment"]["dataRoot"],
        data_root.to_string_lossy().as_ref()
    );
    assert_eq!(
        report["environment"]["configPath"],
        home.join("config")
            .join("sources.toml")
            .to_string_lossy()
            .as_ref()
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
    assert_eq!(short.stderr.len(), 0);
    assert_eq!(short.stdout, long.stdout);
    assert!(long.status.success());
    assert_eq!(long.stderr.len(), 0);
}

#[test]
fn explicit_tui_requires_a_real_terminal_before_loading_a_document() {
    let output = Command::new(executable())
        .args(["definitely-not-a-real-manual", "--display", "tui"])
        .output()
        .expect("run redirected mant UI");

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout.len(), 0);
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.contains(if cfg!(feature = "tui") {
        "interactive display requires"
    } else {
        "invalid value"
    }));
    assert!(!diagnostic.contains("No manual entry"));
}

#[test]
fn unknown_options_do_not_expose_rust_source_excerpts() {
    let output = Command::new(executable())
        .arg("--not-an-option")
        .output()
        .expect("run mant");

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout.len(), 0);
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.starts_with("error: unexpected argument '--not-an-option'"));
    assert!(diagnostic.contains("Usage: mant"));
    assert!(diagnostic.contains("For more information, try '--help'."));
}
