#![cfg(feature = "annotated")]

//! R01 native-only capture of the P0 representative pages. This does not
//! claim an IR, query, CLI or TUI consumer has migrated.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

use libmandoc_rs::annotated::AnnotatedRenderer;
use libmandoc_rs::{InputFormat, SourceBundle};

#[test]
#[ignore = "manual G1 audit: writes patched raw renderer output under repository target"]
#[cfg(feature = "render")]
fn emit_patched_raw_renderer_for_four_page_display_comparison() {
    // These exact decoded fixtures were run through the pinned CVS reference
    // before G1 assertions. This extra output is from the approved patched
    // renderer, so patch-specific wrapping can be compared independently of
    // pristine CVS. No source or fixture file is modified.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/roff/real");
    let audit =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/annotated-g1-patched-raw");
    fs::create_dir_all(&audit).unwrap();
    for (name, path, gzip) in [
        ("gcc.1", "archlinux/gcc.1.gz", true),
        ("git.1", "archlinux/git.1.gz", true),
        ("clang.1", "archlinux/clang.1.gz", true),
        ("rclone.1", "windows-releases/rclone.1.zst", false),
    ] {
        let compressed = fs::read(root.join(path)).unwrap();
        let decoded = if gzip {
            let mut decoded = Vec::new();
            flate2::read::MultiGzDecoder::new(compressed.as_slice())
                .read_to_end(&mut decoded)
                .unwrap();
            decoded
        } else {
            zstd::stream::decode_all(compressed.as_slice()).unwrap()
        };
        let mut bundle = SourceBundle::new();
        bundle.insert(name, decoded).unwrap();
        let raw = libmandoc_rs::Renderer::new(libmandoc_rs::RenderFormat::Utf8)
            .render_bundle(name, &bundle)
            .unwrap();
        fs::write(audit.join(format!("{name}.utf8")), raw.output).unwrap();
    }
}

#[test]
fn four_representative_pages_have_checked_native_surfaces() {
    // The exact decoded fixtures were run through the fixed CVS -Tutf8
    // -O width=78 reference before this new-path capture assertion.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/roff/real");
    for (name, path, gzip) in [
        ("gcc.1", "archlinux/gcc.1.gz", true),
        ("git.1", "archlinux/git.1.gz", true),
        ("clang.1", "archlinux/clang.1.gz", true),
        ("rclone.1", "windows-releases/rclone.1.zst", false),
    ] {
        let compressed = fs::read(root.join(path)).unwrap();
        let decoded = if gzip {
            let mut decoded = Vec::new();
            flate2::read::MultiGzDecoder::new(compressed.as_slice())
                .read_to_end(&mut decoded)
                .unwrap();
            decoded
        } else {
            zstd::stream::decode_all(compressed.as_slice()).unwrap()
        };
        let mut bundle = SourceBundle::new();
        let input_bytes = decoded.len();
        bundle.insert(name, decoded).unwrap();
        let started = Instant::now();
        let page = AnnotatedRenderer::default()
            .render_bundle(name, &bundle, InputFormat::Man)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        eprintln!(
            "{name}: input={input_bytes} surface={} rows={} runs={} marks={} parts={} join_bytes={} elapsed_ms={}",
            page.text.len(),
            page.rows.len(),
            page.runs.len(),
            page.marks.len(),
            page.selection_parts.len(),
            page.join_text.len(),
            started.elapsed().as_millis()
        );
        assert!(!page.text.is_empty(), "{name}");
        assert_eq!(page.sources.len(), 1, "{name}");
        assert!(!page.rows.is_empty(), "{name}");
        assert!(!page.runs.is_empty(), "{name}");
        assert_eq!(page.coverage.checks.len(), 24, "{name}");
        for check in &page.coverage.checks {
            let has_issue = page.coverage.issues.iter().any(|issue| {
                issue.producer == check.producer && issue.dimension == check.dimension
            });
            assert_eq!(
                has_issue,
                check.state == libmandoc_rs::annotated::AnnotationCheckState::Unverified,
                "{name}"
            );
        }
    }
}
