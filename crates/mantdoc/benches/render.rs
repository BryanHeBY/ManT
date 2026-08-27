//! Generated parse-plus-render throughput benchmark.
//!
//! Run with `cargo bench --locked --package mantdoc --bench render --features render`.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use mantdoc::{Compression, RenderFormat, Renderer, SourceName};

mod support;

use support::{Case, generated_cases};

fn main() {
    let renderer = Renderer::new(RenderFormat::Utf8);
    let source_name = SourceName::new("generated-render-benchmark.1")
        .expect("fixed benchmark source name is valid");

    println!("case\tinput_bytes\toutput_bytes\titerations\tns_per_parse_render");
    for mut case in generated_cases() {
        case.iterations = render_iterations(case.iterations);
        run_case(&renderer, &source_name, &case);
    }
}

fn run_case(renderer: &Renderer, source_name: &SourceName, case: &Case) {
    let source = (case.generate)();
    let initial = renderer
        .render_bytes(source_name, source.as_bytes(), Compression::Plain)
        .expect("generated benchmark input must render");

    for _ in 0..1 {
        black_box(
            renderer
                .render_bytes(source_name, source.as_bytes(), Compression::Plain)
                .expect("generated benchmark warm-up must render"),
        );
    }

    let started = Instant::now();
    for _ in 0..case.iterations {
        black_box(
            renderer
                .render_bytes(source_name, source.as_bytes(), Compression::Plain)
                .expect("generated benchmark input must keep rendering"),
        );
    }
    println!(
        "{}\t{}\t{}\t{}\t{}",
        case.name,
        source.len(),
        initial.output.len(),
        case.iterations,
        per_iteration(started.elapsed(), case.iterations),
    );
}

const fn render_iterations(parse_iterations: u32) -> u32 {
    let reduced = parse_iterations / 20;
    if reduced == 0 { 1 } else { reduced }
}

fn per_iteration(elapsed: Duration, iterations: u32) -> u128 {
    elapsed.as_nanos() / u128::from(iterations)
}
