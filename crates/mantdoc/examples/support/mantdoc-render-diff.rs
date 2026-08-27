//! Observe exact upstream renderer-golden differences for native `mantdoc`.
//!
//! This is an M9 evidence tool, not a pass/fail compatibility gate while the
//! native reference renderers are being implemented. It validates the pinned
//! archive and every selected output digest, then reports exact byte equality
//! without invoking or linking the C renderer.

use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::ExitCode,
};

use mantdoc::{RenderFormat, Renderer, Source, SourceName};
#[path = "../../tests/conformance/mod.rs"]
#[allow(dead_code, unused_imports)]
mod conformance;

use conformance::{CorpusCase, stable_1_14_6_inventory, stable_1_14_6_renderer_case};

const RENDER_FORMATS: [&str; 3] = ["ascii", "utf8", "html"];

pub fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let Some(archive) = arguments.next().map(PathBuf::from) else {
        usage(&program);
        return ExitCode::from(2);
    };
    let Some(target) = arguments.next() else {
        return run_all(&program, &archive, None, false, false);
    };
    if target == "--all" {
        return if arguments.next().is_none() {
            run_all(&program, &archive, None, false, false)
        } else {
            usage(&program);
            ExitCode::from(2)
        };
    }
    if target == "--all-shard" {
        let Some(shard) = arguments.next() else {
            usage(&program);
            return ExitCode::from(2);
        };
        return if arguments.next().is_some() {
            usage(&program);
            ExitCode::from(2)
        } else {
            match parse_shard(&shard) {
                Ok(shard) => run_all(&program, &archive, Some(shard), false, false),
                Err(error) => {
                    eprintln!("{}: {error}", program.to_string_lossy());
                    ExitCode::from(2)
                }
            }
        };
    }
    if target == "--all-list-shard" || target == "--all-list-differences-shard" {
        let Some(shard) = arguments.next() else {
            usage(&program);
            return ExitCode::from(2);
        };
        return if arguments.next().is_some() {
            usage(&program);
            ExitCode::from(2)
        } else {
            match parse_shard(&shard) {
                Ok(shard) => run_all(
                    &program,
                    &archive,
                    Some(shard),
                    target == "--all-list-shard",
                    target == "--all-list-differences-shard",
                ),
                Err(error) => {
                    eprintln!("{}: {error}", program.to_string_lossy());
                    ExitCode::from(2)
                }
            }
        };
    }
    let show_preview =
        matches!(arguments.next().as_deref(), Some(value) if value == "--show-preview");
    if arguments.next().is_some() {
        usage(&program);
        return ExitCode::from(2);
    }
    run_one(&program, &archive, &target.to_string_lossy(), show_preview)
}

fn usage(program: &OsStr) {
    eprintln!(
        "usage: {} <mandoc-1.14.6.tar.gz> [--all | --all-shard INDEX/COUNT | --all-list-shard INDEX/COUNT | --all-list-differences-shard INDEX/COUNT | case-id [--show-preview]]",
        program.to_string_lossy()
    );
}

