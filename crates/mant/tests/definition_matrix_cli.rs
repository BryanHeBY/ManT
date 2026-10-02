//! Process-level twin of the pinned definition row-machine matrix.
//!
//! `mant-engine`'s `definition_matrix` suite pins the lowered renderer
//! against exported reference snapshots. This suite exercises the same
//! case set through the real CLI process (`man <name>` with
//! `MANT_MANPATH`), asserting the process output is byte-identical to
//! the library rendering — the only behavior the retired /tmp probe
//! shell still covered — plus row-grouping agreement with the snapshot
//! on a sample.

#![cfg(feature = "roff")]

use std::{path::Path, path::PathBuf, process::Command};

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../mant-engine/tests/roff_lowering/definition_matrix/cases"
);

/// Minimal scratch manroot without the tempfile crate: a unique directory
/// under the target temp dir, removed on drop.
struct ScratchManroot {
    path: PathBuf,
}

impl ScratchManroot {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("mant-matrix-{label}-{}", std::process::id()));
        std::fs::create_dir_all(path.join("man1")).expect("scratch man1");
        Self { path }
    }
}

impl Drop for ScratchManroot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The CLI and library renderer have one label row and no footer. Remove
/// that first row by position, then collapse horizontal whitespace and drop
/// blank rows: the observable is which body words share a physical row.
/// Body text never becomes page furniture because of its spelling.
fn row_groups(output: &str) -> Vec<String> {
    output
        .lines()
        .skip(1)
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn row_groups_preserves_body_text_that_resembles_page_furniture() {
    // This is the footerless renderer's position contract, matching the
    // engine definition matrix. No roff behavior is inferred from word shape.
    let output = concat!(
        "arbitrary first-row label\n",
        "\n",
        "OPTIONS\n",
        "  BODY(1)\n",
        "printf(3)\n",
        "Linux commands begin the body here.\n",
        "\n",
        "2026-09-30 printf(3)\n",
        "September 30, 2026\n",
    );
    assert_eq!(
        row_groups(output),
        [
            "OPTIONS",
            "BODY(1)",
            "printf(3)",
            "Linux commands begin the body here.",
            "2026-09-30 printf(3)",
            "September 30, 2026",
        ]
    );
}

fn library_rendering(source: &str) -> String {
    let query = mant_loader::load_roff_bytes(source.as_bytes())
        .expect("lower the matrix case through the library");
    mant_render::render_query_man(&query)
}

#[test]
fn cli_matrix_matches_the_library_rendering() {
    let mut checked = 0;
    let mut entries: Vec<_> = std::fs::read_dir(CASES)
        .expect("definition matrix case directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "1"))
        .collect();
    entries.sort();
    assert_eq!(entries.len(), 54, "case set must match the pinned suite");
    for source_path in entries {
        let name = source_path
            .file_stem()
            .expect("case stem")
            .to_string_lossy()
            .into_owned();
        // Each case installs into its own manroot: the cases share `Dt`
        // titles, and one shared root would collide in title lookup.
        let scratch = ScratchManroot::new(&name);
        std::fs::copy(&source_path, scratch.path.join(format!("man1/{name}.1")))
            .expect("install case");

        let output = Command::new(env!("CARGO_BIN_EXE_mant"))
            .arg(&name)
            .args(["--manual", "--format", "man"])
            .env("MANT_MANPATH", &scratch.path)
            .output()
            .expect("run the mant process");
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let process_text = String::from_utf8(output.stdout).expect("process output");
        let library_text =
            library_rendering(&std::fs::read_to_string(&source_path).expect("source"));
        // The process and the library differ only in page furniture
        // (`name(1)` label sourcing); the row grouping must be identical.
        assert_eq!(
            row_groups(&process_text),
            row_groups(&library_text),
            "{name}: process vs library"
        );
        checked += 1;
    }
    assert_eq!(checked, 54);
}

#[test]
fn cli_matrix_rows_match_a_snapshot_sample() {
    let scratch = ScratchManroot::new("full");
    let man1 = scratch.path.join("man1");
    for name in ["m10_hang_h10", "m11_tag_h1", "m18_tag_h8", "m42_diag_h2"] {
        let source_path = Path::new(CASES).join(format!("{name}.1"));
        let expected_path = Path::new(CASES).join(format!("{name}.expected"));
        let _source = std::fs::read_to_string(&source_path).unwrap_or_else(|error| {
            panic!("{name}: read case: {error}");
        });
        std::fs::copy(&source_path, man1.join(format!("{name}.1"))).expect("install case");
        let output = Command::new(env!("CARGO_BIN_EXE_mant"))
            .arg(name)
            .args(["--manual", "--format", "man"])
            .env("MANT_MANPATH", &scratch.path)
            .output()
            .expect("run the mant process");
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let process_text = String::from_utf8(output.stdout).expect("process output");
        let rows = row_groups(&process_text);
        let expected: Vec<String> = std::fs::read_to_string(&expected_path)
            .expect("snapshot")
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(rows, expected, "{name}");
    }
}
