//! Guards the crates.io installer metadata against the native archive layout.

use std::{path::PathBuf, process::Command};

#[cfg(target_os = "linux")]
use std::{fs, process, time::SystemTime};

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
    assert!(windows.contains(r"crates/libmandoc-rs/LICENSES/*"));
    assert!(windows.contains(r"LICENSES/THIRD_PARTY_NOTICES.md"));
    assert!(windows.contains(r"LICENSES/RUST_DEPENDENCIES.html"));
}

#[test]
fn release_workflow_publishes_and_attests_target_specific_sboms() {
    let workflow = include_str!("../../../.github/workflows/release.yml");
    assert!(workflow.contains("tool: cargo-cyclonedx@0.5.9"));
    assert!(workflow.contains("--spec-version 1.5"));
    assert!(workflow.contains("--describe binaries"));
    assert!(workflow.contains("--target-in-filename"));
    assert!(workflow.contains("SOURCE_DATE_EPOCH=0"));
    assert!(workflow.contains("dist/mant-*.cdx.json"));
    assert!(workflow.contains(r#"echo "MANT_SBOM_PATH=dist/$sbom" >> "$GITHUB_ENV""#));
    assert!(workflow.contains(r#""MANT_SBOM_PATH=$Sbom" | Out-File"#));
    assert_eq!(
        workflow
            .matches("sbom-path: ${{ env.MANT_SBOM_PATH }}")
            .count(),
        2
    );
    assert!(!workflow.contains("sbom-path: dist/mant-*.cdx.json"));
    assert!(workflow.contains("uses: actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6"));
}

#[test]
fn one_line_installers_follow_the_published_release_contract() {
    let readme = include_str!("../../../README.md");
    let unix = include_str!("../../../scripts/install.sh");
    let windows = include_str!("../../../scripts/install.ps1");

    assert!(
        readme.contains(
            "https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.sh | sh"
        )
    );
    assert!(readme.contains(
        "https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.ps1 | iex"
    ));

    assert!(unix.contains("$GITHUB_URL/releases/latest"));
    assert!(unix.contains(r#"archive="mant-$version-$target.tar.gz""#));
    assert!(unix.contains(r#"download "$release_url/SHA256SUMS""#));
    assert!(unix.contains(r#"install -m 0644 "$manual" "$manual_path""#));
    assert!(unix.contains("RECEIPT_SCHEMA=mant.install/v1"));
    assert!(unix.contains(r#"[ "$uninstall" = true ]"#));
    assert!(unix.contains("ManT %s is already up to date."));
    assert!(unix.contains("public Linux archives require glibc"));

    assert!(windows.contains("/releases/latest"));
    assert!(windows.contains(r#"$Archive = "mant-$Version-$Target.zip""#));
    assert!(
        windows.contains(r#"Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseUrl/SHA256SUMS""#)
    );
    assert!(windows.contains(r"Copy-Item $Manual $ManualPath -Force"));
    assert!(windows.contains(r#"$ReceiptSchema = "mant.install/v1""#));
    assert!(windows.contains("if ($Uninstall)"));
    assert!(windows.contains("ManT $Version is already up to date."));

    assert!(readme.contains("docs/installation.md"));
    let installation = include_str!("../../../docs/installation.md");
    assert!(installation.contains("sh -s -- --uninstall"));
    assert!(installation.contains("-Uninstall"));
}

#[cfg(target_os = "linux")]
#[test]
fn unix_installer_uninstalls_only_files_owned_by_its_receipt() {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mant-installer-{}-{nonce}", process::id()));
    let state = root.join("state/mant");
    let bin = root.join("bin");
    let documents = root.join("documents");
    fs::create_dir_all(&state).expect("installer state directory");
    fs::create_dir_all(&bin).expect("installer binary directory");
    fs::create_dir_all(&documents).expect("installer document directory");

    let binary = bin.join("mant");
    let manual = documents.join("mant.md");
    let user_document = documents.join("user.md");
    fs::write(&binary, "installed binary").expect("installed binary fixture");
    fs::write(&manual, "installed manual").expect("installed manual fixture");
    fs::write(&user_document, "user document").expect("user document fixture");
    fs::write(
        state.join("install-receipt"),
        format!(
            "schema\tmant.install/v1\nversion\t0.5.0\ninstall_dir\t{}\ndata_dir\t{}\nbinary\t{}\nmanual\t{}\n",
            bin.display(),
            documents.display(),
            binary.display(),
            manual.display()
        ),
    )
    .expect("installer receipt fixture");

    let installer = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh");
    let output = Command::new("sh")
        .arg(installer)
        .arg("--uninstall")
        .env("XDG_STATE_HOME", root.join("state"))
        .output()
        .expect("run Unix uninstaller");
    assert!(
        output.status.success(),
        "Unix uninstaller failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!binary.exists());
    assert!(!manual.exists());
    assert!(!state.join("install-receipt").exists());
    assert!(user_document.exists());
    assert!(documents.exists());

    fs::remove_dir_all(root).expect("remove installer fixture");
}

#[test]
fn crates_are_packaged_only_after_their_exact_predecessors_reach_the_registry() {
    let publish = include_str!("../../../scripts/publish-crates.sh").replace("\r\n", "\n");
    assert!(
        publish.contains("PACKAGES=(mant-ast libmandoc-rs mant-sources mant-core mant-ui mant)")
    );
    let publish_loop = publish
        .split_once("# Exact internal dependencies make publication inherently sequential:")
        .map(|(_, suffix)| suffix)
        .expect("sequential publication explanation");

    assert_eq!(
        publish
            .matches("cargo package --locked --no-verify")
            .count(),
        1
    );
    assert!(publish_loop.contains(
        "else\n    cargo package --locked --no-verify -p \"$package\"\n    cargo publish --locked -p \"$package\"\n  fi\n  wait_for_registry"
    ));
}
