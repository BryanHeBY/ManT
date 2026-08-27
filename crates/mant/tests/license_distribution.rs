//! Guards the license boundary of every published `ManT` artifact.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const APACHE_CRATES: &[&str] = &[
    "mant-ir",
    "mant-protocol",
    "mantdoc",
    "mant-engine",
    "mant-sources",
    "mant-ui",
    "mant",
];

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
fn mantdoc_package_excludes_c_oracle_sources() {
    let files = package_files("mantdoc");
    assert!(files.iter().any(|path| path == "src/lib.rs"));
    assert!(files.iter().any(|path| path == "benches/parse.rs"));
    assert!(files.iter().all(|path| {
        !path.contains("vendor/")
            && !path.contains("shim/")
            && !Path::new(path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("c"))
    }));
}

#[test]
fn native_archives_copy_rust_and_product_notices_without_c_parser_notices() {
    let unix = include_str!("../../../scripts/package-release.sh");
    assert!(unix.contains("THIRD_PARTY_LICENSES.html"));
    assert!(unix.contains("LICENSES/RUST_DEPENDENCIES.html"));
    assert!(!unix.contains("crates/libmandoc-rs/"));
    assert!(unix.contains("LICENSES/CC-BY-4.0.txt"));
    assert!(unix.contains("LICENSES/PRODUCT_THIRD_PARTY_NOTICES.md"));

    let windows = include_str!("../../../scripts/package-release.ps1");
    assert!(windows.contains("THIRD_PARTY_LICENSES.html"));
    assert!(windows.contains("LICENSES/RUST_DEPENDENCIES.html"));
    assert!(!windows.contains("crates/libmandoc-rs/"));
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
    assert!(generator.contains(r#"about_version == "cargo-about 0.9.2""#));
    assert!(generator.contains("--frozen"));
    assert!(generator.contains("--manifest-path crates/mant/Cargo.toml"));
    assert!(generator.contains("--all-features"));
    assert!(generator.contains("--fail"));

    let licenses = include_str!("../../../THIRD_PARTY_LICENSES.html");
    let version = env!("CARGO_PKG_VERSION");
    assert!(licenses.contains("ManT Rust dependency licenses"));
    assert!(licenses.contains("cargo-about"));
    assert!(licenses.contains(&format!("mant {version}")));
    assert!(licenses.contains("mantdoc 0.1.0-alpha.1"));
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
