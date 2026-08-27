//! Generated parse-plus-lower throughput benchmark.
//!
//! Run with `cargo bench --locked --package mant-engine --bench manual_pipeline`.

use std::{
    fmt::Write as _,
    hint::black_box,
    path::Path,
    time::{Duration, Instant},
};

use mant_engine::parse_manual_bytes;

struct Case {
    name: &'static str,
    paragraphs: usize,
    iterations: u32,
}

fn main() {
    let path = Path::new("generated-pipeline-benchmark.1");
    let cases = [
        Case {
            name: "small",
            paragraphs: 100,
            iterations: 500,
        },
        Case {
            name: "medium",
            paragraphs: 1_000,
            iterations: 50,
        },
        Case {
            name: "large",
            paragraphs: 10_000,
            iterations: 5,
        },
    ];

    println!("case\tinput_bytes\titerations\tns_per_parse_lower");
    for case in &cases {
        run_case(path, case);
    }
}

fn run_case(path: &Path, case: &Case) {
    let source = generated_manual(case.paragraphs);
    parse_manual_bytes(path, source.as_bytes()).expect("generated benchmark input must lower");
    for _ in 0..2 {
        black_box(
            parse_manual_bytes(path, source.as_bytes())
                .expect("generated benchmark warm-up must lower"),
        );
    }

    let started = Instant::now();
    for _ in 0..case.iterations {
        black_box(
            parse_manual_bytes(path, source.as_bytes())
                .expect("generated benchmark input must keep lowering"),
        );
    }
    println!(
        "{}\t{}\t{}\t{}",
        case.name,
        source.len(),
        case.iterations,
        per_iteration(started.elapsed(), case.iterations),
    );
}

fn generated_manual(paragraphs: usize) -> String {
    let mut source = String::with_capacity(paragraphs * 64);
    source.push_str(".TH TRANSFER 1\n.SH NAME\ntransfer \\- generated benchmark\n");
    for index in 0..paragraphs {
        writeln!(
            source,
            ".PP\nparagraph {index} carries stable visible text and \\fBstyle\\fR."
        )
        .expect("writing into String is infallible");
    }
    source
}

fn per_iteration(elapsed: Duration, iterations: u32) -> u128 {
    elapsed.as_nanos() / u128::from(iterations)
}
