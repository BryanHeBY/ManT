//! HTML throughput for long consecutive man(7) field groups.
//!
//! Run with:
//! `cargo bench --locked --package mantdoc --bench render_html_groups --features render`.

use std::{fmt::Write as _, hint::black_box, time::Instant};

use mantdoc::{Compression, Parser, RenderFormat, Renderer, SourceName};

fn main() {
    let parser = Parser::default();
    let renderer = Renderer::new(RenderFormat::Html);
    let source_name =
        SourceName::new("generated-html-groups.1").expect("fixed source name is valid");

    println!("fields\tinput_bytes\toutput_bytes\titerations\tparse_ns\tparse_render_ns");
    for (fields, iterations) in [(100, 20), (1_000, 5), (5_000, 1)] {
        run_case(&parser, &renderer, &source_name, fields, iterations);
    }
}

fn run_case(
    parser: &Parser,
    renderer: &Renderer,
    source_name: &SourceName,
    fields: usize,
    iterations: u32,
) {
    let source = generated_tagged_manual(fields);
    let initial = renderer
        .render_bytes(source_name, source.as_bytes(), Compression::Plain)
        .expect("generated tagged manual must render");
    black_box(&initial);

    let parse_started = Instant::now();
    for _ in 0..iterations {
        black_box(
            parser
                .parse_bytes(source_name, source.as_bytes(), Compression::Plain)
                .expect("generated tagged manual must keep parsing"),
        );
    }
    let parse_elapsed = parse_started.elapsed();
    let started = Instant::now();
    for _ in 0..iterations {
        black_box(
            renderer
                .render_bytes(source_name, source.as_bytes(), Compression::Plain)
                .expect("generated tagged manual must keep rendering"),
        );
    }
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        fields,
        source.len(),
        initial.output.len(),
        iterations,
        parse_elapsed.as_nanos() / u128::from(iterations),
        started.elapsed().as_nanos() / u128::from(iterations),
    );
}

fn generated_tagged_manual(fields: usize) -> String {
    let mut source = String::with_capacity(fields * 64);
    source.push_str(".TH TAGS 1\n.SH NAME\ntags \\- generated HTML benchmark\n.SH DESCRIPTION\n");
    for index in 0..fields {
        writeln!(
            source,
            ".TP\ntag-{index}\ndescription {index} carries stable visible text."
        )
        .expect("writing into String is infallible");
    }
    source
}
