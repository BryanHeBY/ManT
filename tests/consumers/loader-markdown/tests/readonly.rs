//! Exercise the compiled external consumer in an isolated process environment.

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        // Runtime fixture files and build products both stay under repo target.
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target"));
        let target = fs::canonicalize(target).expect("existing Cargo target directory");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = target.join(format!("loader-consumer-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).expect("unique fixture directory");
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the unique directory successfully created by this fixture is owned.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn standalone_file_loading_reads_source_without_creating_sources_or_caches() {
    let fixture = Fixture::new();
    let source = fixture.0.join("独立 source.md");
    for heading in ["First café", "Second 日本"] {
        let text = format!(
            "# {heading}\n\nActual file content.\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `--help`: Read the help.\n"
        );
        fs::write(&source, &text).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mant-loader-markdown-consumer"))
            .arg(&source)
            .arg(heading)
            .current_dir(&fixture.0)
            // Environment changes affect only the child, not parallel Rust tests.
            .env("HOME", fixture.0.join("home"))
            .env("USERPROFILE", fixture.0.join("home"))
            .env("APPDATA", fixture.0.join("appdata"))
            .env("LOCALAPPDATA", fixture.0.join("localappdata"))
            .env("XDG_DATA_HOME", fixture.0.join("data"))
            .env("XDG_CONFIG_HOME", fixture.0.join("config"))
            .env("XDG_CACHE_HOME", fixture.0.join("cache"))
            .env("MANT_MANPATH", fixture.0.join("manuals"))
            .env("MANPATH", fixture.0.join("manuals"))
            .output()
            .expect("start standalone loader consumer");
        assert!(
            output.status.success(),
            "consumer failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(&source).unwrap(), text);
        let remaining: Vec<_> = fs::read_dir(&fixture.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(
            remaining,
            [source.clone()],
            "read-only loading must not create configuration, sources, or caches"
        );
    }
}
