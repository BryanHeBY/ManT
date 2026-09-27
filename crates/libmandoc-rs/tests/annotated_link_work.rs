#![cfg(feature = "annotated")]

//! R05 bounded candidate work and per-call resource recovery.

use libmandoc_rs::annotated::{AnnotatedDocument, AnnotatedRenderer};
use libmandoc_rs::{InputFormat, SourceBundle};

fn bundle(input: String) -> SourceBundle {
    let mut sources = SourceBundle::new();
    sources.insert("t.1", input.into_bytes()).unwrap();
    sources
}

fn render(sources: &SourceBundle) -> AnnotatedDocument {
    AnnotatedRenderer::default()
        .render_bundle("t.1", sources, InputFormat::Man)
        .unwrap()
}

fn clickable(page: &AnnotatedDocument) -> usize {
    page.marks
        .iter()
        .filter(|mark| mark.kind == 3 && mark.link_target.is_some())
        .count()
}

#[test]
fn a_long_non_candidate_word_does_not_invent_a_link() {
    // The exact 32,768-a input ran pinned CVS -Tutf8 -O width=78 first.
    // term.c::term_word() executes it as one ordinary word; the compatible
    // source scan is bounded without a repeated suffix search on misses.
    let sources = bundle(format!(".TH T 1\n.SH D\n{}\n", "a".repeat(32_768)));
    let page = render(&sources);
    assert_eq!(clickable(&page), 0);
    assert!(page.text.contains(&"a".repeat(32_768)));
}

#[test]
fn dense_repeated_markers_keep_distinct_bounded_occurrences() {
    // The exact 512-marker input ran pinned CVS -Tutf8 -O width=78 first.
    // One term_word() invocation can hold all markers, while consumed byte
    // intervals must still identify each candidate separately.
    let sources = bundle(format!(".TH T 1\n.SH D\n{}\n", "a(1) \\%<> ".repeat(512)));
    let page = render(&sources);
    assert_eq!(clickable(&page), 512);
    assert!(page.marks.len() < 2048);
}

#[test]
fn many_final_style_fragments_and_failed_budget_release_the_call_state() {
    // The exact 64-fragment input ran pinned CVS -Tutf8 -O width=78 first.
    // term.c::term_word() changes font inside one sourced word; final label
    // proof consumes the surviving slices and charges the same work budget.
    let sources = bundle(format!(
        ".TH T 1\n.SH D\n{}(1) \\%<>\n",
        "a\\fBb\\fP".repeat(64)
    ));
    let limited = AnnotatedRenderer::default()
        .with_max_builder_operations(1)
        .unwrap();
    let failure = limited
        .render_bundle("t.1", &sources, InputFormat::Man)
        .unwrap_err();
    assert_ne!(failure.limit_kind, 0);
    let page = render(&sources);
    assert_eq!(clickable(&page), 1);
    let link = page
        .marks
        .iter()
        .find(|mark| mark.kind == 3 && mark.link_target.is_some())
        .unwrap();
    assert!(link.selection_count >= 64);
}

#[test]
#[ignore = "local release-only scaling probe; not a timing assertion"]
fn compatible_candidate_scale_probe() {
    use std::time::Instant;

    // All five exact generated sizes first ran pinned CVS -Tutf8 -O
    // width=78. term.c::term_word() receives the whole sourced line;
    // repeated markers stress interval lookup, misses stress the scan.
    for (label, input) in [
        (
            "miss-32768",
            format!(".TH T 1\n.SH D\n{}\n", "a".repeat(32_768)),
        ),
        (
            "miss-65536",
            format!(".TH T 1\n.SH D\n{}\n", "a".repeat(65_536)),
        ),
        (
            "marker-512",
            format!(".TH T 1\n.SH D\n{}\n", "a(1) \\%<> ".repeat(512)),
        ),
        (
            "marker-1024",
            format!(".TH T 1\n.SH D\n{}\n", "a(1) \\%<> ".repeat(1024)),
        ),
        (
            "marker-2048",
            format!(".TH T 1\n.SH D\n{}\n", "a(1) \\%<> ".repeat(2048)),
        ),
    ] {
        let sources = bundle(input);
        let mut samples = Vec::with_capacity(12);
        for _ in 0..12 {
            let start = Instant::now();
            std::hint::black_box(render(&sources));
            samples.push(start.elapsed().as_micros());
        }
        samples.sort_unstable();
        println!("{label}: median={} us", samples[samples.len() / 2]);
    }
}
