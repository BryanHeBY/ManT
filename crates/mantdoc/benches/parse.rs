//! Generated parse-throughput benchmark for the native parser.
//!
//! This intentionally uses the same three redistributable documents as the
//! M0 `libmandoc-rs` AST-transfer benchmark.  It is a transparent, dependency-
//! free release-evidence tool rather than a portable performance threshold.
//!
//! Run with `cargo bench --locked --package mantdoc --bench parse`.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use mantdoc::{Compression, Parser, SourceName};

mod support;

use support::{Case, generated_cases};

fn main() {
    let parser = Parser::default();
    let source_name = SourceName::new("generated-transfer-benchmark.1")
        .expect("fixed benchmark source name is valid");
    let cases = generated_cases();

    println!("case\tinput_bytes\tnodes\titerations\tns_per_parse");
    for case in &cases {
        run_case(&parser, &source_name, case);
    }
}

fn run_case(parser: &Parser, source_name: &SourceName, case: &Case) {
    let source = (case.generate)();
    let initial = parser
        .parse_bytes(source_name, source.as_bytes(), Compression::Plain)
        .expect("generated benchmark input must parse");

    for _ in 0..3 {
        black_box(
            parser
                .parse_bytes(source_name, source.as_bytes(), Compression::Plain)
                .expect("generated benchmark warm-up must parse"),
        );
    }

    let started = Instant::now();
    for _ in 0..case.iterations {
        black_box(
            parser
                .parse_bytes(source_name, source.as_bytes(), Compression::Plain)
                .expect("generated benchmark input must keep parsing"),
        );
    }
    println!(
        "{}\t{}\t{}\t{}\t{}",
        case.name,
        source.len(),
        initial.document.node_count(),
        case.iterations,
        per_iteration(started.elapsed(), case.iterations),
    );
}

fn per_iteration(elapsed: Duration, iterations: u32) -> u128 {
    elapsed.as_nanos() / u128::from(iterations)
}
