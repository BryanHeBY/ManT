//! Verify native AST destination tags against exact upstream mandoc tag data.

use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::ExitCode,
};

use mantdoc::{Parser, Source, SourceName};

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
    let mut first = None;
    for (case_index, case) in inventory.cases.iter().enumerate() {
        if shard.is_some_and(|(index, count)| case_index % count != index) || !has_tag_output(case)
        {
            continue;
        }
        match compare_case(archive, case) {
            Ok(Comparison::Equal) => equal += 1,
            Ok(Comparison::Different { detail }) => {
                different += 1;
                if list_differences {
                    println!("tag_difference_case={} {detail}", case.id);
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
    println!("tag_output_count={}", equal + different + errors);
    println!("tag_equal_output_count={equal}");
    println!("tag_difference_output_count={different}");
    println!("tag_error_output_count={errors}");
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

fn has_tag_output(case: &CorpusCase) -> bool {
    case.expected_outputs
        .iter()
        .any(|output| output.format.as_ref() == "tag")
}

enum Comparison {
    Equal,
    Different { detail: String },
}

fn compare_case(archive: &Path, case: &CorpusCase) -> Result<Comparison, String> {
    let payload = stable_1_14_6_renderer_case(archive, &case.id, &["tag"])
        .map_err(|error| error.to_string())?;
    let expected = payload
        .outputs
        .into_iter()
        .next()
        .ok_or_else(|| "verified tag payload did not retain an output".to_owned())?
        .output_bytes;
    let expected = parse_expected_tags(&expected)?;
    let source_name = SourceName::new(&payload.source.case.input_archive_path)
        .map_err(|error| format!("invalid verified source name: {error}"))?;
    let report = Parser::default()
        .parse(Source::new(&source_name, &payload.source.source_bytes))
        .map_err(|error| error.to_string())?;
    let mut actual = report
        .document
        .preorder()
        .filter_map(|node| {
            let tag = node.tag()?;
            let location = node.location()?;
            let position = report.document.source_position(location)?;
            Some((tag.to_owned(), position.line))
        })
        .collect::<Vec<_>>();
    actual.sort_unstable();
    if actual == expected {
        Ok(Comparison::Equal)
    } else {
        Ok(Comparison::Different {
            detail: difference_detail(&expected, &actual),
        })
    }
}

fn parse_expected_tags(bytes: &[u8]) -> Result<Vec<(String, u32)>, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("tag output is not UTF-8: {error}"))?;
    let mut tags = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        let Some(tag) = fields.next() else {
            continue;
        };
        let Some(_source) = fields.next() else {
            return Err(format!("tag output has no source field: {line:?}"));
        };
        let Some(line_number) = fields.next() else {
            return Err(format!("tag output has no line field: {line:?}"));
        };
        if fields.next().is_some() {
            return Err(format!("tag output has extra fields: {line:?}"));
        }
        let line_number = line_number.parse::<u32>().map_err(|error| {
            format!("tag output line is not an unsigned integer: {line:?}: {error}")
        })?;
        tags.push((tag.to_owned(), line_number));
    }
    tags.sort_unstable();
    Ok(tags)
}

fn difference_detail(expected: &[(String, u32)], actual: &[(String, u32)]) -> String {
    let offset = expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    format!(
        "entry:{offset} expected_entry={:?} actual_entry={:?} expected_tags={expected:?} actual_tags={actual:?}",
        expected.get(offset),
        actual.get(offset),
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
    use super::parse_expected_tags;

    #[test]
    fn tag_parser_uses_destination_and_source_line_only() {
        assert_eq!(
            parse_expected_tags(
                b"DESCRIPTION fixture.mandoc_ascii 8\nNAME fixture.mandoc_ascii 3\n"
            )
            .unwrap(),
            [("DESCRIPTION".to_owned(), 8), ("NAME".to_owned(), 3),]
        );
    }
}
