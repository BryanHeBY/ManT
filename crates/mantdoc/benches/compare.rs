//! Compare native library throughput with installed mandoc and groff CLIs.
//!
//! External measurements include process startup and write output to the null
//! device. They represent command-line latency, while the mantdoc columns
//! represent reusable in-process library calls.
//!
//! Run with:
//! `cargo bench --locked --package mantdoc --bench compare --features render`.

use std::{
    ffi::OsStr,
    fs,
    hint::black_box,
    path::Path,
    process::{Command, Stdio},
    time::Instant,
};

use mantdoc::{Compression, Parser, RenderFormat, Renderer, SourceName};

mod support;

use support::{Case, generated_cases};

const EXTERNAL_SAMPLES: usize = 9;
const INTERNAL_SAMPLES: usize = 5;

fn main() {
    let parser = Parser::default();
    let renderer = Renderer::new(RenderFormat::Utf8);
    let source_name = SourceName::new("generated-comparison-benchmark.1")
        .expect("fixed benchmark source name is valid");
    let temporary = std::env::temp_dir().join(format!(
        "mantdoc-formatter-comparison-{}",
        std::process::id()
    ));
    fs::create_dir_all(&temporary).expect("comparison directory can be created");

    println!(
        "case\tbytes\tmantdoc_parse_ns\tmandoc_lint_ns\tmantdoc_render_ns\tmandoc_render_ns\tgroff_render_ns"
    );
    for case in generated_cases() {
        run_case(&parser, &renderer, &source_name, &temporary, &case);
    }

    fs::remove_dir_all(&temporary).expect("comparison directory can be removed");
}

fn run_case(
    parser: &Parser,
    renderer: &Renderer,
    source_name: &SourceName,
    temporary: &Path,
    case: &Case,
) {
    let source = (case.generate)();
    let path = temporary.join(format!("{}.roff", case.name));
    fs::write(&path, source.as_bytes()).expect("generated comparison input can be written");

    let mantdoc_parse = median_internal(case.iterations, || {
        black_box(
            parser
                .parse_bytes(source_name, source.as_bytes(), Compression::Plain)
                .expect("generated comparison input must parse"),
        );
    });
    let native_render = median_internal(render_iterations(case.iterations), || {
        black_box(
            renderer
                .render_bytes(source_name, source.as_bytes(), Compression::Plain)
                .expect("generated comparison input must render"),
        );
    });

    let package = if case.name == "mdoc-inline" {
        "-mdoc"
    } else {
        "-man"
    };
    let mandoc_lint = median_command("mandoc", &["-T", "lint", package], &path, false);
    let mandoc_render = median_command("mandoc", &["-T", "utf8", package], &path, true);
    let groff_render = match case.name {
        "tbl-heavy" => median_command("groff", &["-Tutf8", "-t", package], &path, true),
        "eqn-heavy" => median_command("groff", &["-Tutf8", "-e", package], &path, true),
        _ => median_command("groff", &["-Tutf8", package], &path, true),
    };

    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        case.name,
        source.len(),
        mantdoc_parse,
        display_optional(mandoc_lint),
        native_render,
        display_optional(mandoc_render),
        display_optional(groff_render),
    );
}

fn median_internal(mut iterations: u32, mut operation: impl FnMut()) -> u128 {
    iterations = iterations.max(1);
    operation();
    let mut samples = Vec::with_capacity(INTERNAL_SAMPLES);
    for _ in 0..INTERNAL_SAMPLES {
        let started = Instant::now();
        for _ in 0..iterations {
            operation();
        }
        samples.push(started.elapsed().as_nanos() / u128::from(iterations));
    }
    median(&mut samples)
}

fn median_command<S>(
    program: &str,
    arguments: &[S],
    path: &Path,
    require_success: bool,
) -> Option<u128>
where
    S: AsRef<OsStr>,
{
    if Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        return None;
    }
    let run = || {
        let mut command = Command::new(program);
        command
            .args(arguments.iter())
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let started = Instant::now();
        let status = command.status().ok()?;
        (!require_success || status.success()).then(|| started.elapsed().as_nanos())
    };
    run()?;
    let mut samples = Vec::with_capacity(EXTERNAL_SAMPLES);
    for _ in 0..EXTERNAL_SAMPLES {
        samples.push(run()?);
    }
    Some(median(&mut samples))
}

fn median(samples: &mut [u128]) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn display_optional(value: Option<u128>) -> String {
    value.map_or_else(|| "unavailable".into(), |value| value.to_string())
}

const fn render_iterations(iterations: u32) -> u32 {
    let reduced = iterations / 10;
    if reduced == 0 { 1 } else { reduced }
}
