//! Installation-layout checks without a checkout or host manual dependency.
mod support;

use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(home: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mant"));
    support::configure_registered_documents(&mut command, home);
    command
        .current_dir(home)
        .env("MANT_MANPATH", home.join("manuals"))
        .env("MANSECT", "3:1")
        .args(args)
        .args(["--format", "json", "--color", "never"])
        .output()
        .unwrap()
}

fn success(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn installed_manifest_supports_catalog_outline_and_explicit_reads() {
    let home = std::env::temp_dir().join(format!("mant-installed-links-{}", std::process::id()));
    let documents = support::registered_documents_dir(&home);
    fs::create_dir_all(&documents).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/manuals");
    let files: Vec<_> = include_str!("../../../docs/manuals/manifest.txt")
        .lines()
        .filter(|line| !line.is_empty())
        .collect();
    for file in &files {
        fs::copy(source.join(file), documents.join(file)).unwrap();
    }
    let catalog = success(&run(&home, &["--list", "--kind", "markdown"]));
    let catalog_text = catalog.to_string();
    for file in files {
        let name = file.strip_suffix(".md").unwrap();
        assert!(catalog_text.contains(name), "{name}");
        let outline = success(&run(
            &home,
            &[name, "--outline", "--outline-references=all"],
        ));
        assert_eq!(
            outline["references"]["coverage"]["status"]["kind"],
            "complete"
        );
        for record in outline["references"]["records"].as_array().unwrap() {
            assert_ne!(
                record["resolution"]["kind"], "restricted",
                "{name}: {record}"
            );
            if record["target"]["kind"] == "document" {
                let target = record["resolution"]["address"]["path"].as_str().unwrap();
                success(&run(&home, &[&format!("documents/{target}")]));
            }
        }
        success(&run(&home, &[name, "--node=1"]));
    }
    // A directory containing only the shipped manuals cannot accidentally load
    // checkout-only architecture pages through relative namespace traversal.
    assert!(
        !run(&home, &["documents/../architecture/semantic-explanations"])
            .status
            .success()
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn native_link_discovery_does_not_require_targets_but_opening_uses_exact_sections() {
    let home = std::env::temp_dir().join(format!("mant-native-links-{}", std::process::id()));
    let documents = support::registered_documents_dir(&home);
    fs::create_dir_all(&documents).unwrap();
    fs::create_dir_all(home.join("manuals/man1")).unwrap();
    fs::create_dir_all(home.join("manuals/man3")).unwrap();
    fs::write(documents.join("links.md"), "# Manual links\n\n[exact](man:linkprobe(3)) [unqualified](man:linkprobe) [missing](man:linkabsent(7))\n").unwrap();
    for section in ["1", "3"] {
        fs::write(
            home.join(format!("manuals/man{section}/linkprobe.{section}")),
            format!(".TH LINKPROBE {section}\n.SH NAME\nlinkprobe \\- test manual {section}\n"),
        )
        .unwrap();
    }
    let outline = success(&run(
        &home,
        &["links", "--outline", "--outline-references=all"],
    ));
    let records = outline["references"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["target"]["manualSection"], "3");
    assert_eq!(records[1]["resolution"]["kind"], "not-queried");
    assert_eq!(records[2]["target"]["name"], "linkabsent");
    if cfg!(feature = "roff") {
        let exact = success(&run(&home, &["manual/3/linkprobe"]));
        assert!(exact.to_string().contains("test manual 3"));
        let unqualified = success(&run(&home, &["linkprobe", "--manual"]));
        assert!(
            unqualified.to_string().contains("test manual 3"),
            "unqualified opening follows configured manual section precedence, not a guessed section 1"
        );
    } else {
        let unavailable = run(&home, &["manual/3/linkprobe"]);
        assert_eq!(unavailable.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&unavailable.stderr).contains("requires the 'roff' feature")
        );
        assert!(unavailable.stdout.is_empty());
    }
    assert!(!run(&home, &["manual/7/linkabsent"]).status.success());
    // No implicit section is introduced into the unqualified link's target.
    assert!(records[1]["target"].get("manualSection").is_none());
    fs::remove_dir_all(home).unwrap();
}