fn run_all(
    program: &OsStr,
    archive: &Path,
    shard: Option<(usize, usize)>,
    list_equal: bool,
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
    let mut first = None;
    for (case_index, case) in inventory.cases.into_iter().enumerate() {
        if shard.is_some_and(|(shard_index, shard_count)| case_index % shard_count != shard_index) {
            continue;
        }
        let formats = available_renderer_formats(&case);
        match compare_case_outputs(archive, &case.id, &formats) {
            Ok(comparisons) => {
                for (format, comparison) in comparisons {
                    match comparison {
                        Ok(OutputComparison::Equal) => {
                            equal += 1;
                            if list_equal {
                                println!("renderer_equal_case={} format={format}", case.id);
                            }
                        }
                        Ok(OutputComparison::Different { offset }) => {
                            different += 1;
                            if list_differences {
                                println!(
                                    "renderer_difference_case={} format={format} offset={offset}",
                                    case.id
                                );
                            }
                            first.get_or_insert_with(|| {
                                (case.id.clone(), format, format!("byte:{offset}"))
                            });
                        }
                        Err(error) => {
                            errors += 1;
                            first.get_or_insert_with(|| (case.id.clone(), format, error));
                        }
                    }
                }
            }
            Err(error) => {
                errors += formats.len();
                first.get_or_insert_with(|| (case.id.clone(), "batch".into(), error));
            }
        }
    }
    if let Some((shard_index, shard_count)) = shard {
        println!("shard_index={shard_index}");
        println!("shard_count={shard_count}");
    }
    println!("renderer_output_count={}", equal + different + errors);
    println!("renderer_equal_output_count={equal}");
    println!("renderer_difference_output_count={different}");
    println!("renderer_error_output_count={errors}");
    if let Some((case, format, result)) = first {
        println!("first_difference_case={case}");
        println!("first_difference_format={format}");
        println!("first_difference={result}");
    }
    if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
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

fn run_one(program: &OsStr, archive: &Path, case: &str, show_preview: bool) -> ExitCode {
    let inventory = match stable_1_14_6_inventory(archive) {
        Ok(inventory) => inventory,
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            return ExitCode::from(1);
        }
    };
    let Some(case) = inventory
        .cases
        .iter()
        .find(|candidate| candidate.id.as_ref() == case)
    else {
        eprintln!(
            "{}: unknown stable regression case",
            program.to_string_lossy()
        );
        return ExitCode::from(1);
    };
    let formats = available_renderer_formats(case);
    if formats.is_empty() {
        println!("case_id={}", case.id);
        println!("renderer_output_count=0");
        return ExitCode::SUCCESS;
    }
    let comparisons = match compare_case_outputs(archive, &case.id, &formats) {
        Ok(comparisons) => comparisons,
        Err(error) => {
            eprintln!("{}: {}: {error}", program.to_string_lossy(), case.id);
            return ExitCode::from(1);
        }
    };
    let mut failed = false;
    for (format, comparison) in comparisons {
        match comparison {
            Ok(OutputComparison::Equal) => println!("{format}_equal=true"),
            Ok(OutputComparison::Different { offset }) => {
                println!("{format}_equal=false");
                println!("{format}_first_difference_byte={offset}");
            }
            Err(error) => {
                eprintln!(
                    "{}: {} {format}: {error}",
                    program.to_string_lossy(),
                    case.id
                );
                failed = true;
            }
        }
    }
    if show_preview {
        match render_previews(archive, &case.id, &formats) {
            Ok(previews) => {
                for (format, expected, actual, window) in previews {
                    println!("{format}_expected_preview={expected:?}");
                    println!("{format}_actual_preview={actual:?}");
                    println!("{format}_difference_window={window:?}");
                }
            }
            Err(error) => {
                eprintln!(
                    "{}: {} preview: {error}",
                    program.to_string_lossy(),
                    case.id
                );
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Return bounded escaped leading snippets for one direct M9 diagnosis.
///
/// This intentionally reloads the checksum-verified selected case only for
/// an explicit operator request. Bulk comparison retains its one-pass archive
/// extraction and never stores output text for differences.
fn render_previews(
    archive: &Path,
    case_id: &str,
    formats: &[&str],
) -> Result<Vec<RenderPreview>, String> {
    let payload = stable_1_14_6_renderer_case(archive, case_id, formats)
        .map_err(|error| error.to_string())?;
    let name = SourceName::new(payload.source.case.input_archive_path.as_ref())
        .map_err(|error| format!("invalid verified source name: {error}"))?;
    let source = Source::new(&name, &payload.source.source_bytes);
    payload
        .outputs
        .into_iter()
        .map(|expected| {
            let format = expected.output.format.clone();
            let html_fragment = format.as_ref() == "html";
            let renderer_format = render_format(&format)?;
            let actual = Renderer::new(renderer_format)
                .render(source)
                .map_err(|error| error.to_string())?;
            let actual = if html_fragment {
                extract_upstream_html_test_output(&actual.output)
            } else {
                actual.output
            };
            let window = difference_window(&expected.output_bytes, actual.as_bytes());
            Ok((
                format,
                preview(&String::from_utf8_lossy(&expected.output_bytes)),
                preview(&actual),
                window,
            ))
        })
        .collect()
}

/// Return a compact byte window around the first exact-output mismatch.
///
/// Keeping this diagnostic at the byte level makes it safe for terminal
/// overstrikes and deliberately malformed UTF-8 recovery fixtures alike.
fn difference_window(expected: &[u8], actual: &[u8]) -> String {
    if expected == actual {
        return "equal".to_owned();
    }
    let offset = expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    // Focused single-case diagnosis is an interactive path.  A wider bounded
    // context frequently captures the end of the affected layout block and
    // the following block boundary in one invocation, avoiding repeated
    // renderer launches for long list/table fixtures.
    let start = offset.saturating_sub(256);
    let end = expected
        .len()
        .max(actual.len())
        .min(offset.saturating_add(512));
    let expected =
        visible_difference_fragment(&expected[start.min(expected.len())..end.min(expected.len())]);
    let actual =
        visible_difference_fragment(&actual[start.min(actual.len())..end.min(actual.len())]);
    format!("byte:{offset} expected={expected:?} actual={actual:?}")
}

/// Make repeated spaces visible in a compact renderer-difference preview.
///
/// The renderer's exact gate remains byte based; this is only operator-facing
/// diagnostics for the common case where an otherwise identical terminal line
/// differs by one source-controlled separator.
fn visible_difference_fragment(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|character| if character == ' ' { '·' } else { character })
        .collect()
}

fn preview(value: &str) -> String {
    // Enough context to reach the first body-level layout distinction after
    // the common header and NAME section, while remaining safe to print for a
    // direct single-case operator diagnosis.
    const MAX_PREVIEW_CHARACTERS: usize = 2_048;
    let mut characters = value.chars();
    let preview = characters
        .by_ref()
        .take(MAX_PREVIEW_CHARACTERS)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn available_renderer_formats(case: &CorpusCase) -> Vec<&'static str> {
    RENDER_FORMATS
        .into_iter()
        .filter(|format| {
            case.expected_outputs
                .iter()
                .any(|output| output.format.as_ref() == *format)
        })
        .collect()
}

enum OutputComparison {
    Equal,
    Different { offset: usize },
}

type FormatComparison = (Box<str>, Result<OutputComparison, String>);
type RenderPreview = (Box<str>, String, String, String);

fn compare_case_outputs(
    archive: &Path,
    case_id: &str,
    formats: &[&str],
) -> Result<Vec<FormatComparison>, String> {
    let payload = stable_1_14_6_renderer_case(archive, case_id, formats)
        .map_err(|error| error.to_string())?;
    let name = SourceName::new(payload.source.case.input_archive_path.as_ref())
        .map_err(|error| format!("invalid verified source name: {error}"))?;
    let source = Source::new(&name, &payload.source.source_bytes);
    // Upstream's HTML regression harness renders a complete HTML document and
    // then extracts only text bracketed by BEGINTEST/ENDTEST or MathML inside
    // `<math class="eqn">`. Reproduce that documented test transform exactly;
    // fragment mode would compare a different renderer invocation.
    Ok(payload
        .outputs
        .into_iter()
        .map(|expected| {
            let html_fragment = expected.output.format.as_ref() == "html";
            let format = expected.output.format.clone();
            let comparison = render_format(&format).and_then(|format| {
                let actual = Renderer::new(format)
                    .render(source)
                    .map_err(|error| error.to_string())?;
                let actual = if html_fragment {
                    extract_upstream_html_test_output(&actual.output)
                } else {
                    actual.output
                };
                let actual = actual.as_bytes();
                if actual == expected.output_bytes {
                    return Ok(OutputComparison::Equal);
                }
                let offset = actual
                    .iter()
                    .zip(&expected.output_bytes)
                    .position(|(actual, expected)| actual != expected)
                    .unwrap_or_else(|| actual.len().min(expected.output_bytes.len()));
                Ok(OutputComparison::Different { offset })
            });
            (format, comparison)
        })
        .collect())
}

/// Apply the stable mandoc `regress.pl` HTML-output extractor without shelling
/// out to the Perl harness or accepting a looser renderer normalization.
fn extract_upstream_html_test_output(output: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Outside,
        Math,
        Other,
    }

    let mut state = State::Outside;
    let mut extracted = String::new();
    for raw_line in output.lines() {
        let mut line = raw_line;
        if matches!(state, State::Outside)
            && let Some(offset) = line.find("<math class=\"eqn\">")
        {
            line = &line[offset + "<math class=\"eqn\">".len()..];
            state = State::Math;
            if line.is_empty() {
                continue;
            }
        } else if line.contains("BEGINTEST") {
            state = State::Other;
            continue;
        } else if line.contains("ENDTEST") {
            state = State::Outside;
            continue;
        }
        if matches!(state, State::Math) {
            line = line.trim_start_matches(' ');
            if let Some(offset) = line.find("</math>") {
                let line = &line[..offset];
                if !line.is_empty() {
                    extracted.push_str(line);
                    extracted.push('\n');
                }
                state = State::Outside;
                continue;
            }
        }
        if matches!(state, State::Math | State::Other) {
            extracted.push_str(line);
            extracted.push('\n');
        }
    }
    extracted
}

fn render_format(format: &str) -> Result<RenderFormat, String> {
    match format {
        "ascii" => Ok(RenderFormat::Ascii),
        "utf8" => Ok(RenderFormat::Utf8),
        "html" => Ok(RenderFormat::Html),
        _ => Err(format!("unsupported native renderer format {format:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        available_renderer_formats, difference_window, extract_upstream_html_test_output,
        render_format, visible_difference_fragment,
    };
    use crate::conformance::CorpusCase;

    #[test]
    fn only_compares_native_renderer_formats() {
        let case = CorpusCase {
            id: "fixture".into(),
            input_archive_path: "fixture.in".into(),
            source_sha256: "hash".into(),
            expected_outputs: vec![
                crate::conformance::ReferenceOutput {
                    format: "ascii".into(),
                    archive_path: "fixture.out_ascii".into(),
                    sha256: "hash".into(),
                },
                crate::conformance::ReferenceOutput {
                    format: "lint".into(),
                    archive_path: "fixture.out_lint".into(),
                    sha256: "hash".into(),
                },
            ],
        };
        assert_eq!(available_renderer_formats(&case), ["ascii"]);
        assert!(render_format("markdown").is_err());
    }

    #[test]
    fn mirrors_the_upstream_html_regression_extractor() {
        let output = concat!(
            "<html>ignored\n",
            "BEGINTEST\n",
            "text &amp; value\n",
            "ENDTEST\n",
            "<math class=\"eqn\">\n",
            "  <mi>x</mi>\n",
            "</math>ignored\n",
            "</html>\n",
        );
        assert_eq!(
            extract_upstream_html_test_output(output),
            "text &amp; value\n<mi>x</mi>\n"
        );
    }

    #[test]
    fn difference_window_reports_the_first_byte_with_local_context() {
        assert_eq!(
            difference_window(b"prefix expected suffix", b"prefix actual suffix"),
            r#"byte:7 expected="prefix·expected·suffix" actual="prefix·actual·suffix""#
        );
    }

    #[test]
    fn difference_preview_marks_spaces_without_changing_bytes() {
        assert_eq!(visible_difference_fragment(b"one  two"), "one··two");
    }
}
