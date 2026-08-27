//! Verify native parser diagnostics against exact upstream mandoc lint output.

use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::ExitCode,
};

use mantdoc::{Diagnostic, ParseReport, Parser, Severity, Source, SourceName};

#[path = "../../tests/conformance/mod.rs"]
#[allow(dead_code, unused_imports)]
mod conformance;

use conformance::{CorpusCase, stable_1_14_6_inventory, stable_1_14_6_renderer_case};

pub fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let Some(archive) = arguments.next().map(PathBuf::from) else {
        usage(&program);
        return ExitCode::from(2);
    };
    let (shard, list_differences) = match arguments.next() {
        None => (None, false),
        Some(argument) if argument == "--all" => (None, false),
        Some(argument)
            if argument == "--all-shard" || argument == "--all-list-differences-shard" =>
        {
            let Some(value) = arguments.next() else {
                usage(&program);
                return ExitCode::from(2);
            };
            match parse_shard(&value) {
                Ok(value) => (Some(value), argument == "--all-list-differences-shard"),
                Err(error) => {
                    eprintln!("{}: {error}", program.to_string_lossy());
                    return ExitCode::from(2);
                }
            }
        }
        Some(_) => {
            usage(&program);
            return ExitCode::from(2);
        }
    };
    if arguments.next().is_some() {
        usage(&program);
        return ExitCode::from(2);
    }
    run_all(&program, &archive, shard, list_differences)
}

fn usage(program: &OsStr) {
    eprintln!(
        "usage: {} <mandoc-1.14.6.tar.gz> [--all | --all-shard INDEX/COUNT | --all-list-differences-shard INDEX/COUNT]",
        program.to_string_lossy()
    );
}

fn run_all(
    program: &OsStr,
    archive: &Path,
    shard: Option<(usize, usize)>,
    list_differences: bool,
) -> ExitCode {
    let inventory = match stable_1_14_6_inventory(archive) {
        Ok(inventory) => inventory,
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            return ExitCode::from(1);
        }
    };
    let mut equal = 0_usize;
    let mut different = 0_usize;
    let mut errors = 0_usize;
    let mut external = 0_usize;
    let mut first = None;
    for (case_index, case) in inventory.cases.iter().enumerate() {
        if shard.is_some_and(|(index, count)| case_index % count != index) || !has_lint_output(case)
        {
            continue;
        }
        match compare_case(archive, case) {
            Ok(Comparison::Equal { external_count }) => {
                equal += 1;
                external += external_count;
            }
            Ok(Comparison::Different {
                detail,
                external_count,
            }) => {
                different += 1;
                external += external_count;
                if list_differences {
                    println!("lint_difference_case={} {detail}", case.id);
                }
                first.get_or_insert_with(|| (case.id.clone(), detail));
            }
            Err(error) => {
                errors += 1;
                first.get_or_insert_with(|| (case.id.clone(), error));
            }
        }
    }
    if let Some((index, count)) = shard {
        println!("shard_index={index}");
        println!("shard_count={count}");
    }
    println!("lint_output_count={}", equal + different + errors);
    println!("lint_equal_output_count={equal}");
    println!("lint_difference_output_count={different}");
    println!("lint_error_output_count={errors}");
    println!("lint_external_output_count={external}");
    if let Some((case, detail)) = first {
        println!("first_difference_case={case}");
        println!("first_difference={detail}");
    }
    if different == 0 && errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn has_lint_output(case: &CorpusCase) -> bool {
    case.expected_outputs
        .iter()
        .any(|output| output.format.as_ref() == "lint")
}

enum Comparison {
    Equal {
        external_count: usize,
    },
    Different {
        detail: String,
        external_count: usize,
    },
}

fn compare_case(archive: &Path, case: &CorpusCase) -> Result<Comparison, String> {
    let payload = stable_1_14_6_renderer_case(archive, &case.id, &["lint"])
        .map_err(|error| error.to_string())?;
    let expected = payload
        .outputs
        .into_iter()
        .next()
        .ok_or_else(|| "verified lint payload did not retain an output".to_owned())?
        .output_bytes;
    let external_count = external_lint_diagnostic_count(&expected);
    let expected = parser_lint_output(&expected);
    let source_name = SourceName::new(lint_source_label(&payload.source.case.input_archive_path))
        .map_err(|error| format!("invalid verified source name: {error}"))?;
    let report = Parser::default()
        .parse(Source::new(&source_name, &payload.source.source_bytes))
        .map_err(|error| error.to_string())?;
    let actual = format_lint(&report, source_name.as_str());
    if actual.as_bytes() == expected.as_slice() {
        Ok(Comparison::Equal { external_count })
    } else {
        Ok(Comparison::Different {
            detail: difference_detail(&expected, actual.as_bytes()),
            external_count,
        })
    }
}

