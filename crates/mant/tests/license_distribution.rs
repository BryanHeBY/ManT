//! Guards the license boundary of every published `ManT` artifact.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const APACHE_CRATES: &[&str] = &["mant-ast", "mant-core", "mant-sources", "mant-ui", "mant"];

fn portable_package_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn package_files(package: &str) -> Vec<String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "package",
            "--locked",
            "--allow-dirty",
            "--list",
            "-p",
            package,
        ])
        .current_dir(workspace_root())
        .output()
        .expect("run cargo package --list");
    assert!(
        output.status.success(),
        "package listing failed for {package}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8 package listing")
        .lines()
        .map(portable_package_path)
        .collect()
}

#[test]
fn cargo_package_paths_use_one_manifest_style_on_every_host() {
    assert_eq!(
        portable_package_path(r"LICENSES\Apache-2.0.txt"),
        "LICENSES/Apache-2.0.txt"
    );
}

#[test]
fn apache_crates_publish_the_repository_license_text() {
    let root = workspace_root();
    let apache = fs::read(root.join("LICENSE")).expect("repository Apache license");

    for package in APACHE_CRATES {
        let crate_license = root.join("crates").join(package).join("LICENSE");
        assert_eq!(
            fs::read(&crate_license).expect("crate Apache license"),
            apache,
            "{package} must carry the canonical repository license"
        );
        assert!(
            package_files(package).iter().any(|path| path == "LICENSE"),
            "{package} package must contain LICENSE"
        );
    }
}

#[test]
fn libmandoc_package_has_exact_notices_and_excludes_the_non_spdx_source() {
    let files = package_files("libmandoc-rs");
    for required in [
        "THIRD_PARTY_NOTICES.md",
        "LICENSES/Apache-2.0.txt",
        "LICENSES/BSD-2-Clause-NetBSD.txt",
        "LICENSES/BSD-2-Clause-position-unchanged.txt",
        "LICENSES/BSD-2-Clause-soelim.txt",
        "LICENSES/BSD-3-Clause-Regents.txt",
        "LICENSES/mandoc-1.14.6.txt",
        "tests/so_redirect.rs",
        "vendor/mandoc-1.14.6/LICENSE",
    ] {
        assert!(
            files.iter().any(|path| path == required),
            "libmandoc-rs package must contain {required}"
        );
    }
    for excluded in [
        "vendor/mandoc-1.14.6/soelim.c",
        "vendor/mandoc-1.14.6/soelim.1",
    ] {
        assert!(
            !files.iter().any(|path| path == excluded),
            "unused soelim pair must stay outside crates.io"
        );
    }
}

#[test]
fn vendored_license_mapping_tracks_authoritative_headers() {
    let root = workspace_root().join("crates/libmandoc-rs");
    let read = |relative: &str| {
        fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"))
    };

    let regents = read("vendor/mandoc-1.14.6/compat_err.c");
    assert!(regents.contains("Neither the name of the University"));
    let netbsd = read("vendor/mandoc-1.14.6/compat_stringlist.c");
    assert!(netbsd.contains("The NetBSD Foundation, Inc."));
    assert!(!netbsd.contains("Neither the name"));
    let soelim = read("vendor/mandoc-1.14.6/soelim.c");
    assert!(soelim.contains("in this position and unchanged."));
    let soelim_manual = read("vendor/mandoc-1.14.6/soelim.1");
    assert!(!soelim_manual.contains("position and unchanged"));

    let notices = read("THIRD_PARTY_NOTICES.md");
    assert!(notices.contains("| BSD-3-Clause | `compat_err.c`"));
    assert!(notices.contains("`compat_stringlist.c`, `compat_stringlist.h`"));
    assert!(notices.contains("source notice must remain in position and unchanged"));
}

#[test]
fn native_archives_copy_the_complete_parser_notice_set() {
    let unix = include_str!("../../../scripts/package-release.sh");
    assert!(unix.contains("THIRD_PARTY_LICENSES.html"));
    assert!(unix.contains("LICENSES/RUST_DEPENDENCIES.html"));
    assert!(unix.contains("crates/libmandoc-rs/LICENSES/*"));
    assert!(unix.contains("crates/libmandoc-rs/THIRD_PARTY_NOTICES.md"));
    assert!(unix.contains("LICENSES/CC-BY-4.0.txt"));
    assert!(unix.contains("LICENSES/PRODUCT_THIRD_PARTY_NOTICES.md"));

    let windows = include_str!("../../../scripts/package-release.ps1");
    assert!(windows.contains("THIRD_PARTY_LICENSES.html"));
    assert!(windows.contains("LICENSES/RUST_DEPENDENCIES.html"));
    assert!(windows.contains("crates/libmandoc-rs/LICENSES/*"));
    assert!(windows.contains("crates/libmandoc-rs/THIRD_PARTY_NOTICES.md"));
    assert!(windows.contains("LICENSES/CC-BY-4.0.txt"));
    assert!(windows.contains("LICENSES/PRODUCT_THIRD_PARTY_NOTICES.md"));
}

#[test]
fn rust_dependency_notice_is_generated_from_the_locked_product_graph() {
    let about = include_str!("../../../about.toml");
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(about.contains(target), "cargo-about must cover {target}");
    }

    let generator = include_str!("../../../scripts/generate-rust-licenses.sh");
    assert!(generator.contains(r#"about_version == "cargo-about 0.9.1""#));
    assert!(generator.contains("--frozen"));
    assert!(generator.contains("--manifest-path crates/mant/Cargo.toml"));
    assert!(generator.contains("--all-features"));
    assert!(generator.contains("--fail"));

    let licenses = include_str!("../../../THIRD_PARTY_LICENSES.html");
    assert!(licenses.contains("ManT Rust dependency licenses"));
    assert!(licenses.contains("cargo-about"));
    assert!(licenses.contains("mant 0.6.4"));
    assert!(licenses.contains("libmandoc-rs 0.6.4"));
    assert!(licenses.contains("ratatui 0.30.2"));
    assert!(licenses.contains("rustls 0.23.43"));
}

#[test]
fn repository_notice_names_every_non_product_distribution_boundary() {
    let notice = include_str!("../../../THIRD_PARTY_NOTICES.md");
    assert!(notice.contains("tests/fixtures/roff/real/"));
    assert!(notice.contains("docs/assets/fonts/"));
    assert!(notice.contains("crates/libmandoc-rs/THIRD_PARTY_NOTICES.md"));
    assert!(notice.contains("THIRD_PARTY_LICENSES.html"));
    assert!(notice.contains("tldr-pages/tldr"));
    assert!(notice.contains("LICENSES/CC-BY-4.0.txt"));
    assert!(workspace_root().join("LICENSES/CC-BY-4.0.txt").is_file());
}
