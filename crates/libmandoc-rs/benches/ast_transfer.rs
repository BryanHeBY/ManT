//! Allocation-free harness around the complete native-parse-to-owned-AST path.
//!
//! Run with `cargo bench -p libmandoc-rs --bench ast_transfer`. The generated
//! inputs keep the benchmark redistributable and make node counts stable across
//! hosts while still exercising the same FFI ownership transfer as real pages.

use std::{
    fmt::Write as _,
    hint::black_box,
    mem::size_of,
    path::Path,
    time::{Duration, Instant},
};

use libmandoc_rs::{Compression, IncludePolicy, Node, ParseOptions, Parser};

struct Case {
    name: &'static str,
    paragraphs: usize,
    iterations: u32,
}

#[derive(Default)]
struct AstSize {
    nodes: usize,
    child_storage: usize,
    string_storage: usize,
}

fn main() {
    let parser = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    });
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

    println!("case\tinput_bytes\tnodes\towned_ast_bytes\titerations\tns_per_parse");
    for case in cases {
        run_case(&parser, &case);
    }
}

fn run_case(parser: &Parser, case: &Case) {
    let source = generated_manual(case.paragraphs);
    let path = Path::new("generated-transfer-benchmark.1");
    let initial = parser
        .parse_bytes(path, source.as_bytes())
        .expect("generated benchmark input must parse");
    let mut size = AstSize::default();
    measure_node(&initial.document.root, true, &mut size);

    for _ in 0..3 {
        black_box(
            parser
                .parse_bytes(path, source.as_bytes())
                .expect("generated benchmark warm-up must parse"),
        );
    }

    let started = Instant::now();
    for _ in 0..case.iterations {
        black_box(
            parser
                .parse_bytes(path, source.as_bytes())
                .expect("generated benchmark input must keep parsing"),
        );
    }
    let elapsed = started.elapsed();
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        case.name,
        source.len(),
        size.nodes,
        size.child_storage + size.string_storage,
        case.iterations,
        per_iteration(elapsed, case.iterations),
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
        .expect("write generated source");
    }
    source
}

fn measure_node(node: &Node, root: bool, size: &mut AstSize) {
    size.nodes += 1;
    if root {
        size.child_storage += size_of::<Node>();
    }
    size.child_storage += node.children.capacity() * size_of::<Node>();
    for value in [
        node.macro_name.as_ref(),
        node.text.as_ref(),
        node.tag.as_ref(),
        node.offset.as_ref(),
        node.width.as_ref(),
        node.equation.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        size.string_storage += value.capacity();
    }
    if let Some(enclosure) = &node.enclosure {
        size.string_storage += enclosure.opening.capacity();
        size.string_storage += enclosure.closing.as_ref().map_or(0, String::capacity);
    }
    for cell in &node.table_cells {
        size.string_storage += cell.text.as_ref().map_or(0, String::capacity);
    }
    for child in &node.children {
        measure_node(child, false, size);
    }
}

fn per_iteration(elapsed: Duration, iterations: u32) -> u128 {
    elapsed.as_nanos() / u128::from(iterations)
}