/// The `mandoc` command-line driver performs an external manual-database
/// lookup after parsing.  `mantdoc` deliberately has no host database access,
/// so this is not a parser diagnostic (nor one emitted by libmandoc-rs).
const EXTERNAL_XR_LINT_MARKER: &[u8] = b": STYLE: referenced manual not found: Xr ";

fn external_lint_diagnostic_count(output: &[u8]) -> usize {
    output
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| is_external_xr_lint_diagnostic(line))
        .count()
}

fn parser_lint_output(output: &[u8]) -> Vec<u8> {
    output
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| !is_external_xr_lint_diagnostic(line))
        .flatten()
        .copied()
        .collect()
}

fn is_external_xr_lint_diagnostic(line: &[u8]) -> bool {
    line.windows(EXTERNAL_XR_LINT_MARKER.len())
        .any(|window| window == EXTERNAL_XR_LINT_MARKER)
}

fn lint_source_label(input_archive_path: &str) -> &str {
    input_archive_path
        .rsplit('/')
        .next()
        .expect("verified upstream input path has a basename")
}

fn format_lint(report: &ParseReport, fallback_source_name: &str) -> String {
    let mut output = String::new();
    for diagnostic in &report.diagnostics {
        let (source_name, position) = diagnostic
            .primary
            .as_ref()
            .and_then(|span| {
                report.document.source_position(span).map(|position| {
                    let source = report
                        .document
                        .source_name(span.source)
                        .map_or(fallback_source_name, mantdoc::SourceName::as_str);
                    (lint_source_label(source), position)
                })
            })
            .unwrap_or((
                fallback_source_name,
                mantdoc::SourcePosition { line: 0, column: 0 },
            ));
        output.push_str("mandoc: ");
        output.push_str(source_name);
        if position.line != 0 {
            output.push(':');
            output.push_str(&position.line.to_string());
            output.push(':');
            output.push_str(&position.column.to_string());
        }
        output.push_str(": ");
        output.push_str(lint_severity(diagnostic));
        output.push_str(": ");
        output.push_str(&diagnostic.message);
        output.push('\n');
    }
    output
}

const fn lint_severity(diagnostic: &Diagnostic) -> &'static str {
    match diagnostic.severity {
        Severity::Unsupported => "UNSUPP",
        Severity::Error => "ERROR",
        Severity::Warning => "WARNING",
        Severity::Style => "STYLE",
    }
}

fn difference_detail(expected: &[u8], actual: &[u8]) -> String {
    let offset = expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    let start = offset.saturating_sub(96);
    let end = expected
        .len()
        .max(actual.len())
        .min(offset.saturating_add(192));
    format!(
        "byte:{offset} expected={:?} actual={:?}",
        String::from_utf8_lossy(&expected[start.min(expected.len())..end.min(expected.len())]),
        String::from_utf8_lossy(&actual[start.min(actual.len())..end.min(actual.len())])
    )
}

fn parse_shard(value: &OsStr) -> Result<(usize, usize), String> {
    let value = value
        .to_str()
        .ok_or_else(|| "shard must be valid UTF-8 in the form INDEX/COUNT".to_owned())?;
    let (index, count) = value
        .split_once('/')
        .ok_or_else(|| "shard must use zero-based INDEX/COUNT syntax".to_owned())?;
    let index = index
        .parse::<usize>()
        .map_err(|_| "shard index must be an unsigned integer".to_owned())?;
    let count = count
        .parse::<usize>()
        .map_err(|_| "shard count must be an unsigned integer".to_owned())?;
    if count == 0 || index >= count {
        return Err("shard must satisfy 0 <= INDEX < COUNT".to_owned());
    }
    Ok((index, count))
}

#[cfg(test)]
mod tests {
    use super::{
        external_lint_diagnostic_count, lint_severity, lint_source_label, parser_lint_output,
    };
    use mantdoc::{Diagnostic, DiagnosticCode, Severity};

    #[test]
    fn lint_source_uses_the_upstream_basename() {
        assert_eq!(lint_source_label("regress/roff/cond/if.in"), "if.in");
    }

    #[test]
    fn lint_severity_matches_mandoc_spelling() {
        let code = DiagnosticCode::new("fixture.code").unwrap();
        for (severity, expected) in [
            (Severity::Unsupported, "UNSUPP"),
            (Severity::Error, "ERROR"),
            (Severity::Warning, "WARNING"),
            (Severity::Style, "STYLE"),
        ] {
            assert_eq!(
                lint_severity(&Diagnostic::new(code.clone(), severity, "fixture")),
                expected
            );
        }
    }

    #[test]
    fn parser_lint_output_excludes_only_external_xr_lookup_findings() {
        let output = b"mandoc: x.in:1:1: WARNING: parser finding\nmandoc: x.in:2:1: STYLE: referenced manual not found: Xr x 1\n";
        assert_eq!(external_lint_diagnostic_count(output), 1);
        assert_eq!(
            parser_lint_output(output),
            b"mandoc: x.in:1:1: WARNING: parser finding\n"
        );
    }
}
