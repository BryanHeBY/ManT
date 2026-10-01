use super::process_support::{executable, registered_data_root};
use super::support::{configure_registered_documents, registered_documents_dir};
use std::fs;
use std::process::Command;

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
    assert!(diagnostic.contains(if cfg!(feature = "roff") {
        "could not load manual 'quick-only'"
    } else {
        "requires the 'roff' feature"
    }));
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

    #[cfg(feature = "roff")]
    {
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
    }

    assert_quick_reference_policy(&run);

    #[cfg(feature = "roff")]
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

    #[cfg(not(feature = "roff"))]
    {
        let unavailable = run(&["--format", "json", "--compact"]);
        assert_eq!(unavailable.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&unavailable.stderr).contains("requires the 'roff' feature")
        );
        assert!(unavailable.stdout.is_empty());
    }
    fs::remove_dir_all(root).expect("remove explicit-content fixture");
}

fn assert_quick_reference_policy(run: &impl Fn(&[&str]) -> std::process::Output) {
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
}
