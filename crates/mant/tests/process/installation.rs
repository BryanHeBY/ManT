use super::process_support::run_with_registered_documents;
use std::fs;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "mant-installer-process-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(all(target_os = "linux", feature = "update"))]
mod unix_upgrade {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

    fn executable(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    struct ReleaseFixture {
        root: Fixture,
        version: &'static str,
    }

    impl ReleaseFixture {
        fn new() -> Self {
            let root = Fixture::new();
            let version = env!("CARGO_PKG_VERSION");
            let target = match std::env::consts::ARCH {
                "x86_64" => "linux-x64",
                "aarch64" => "linux-arm64",
                architecture => panic!("unsupported installer test architecture {architecture}"),
            };
            let package_name = format!("mant-{version}-{target}");
            let package = root.path().join(&package_name);
            fs::create_dir_all(package.join("manuals")).unwrap();
            fs::copy(
                super::super::process_support::executable(),
                package.join("mant"),
            )
            .unwrap();
            for name in [
                "mant.md",
                "mant-ir.md",
                "mant-markdown.md",
                "mant-protocol.md",
                "mant-roff.md",
            ] {
                fs::write(package.join("manuals").join(name), "# Bundled manual\n").unwrap();
            }
            fs::write(package.join("manuals/manifest.txt"), "mant.md\n").unwrap();
            let archive_name = format!("{package_name}.tar.gz");
            let archive = root.path().join(&archive_name);
            assert!(
                Command::new("tar")
                    .args(["-czf"])
                    .arg(&archive)
                    .arg("-C")
                    .arg(root.path())
                    .arg(&package_name)
                    .status()
                    .unwrap()
                    .success()
            );
            let hash = Command::new("sha256sum").arg(&archive).output().unwrap();
            assert!(hash.status.success());
            let hash = String::from_utf8(hash.stdout).unwrap();
            fs::write(
                root.path().join("SHA256SUMS"),
                format!(
                    "{}  {archive_name}\n",
                    hash.split_whitespace().next().unwrap()
                ),
            )
            .unwrap();
            let bin = root.path().join("tools");
            fs::create_dir(&bin).unwrap();
            executable(
                &bin.join("curl"),
                "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n case $1 in -o) output=$2; shift;; https://*) url=$1;; esac\n shift\ndone\ncp \"$MANT_TEST_ASSETS/${url##*/}\" \"$output\"\n",
            );
            executable(&bin.join("gh"), "#!/bin/sh\nexit 1\n");
            fs::create_dir_all(root.path().join(".local/bin")).unwrap();
            executable(
                &root.path().join(".local/bin/mant"),
                "#!/bin/sh\nprintf 'mant 0.11.0\\n'\n",
            );
            fs::create_dir_all(root.path().join(".local/share/mant/documents")).unwrap();
            fs::write(
                root.path().join(".local/share/mant/documents/user.md"),
                "# Original\n",
            )
            .unwrap();
            fs::write(
                root.path().join(".local/share/mant/documents/mant.md"),
                "# Old bundled\n",
            )
            .unwrap();
            fs::write(
                root.path().join(".local/share/mant/sources.toml"),
                "# original configuration\n",
            )
            .unwrap();
            let state = root.path().join(".local/state/mant");
            fs::create_dir_all(&state).unwrap();
            fs::write(state.join("install-receipt"), format!("schema\tmant.install/v1\nversion\t0.11.0\ninstall_dir\t{0}/.local/bin\ndata_dir\t{0}/.local/share/mant/documents\nbinary\t{0}/.local/bin/mant\nmanual\t{0}/.local/share/mant/documents/mant.md\n", root.path().display())).unwrap();
            Self { root, version }
        }

        fn run(&self, options: &[&str]) -> std::process::Output {
            let mut path = vec![self.root.path().join("tools")];
            path.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
            Command::new("sh")
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh"))
                .args(["--version", self.version])
                .args(options)
                .env_clear()
                .env("PATH", std::env::join_paths(path).unwrap())
                .env("HOME", self.root.path())
                .env("MANT_TEST_ASSETS", self.root.path())
                .env("MANT_DATA_HOME", self.root.path().join("current-data"))
                .output()
                .unwrap()
        }
    }

    #[test]
    fn upgrade_migrates_once_rebases_no_manual_ownership_and_preserves_originals() {
        let fixture = ReleaseFixture::new();
        let first = fixture.run(&["--no-manual"]);
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let root = fixture.root.path();
        assert!(root.join(".config/mant/sources.toml").is_file());
        assert!(root.join("current-data/documents/user.md").is_file());
        assert!(root.join(".local/share/mant/documents/user.md").is_file());
        let receipt = fs::read_to_string(root.join(".local/state/mant/install-receipt")).unwrap();
        assert!(receipt.contains("layout\tunix-v1"));
        assert!(receipt.contains(&format!(
            "manual\t{}/current-data/documents/mant.md",
            root.display()
        )));
        fs::write(
            root.join("current-data/documents/user.md"),
            "# Edited after upgrade\n",
        )
        .unwrap();
        let second = fixture.run(&["--force"]);
        assert!(
            second.status.success(),
            "{}",
            String::from_utf8_lossy(&second.stderr)
        );
        assert_eq!(
            fs::read_to_string(root.join("current-data/documents/user.md")).unwrap(),
            "# Edited after upgrade\n"
        );
    }

    #[test]
    fn migration_conflict_retains_old_binary_and_does_not_publish_configuration() {
        let fixture = ReleaseFixture::new();
        fs::create_dir_all(fixture.root.path().join("current-data/documents")).unwrap();
        fs::write(
            fixture.root.path().join("current-data/documents/user.md"),
            "# Existing\n",
        )
        .unwrap();
        let output = fixture.run(&[]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("migration conflict"));
        assert!(
            !fixture
                .root
                .path()
                .join(".config/mant/sources.toml")
                .exists()
        );
        assert!(
            fs::read_to_string(fixture.root.path().join(".local/bin/mant"))
                .unwrap()
                .contains("0.11.0")
        );
    }
}

#[test]
fn installer_paths_resolve_toml_and_environment_without_creating_storage() {
    let fixture = Fixture::new();
    let config = fixture.path().join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("mant.toml"),
        "[paths]\ndata_home = 'relative-data'\ncache_home = '~/custom-cache'\n",
    )
    .unwrap();
    let output = run_with_registered_documents(fixture.path(), &["--installer-paths"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paths: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // The fixture's explicit MANT_* overrides retain precedence over TOML.
    assert_eq!(paths["config"], config.to_str().unwrap());
    assert_eq!(
        paths["data"],
        fixture.path().join("data/mant").to_str().unwrap()
    );
    assert!(!fixture.path().join("data").exists());
    assert!(!config.join("relative-data").exists());
    let output = run_with_registered_documents(fixture.path(), &["--installer-paths", "unknown"]);
    assert!(!output.status.success());
}

#[cfg(feature = "update")]
#[test]
fn installer_bridge_migrates_but_ordinary_path_resolution_does_not() {
    let fixture = Fixture::new();
    let old = fixture.path().join("old");
    fs::create_dir_all(old.join("documents")).unwrap();
    fs::write(old.join("sources.toml"), "# legacy declarations\n").unwrap();
    fs::write(old.join("documents/personal.md"), "# Personal\n").unwrap();
    let output = run_with_registered_documents(fixture.path(), &["--installer-paths"]);
    assert!(output.status.success());
    assert!(!fixture.path().join("config").exists());
    let output = run_with_registered_documents(
        fixture.path(),
        &["--installer-migrate", old.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.path().join("config/sources.toml").is_file());
    assert!(
        fixture
            .path()
            .join("data/mant/documents/personal.md")
            .is_file()
    );
    assert!(old.join("documents/personal.md").is_file());
}
