//! Collector sidecar lifetime and measurement probes.

use super::*;

#[test]
fn collector_reuses_retired_ascii_projection_state() {
    // The registered oracle renders the minimal `\(em word` body as an
    // em dash followed by one space and `word`. Pinned term.c emits the
    // logical scalar before its ASCII projection, consumes only flushed
    // prefixes in term_field(), and retains multicol suffixes in
    // term_flushln(); retired sidecars must follow those same boundaries.
    let mut source = String::from(".TH RECYCLE 1\n.SH BODY\n");
    for _ in 0..4_096 {
        source.push_str("\\(em word\n");
    }
    let mut bundle = SourceBundle::new();
    bundle.insert("recycle.1", source.into_bytes()).unwrap();

    let metrics = probe_structured_profile(
        "recycle.1",
        &bundle,
        InputFormat::Man,
        78,
        PROFILE_ASCII,
        &Limits::default(),
    )
    .expect("probe the complete repeated formatter stream");

    assert!(metrics.token_count > 20_000);
    assert_bounded_sidecar(&metrics);
}

#[test]
#[ignore = "measurement probe: run in release mode under /usr/bin/time -v"]
fn probe_gcc_sidecar_workset() {
    run_real_fixture_probe("gcc.1.gz", "gcc.1");
}

#[test]
#[ignore = "measurement probe: run in release mode under /usr/bin/time -v"]
fn probe_git_sidecar_workset() {
    run_real_fixture_probe("git.1.gz", "git.1");
}

fn run_real_fixture_probe(fixture: &str, logical_name: &str) {
    use flate2::read::MultiGzDecoder;
    use std::{fs, io::Read, path::Path};

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/archlinux")
        .join(fixture);
    let mut bytes = Vec::new();
    MultiGzDecoder::new(fs::File::open(path).expect("open fixed probe fixture"))
        .read_to_end(&mut bytes)
        .expect("decode fixed probe fixture before measured calls");
    let mut bundle = SourceBundle::new();
    bundle.insert(logical_name, bytes).unwrap();

    for sample in 0..=10 {
        let metrics = probe_structured(
            logical_name,
            &bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("probe must traverse the full native renderer and discard its document");
        assert!(metrics.collector_events > 0);
        assert!(metrics.logical_events > 0);
        assert!(metrics.buffer_writes > 0);
        assert!(metrics.token_count > 0);
        assert!(metrics.slot_capacity > 0);
        assert!(metrics.sidecar_allocated_bytes > 0);
        assert_bounded_sidecar(&metrics);
        assert!(metrics.rendered_bytes > 0);
        if sample != 0 {
            eprintln!("{logical_name} sample={sample} {metrics:?}");
        }
    }
}

fn assert_bounded_sidecar(metrics: &ProbeMetrics) {
    // The collector retains only state reachable from active native
    // buffers. Keep this guard proportional to buffer capacity rather
    // than the cumulative logical-token count so an append-only token
    // history cannot silently return.
    let active_workset_bound = metrics
        .slot_capacity
        .saturating_mul(256)
        .saturating_add(1024 * 1024);
    assert!(
        metrics.sidecar_allocated_bytes <= active_workset_bound,
        "sidecar retained execution history: {metrics:?}"
    );
}
