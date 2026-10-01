use super::process_support::executable;
use super::support::{configure_registered_documents, registered_documents_dir};
use std::fs;
use std::process::Command;

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
    assert_eq!(
        value["document"]["heading"]["content"][0]["value"],
        "Registered"
    );
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

    #[cfg(feature = "roff")]
    {
        let manual = run(true);
        assert!(manual.status.success(), "{manual:?}");
        assert!(manual.stderr.is_empty());
        let manual: serde_json::Value =
            serde_json::from_slice(&manual.stdout).expect("manual JSON");
        assert_eq!(manual["document"]["source"]["format"], "man");
        assert_eq!(manual["document"]["meta"]["manualSection"], "1");
        assert!(manual["tldr"].is_null());
    }

    #[cfg(not(feature = "roff"))]
    {
        let unavailable = run(true);
        assert_eq!(unavailable.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&unavailable.stderr).contains("unexpected argument '--manual'")
        );
        assert!(unavailable.stdout.is_empty());
    }
    fs::remove_dir_all(root).expect("remove source-policy fixture");
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

    if cfg!(feature = "roff") {
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("query JSON");
        assert_eq!(value["label"], "native-only");
        assert_eq!(value["document"]["meta"]["manualSection"], "1");
        assert_eq!(value["document"]["source"]["format"], "man");
    } else {
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("requires the 'roff' feature"));
        assert!(output.stdout.is_empty());
    }

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

    #[cfg(feature = "roff")]
    {
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
    }

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
