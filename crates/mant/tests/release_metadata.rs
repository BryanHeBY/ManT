//! Guards the crates.io installer metadata against the native archive layout.

use std::{path::PathBuf, process::Command};

use serde_json::Value;

#[test]
fn binstall_targets_match_the_published_archive_contract() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Value = serde_json::from_slice(&output.stdout).expect("parse cargo metadata");
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| packages.iter().find(|package| package["name"] == "mant"))
        .expect("mant package metadata");
    let overrides = package["metadata"]["binstall"]["overrides"]
        .as_object()
        .expect("cargo-binstall target overrides");

    let expected = [
        (
            "x86_64-unknown-linux-gnu",
            "{ repo }/releases/download/v{ version }/mant-{ version }-linux-x64.tar.gz",
            "mant-{ version }-linux-x64/{ bin }",
            "tgz",
        ),
        (
            "aarch64-unknown-linux-gnu",
            "{ repo }/releases/download/v{ version }/mant-{ version }-linux-arm64.tar.gz",
            "mant-{ version }-linux-arm64/{ bin }",
            "tgz",
        ),
        (
            "x86_64-pc-windows-msvc",
            "{ repo }/releases/download/v{ version }/mant-{ version }-windows-x64.zip",
            "mant-{ version }-windows-x64/{ bin }{ binary-ext }",
            "zip",
        ),
    ];

    assert_eq!(overrides.len(), expected.len());
    for (target, url, binary, format) in expected {
        assert_eq!(overrides[target]["pkg-url"], url, "{target} URL");
        assert_eq!(overrides[target]["bin-dir"], binary, "{target} binary");
        assert_eq!(overrides[target]["pkg-fmt"], format, "{target} format");
    }
}

#[test]
fn release_scripts_keep_the_binary_under_the_binstall_archive_root() {
    let unix = include_str!("../../../scripts/package-release.sh");
    assert!(unix.contains(r#"archive_root="mant-$version-$target""#));
    assert!(unix.contains(r#"install -m 0755 "$binary" "$package/mant""#));

    let windows = include_str!("../../../scripts/package-release.ps1");
    assert!(windows.contains(r#"$ArchiveRoot = "mant-$Version-$Target""#));
    assert!(windows.contains(r#"Copy-Item $Binary (Join-Path $Package "mant.exe")"#));
}
