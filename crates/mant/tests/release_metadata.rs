//! Guards the crates.io installer metadata against the native archive layout.

use std::{path::PathBuf, process::Command};

#[cfg(target_os = "linux")]
use std::{fs, process, time::SystemTime};

use serde_json::Value;

#[test]
fn release_profile_preserves_unwind_cleanup() {
    let manifest = include_str!("../../../Cargo.toml");
    let release = manifest
        .split_once("[profile.release]")
        .expect("release profile")
        .1
        .split("\n[")
        .next()
        .expect("release profile body");

    assert!(
        !release
            .lines()
            .any(|line| line.trim() == "panic = \"abort\""),
        "the TUI terminal guard and catch_unwind require unwinding"
    );
}

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
    assert!(unix.contains("id=$(cargo pkgid -p mant)"));
    assert!(!unix.contains("workspace_version"));
    assert!(unix.contains(r#"archive_root="mant-$version-$target""#));
    assert!(unix.contains(r#"install -m 0755 "$binary" "$package/mant""#));

    let windows = include_str!("../../../scripts/package-release.ps1");
    assert!(windows.contains("cargo metadata --no-deps --format-version 1"));
    assert!(windows.contains("Where-Object { $_.name -eq \"mant\" }"));
    assert!(windows.contains(r#"$ArchiveRoot = "mant-$Version-$Target""#));
    assert!(windows.contains(r#"Copy-Item $Binary (Join-Path $Package "mant.exe")"#));
    assert!(windows.contains(r"crates/libmandoc-rs/LICENSES/*"));
    assert!(windows.contains(r"LICENSES/THIRD_PARTY_NOTICES.md"));
    assert!(windows.contains(r"LICENSES/RUST_DEPENDENCIES.html"));
}

#[test]
fn release_workflow_publishes_and_attests_target_specific_sboms() {
    let workflow = include_str!("../../../.github/workflows/release.yml");
    assert!(workflow.contains("name: Verify release commit"));
    assert!(workflow.contains("LIBMANDOC_RS_DENY_WARNINGS: \"1\""));
    assert!(workflow.contains("scripts/find-successful-ci.sh \"$source_sha\""));
    assert!(workflow.contains("needs: verify"));
    assert!(workflow.contains("bash .release-automation/scripts/build-and-smoke.sh release"));
    assert!(
        workflow
            .contains("./.release-automation/scripts/build-and-smoke.ps1 -BuildProfile release")
    );
    assert_eq!(
        workflow
            .matches("MANT_WORKSPACE: ${{ github.workspace }}")
            .count(),
        2
    );
    assert!(!workflow.contains("run: bash scripts/check.sh"));
    assert!(!workflow.contains("run: ./scripts/check-windows.ps1"));
    assert!(workflow.contains("tool: cargo-cyclonedx@0.5.9"));
    assert!(workflow.contains("--spec-version 1.5"));
    assert!(workflow.contains("--describe binaries"));
    assert!(workflow.contains("--target-in-filename"));
    assert!(workflow.contains("SOURCE_DATE_EPOCH=0"));
    assert_eq!(
        workflow
            .matches("node .release-automation/scripts/finalize-cyclonedx.mjs")
            .count(),
        2
    );
    assert!(workflow.contains("ref: ${{ github.sha }}"));
    assert!(workflow.contains("path: .release-automation"));
    for helper in [
        "scripts/build-and-smoke.sh",
        "scripts/build-and-smoke.ps1",
        "scripts/finalize-cyclonedx.mjs",
    ] {
        assert!(workflow.lines().any(|line| line.trim() == helper));
    }
    assert!(workflow.contains("dist/mant-*.cdx.json"));
    assert!(workflow.contains("bash scripts/package-manuals.sh"));
    assert!(workflow.contains("subject-path: dist/mant-*-manuals.tar.gz*"));
    assert!(workflow.contains("mant-${version}-manuals.sigstore.json"));
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

    let finalizer = include_str!("../../../scripts/finalize-cyclonedx.mjs");
    assert!(finalizer.contains(r#"bom.bomFormat !== "CycloneDX""#));
    assert!(finalizer.contains("bom.serialNumber = `urn:uuid:"));
    assert!(finalizer.contains("delete bom.serialNumber"));
    assert!(finalizer.contains(r#"createHash("sha256").update(normalized)"#));
}

#[test]
fn manual_release_retries_require_an_explicit_crates_publish_choice() {
    let workflow = include_str!("../../../.github/workflows/release.yml");
    assert!(workflow.contains("publish_crates:"));
    assert!(workflow.contains("default: false"));
    assert!(
        workflow.contains("(github.event_name == 'workflow_dispatch' && inputs.publish_crates)")
    );
    assert!(workflow.contains("MANT_PUBLISH_PACKAGE: ${{ needs.verify.outputs.package }}"));
    for tag in [
        "mant-ir-v*.*.*",
        "mant-protocol-v*.*.*",
        "libmandoc-rs-v*.*.*",
        "mant-sources-v*.*.*",
        "mant-engine-v*.*.*",
        "mant-ui-v*.*.*",
    ] {
        assert!(workflow.contains(tag), "missing crate tag trigger {tag}");
    }
    assert!(!workflow.contains("      - \"mant-v*.*.*\""));
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
    assert!(unix.contains(r#"manual_archive="mant-$version-manuals.tar.gz""#));
    assert!(unix.contains(r#"verify_github_attestation "$asset""#));
    assert!(unix.contains(r#"gh attestation verify "$artifact" --repo "$REPOSITORY""#));
    assert!(!unix.contains("raw.githubusercontent.com/$REPOSITORY/$tag/docs/manuals"));
    assert!(unix.contains("predates verified manual bundles; installing the binary only"));
    assert!(unix.contains(
        "BUNDLED_MANUALS=\"mant.md mant-ir.md mant-markdown.md mant-protocol.md mant-roff.md\""
    ));
    assert!(
        unix.contains(r#"install -m 0644 "$manual_dir/$manual_name" "$data_dir/$manual_name""#)
    );
    assert!(unix.contains("RECEIPT_SCHEMA=mant.install/v1"));
    assert!(unix.contains(r#"[ "$uninstall" = true ]"#));
    assert!(unix.contains("ManT %s is already up to date."));
    assert!(unix.contains("public Linux archives require glibc"));

    assert!(windows.contains("/releases/latest"));
    assert!(windows.contains(r#"$Archive = "mant-$Version-$Target.zip""#));
    assert!(
        windows.contains(r#"Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseUrl/SHA256SUMS""#)
    );
    assert!(windows.contains(r#"$BundledManuals = @("mant.md", "mant-ir.md", "mant-markdown.md", "mant-protocol.md", "mant-roff.md")"#));
    assert!(windows.contains(r"Copy-Item $Manual $ManualPath -Force"));
    assert!(windows.contains(r#"$ReceiptSchema = "mant.install/v1""#));
    assert!(windows.contains("if ($Uninstall)"));
    assert!(windows.contains("ManT $Version is already up to date."));
    assert!(windows.contains("Assert-GitHubAttestation $ArchivePath"));
    assert!(windows.contains("gh attestation verify $Artifact --repo $Repository"));

    assert!(readme.contains("docs/installation.md"));
    let installation = include_str!("../../../docs/installation.md");
    assert!(installation.contains("sh -s -- --uninstall"));
    assert!(installation.contains("-Uninstall"));

    let manifest = include_str!("../../../docs/manuals/manifest.txt");
    assert_eq!(
        manifest.lines().collect::<Vec<_>>(),
        [
            "mant.md",
            "mant-ir.md",
            "mant-markdown.md",
            "mant-protocol.md",
            "mant-roff.md",
        ]
    );
    assert!(
        include_str!("../../../scripts/package-release.sh")
            .contains("done < docs/manuals/manifest.txt")
    );
    assert!(
        include_str!("../../../scripts/package-release.ps1").contains("docs/manuals/manifest.txt")
    );
    let manual_package = include_str!("../../../scripts/package-manuals.sh");
    assert!(manual_package.contains("id=$(cargo pkgid -p mant)"));
    assert!(!manual_package.contains("workspace_version"));
    assert!(manual_package.contains(r#"archive_root="mant-$version-manuals""#));
    assert!(manual_package.contains("done < docs/manuals/manifest.txt"));
    assert!(manual_package.contains(r#"install -m 0644 LICENSE "$package/LICENSE""#));
    assert!(
        manual_package.contains(
            r#"install -m 0644 THIRD_PARTY_NOTICES.md "$package/THIRD_PARTY_NOTICES.md""#
        )
    );
    assert!(manual_package.contains("LICENSES/CC-BY-4.0.txt"));
    assert!(manual_package.contains("--mtime=@0"));
    assert!(manual_package.contains("gzip -n"));
}

#[cfg(target_os = "linux")]
#[test]
fn unix_installer_uninstalls_only_files_owned_by_its_receipt() {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mant installer-{}-{nonce}", process::id()));
    let state = root.join("state/mant");
    let bin = root.join("bin");
    let documents = root.join("documents");
    fs::create_dir_all(&state).expect("installer state directory");
    fs::create_dir_all(&bin).expect("installer binary directory");
    fs::create_dir_all(&documents).expect("installer document directory");

    let binary = bin.join("mant");
    let manual = documents.join("mant.md");
    let ir_manual = documents.join("mant-ir.md");
    let user_document = documents.join("user.md");
    fs::write(&binary, "installed binary").expect("installed binary fixture");
    fs::write(&manual, "installed manual").expect("installed manual fixture");
    fs::write(&ir_manual, "installed IR manual").expect("installed IR manual fixture");
    fs::write(&user_document, "user document").expect("user document fixture");
    fs::write(
        state.join("install-receipt"),
        format!(
            "schema\tmant.install/v1\nversion\t0.7.0\ninstall_dir\t{}\ndata_dir\t{}\nbinary\t{}\nmanual\t{}\nmanual\t{}\n",
            bin.display(),
            documents.display(),
            binary.display(),
            manual.display(),
            ir_manual.display()
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
    assert!(!ir_manual.exists());
    assert!(!state.join("install-receipt").exists());
    assert!(user_document.exists());
    assert!(documents.exists());

    fs::remove_dir_all(root).expect("remove installer fixture");
}

#[test]
fn workspace_crates_own_their_versions_and_use_explicit_caret_dependencies() {
    let root = include_str!("../../../Cargo.toml");
    let workspace_package = root
        .split_once("[workspace.package]")
        .and_then(|(_, suffix)| suffix.split_once("\n[").map(|(section, _)| section))
        .expect("workspace package section");
    assert!(
        !workspace_package
            .lines()
            .any(|line| line.starts_with("version"))
    );

    for (name, manifest) in [
        ("mant-ir", include_str!("../../mant-ir/Cargo.toml")),
        (
            "mant-protocol",
            include_str!("../../mant-protocol/Cargo.toml"),
        ),
        (
            "libmandoc-rs",
            include_str!("../../libmandoc-rs/Cargo.toml"),
        ),
        (
            "mant-sources",
            include_str!("../../mant-sources/Cargo.toml"),
        ),
        ("mant-engine", include_str!("../../mant-engine/Cargo.toml")),
        ("mant-ui", include_str!("../../mant-ui/Cargo.toml")),
        ("mant", include_str!("../Cargo.toml")),
    ] {
        assert!(
            !manifest
                .lines()
                .any(|line| line.trim() == "version.workspace = true"),
            "{name} inherits its version"
        );
        assert!(
            manifest
                .lines()
                .any(|line| line.starts_with("version = \"")),
            "{name} has no explicit package version"
        );
        for dependency in manifest
            .lines()
            .filter(|line| line.contains("path = \"../"))
        {
            assert!(
                dependency.contains("version = \"^"),
                "{name} has a non-caret internal dependency: {dependency}"
            );
        }
    }
}

#[test]
fn mcp_sdk_and_generated_macros_use_the_same_exact_release() {
    let root = include_str!("../../../Cargo.toml");
    let exact_version = |prefix: &str| {
        root.lines()
            .find_map(|line| {
                let suffix = line.strip_prefix(prefix)?;
                suffix.split_once('"').map(|(version, _)| version)
            })
            .unwrap_or_else(|| panic!("missing exact dependency prefix {prefix}"))
    };

    let sdk = exact_version("rmcp = { version = \"=");
    let macros = exact_version("rmcp-macros = \"=");
    assert_eq!(sdk, macros, "rmcp and rmcp-macros releases diverged");
}

#[test]
fn selected_crates_are_published_in_dependency_order_at_their_own_versions() {
    let publish = include_str!("../../../scripts/publish-crates.sh").replace("\r\n", "\n");
    assert!(publish.contains(
        "ALL_PACKAGES=(mant-ir mant-protocol libmandoc-rs mant-sources mant-engine mant-ui mant)"
    ));
    assert!(publish.contains(
        "CRATE_TAG_PACKAGES=(mant-ir mant-protocol libmandoc-rs mant-sources mant-engine mant-ui)"
    ));
    assert!(publish.contains("publish_package=${MANT_PUBLISH_PACKAGE:-}"));
    assert!(publish.contains(r#"[[ $tag == "$publish_package-v$version" ]]"#));
    assert!(publish.contains("version=$(package_version \"$package\")"));
    let publish_loop = publish
        .split_once("# A dependent crate can only be published after every version allowed by its")
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

#[cfg(unix)]
#[test]
fn publication_tags_are_validated_before_registry_authentication() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/publish-crates.sh");
    let ir_manifest = include_str!("../../mant-ir/Cargo.toml");
    let ir_version = ir_manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \"")?.strip_suffix('"'))
        .expect("mant-ir version");

    for (tag, package) in [
        (format!("v{}", env!("CARGO_PKG_VERSION")), None),
        (format!("mant-ir-v{ir_version}"), Some("mant-ir")),
    ] {
        let mut command = Command::new("bash");
        command
            .arg(&script)
            .env("MANT_RELEASE_TAG", tag)
            .env_remove("CARGO_REGISTRY_TOKEN");
        if let Some(package) = package {
            command.env("MANT_PUBLISH_PACKAGE", package);
        } else {
            command.env_remove("MANT_PUBLISH_PACKAGE");
        }
        let output = command.output().expect("run publication validation");
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("CARGO_REGISTRY_TOKEN is required"),
            "valid release selection failed before authentication: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let output = Command::new("bash")
        .arg(script)
        .env(
            "MANT_RELEASE_TAG",
            format!("mant-v{}", env!("CARGO_PKG_VERSION")),
        )
        .env("MANT_PUBLISH_PACKAGE", "mant")
        .env_remove("CARGO_REGISTRY_TOKEN")
        .output()
        .expect("reject a crate-only mant release");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown package 'mant'"));
}
