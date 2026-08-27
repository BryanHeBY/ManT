//! Generated parse-throughput benchmark for the native parser.
//!
//! This intentionally uses the same three redistributable documents as the
//! M0 `libmandoc-rs` AST-transfer benchmark.  It is a transparent, dependency-
//! free release-evidence tool rather than a portable performance threshold.
//!
//! Run with `cargo bench --locked --package mantdoc --bench parse`.

use std::{
    fmt::Write as _,
    hint::black_box,
    time::{Duration, Instant},
};

use mantdoc::{Compression, Parser, SourceName};

struct Case {
    name: &'static str,
    paragraphs: usize,
    iterations: u32,
}

fn main() {
    let parser = Parser::default();
    let source_name = SourceName::new("generated-transfer-benchmark.1")
        .expect("fixed benchmark source name is valid");
    let cases = [
        Case {
            name: "small",
            paragraphs: 100,
            iterations: 1_000,
        },
        Case {
            name: "medium",
            paragraphs: 1_000,
            iterations: 100,
        },
        Case {
            name: "large",
            paragraphs: 10_000,
            iterations: 10,
        },
    ];

    println!("case\tinput_bytes\tnodes\titerations\tns_per_parse");
    for case in &cases {
        run_case(&parser, &source_name, case);
    }
}

fn run_case(parser: &Parser, source_name: &SourceName, case: &Case) {
    let source = generated_manual(case.paragraphs);
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

fn generated_manual(paragraphs: usize) -> String {
    let mut source = String::with_capacity(paragraphs * 48);
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
