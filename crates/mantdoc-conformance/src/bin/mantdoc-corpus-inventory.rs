//! Print the verified stable upstream corpus inventory without writing payload.

use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::ExitCode,
};

use mantdoc_conformance::{
    MantdocBackend, inspect_m3_execution_reports, run_case, run_m3_execution_gate,
    run_m4_man_smoke_gate, run_m5_mdoc_smoke_gate, run_m5_mdoc_smoke_shard,
    run_m6_preprocess_smoke_gate, stable_1_14_6_case, stable_1_14_6_case_input,
    stable_1_14_6_inventory,
};

#[derive(Clone, Copy)]
enum CaseMode {
    Inspect,
    Parse,
}

#[allow(clippy::too_many_lines)] // CLI dispatch mirrors the documented, stable command surface.
fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let Some(archive_path) = arguments.next().map(PathBuf::from) else {
        print_usage(&program);
        return ExitCode::from(2);
    };
    let Some(case_id) = arguments.next() else {
        return run_inventory(&program, &archive_path);
    };
    if case_id == "--m3-execution" {
        return if arguments.next().is_some() {
            print_usage(&program);
            ExitCode::from(2)
        } else {
            run_m3_execution(&program, &archive_path)
        };
    }
    if case_id == "--m3-execution-report" {
        return if arguments.next().is_some() {
            print_usage(&program);
            ExitCode::from(2)
        } else {
            run_m3_execution_report(&program, &archive_path)
        };
    }
    if case_id == "--m4-man-smoke" {
        return if arguments.next().is_some() {
            print_usage(&program);
            ExitCode::from(2)
        } else {
            run_m4_man_smoke(&program, &archive_path)
        };
    }
    if case_id == "--m5-mdoc-smoke" {
        return if arguments.next().is_some() {
            print_usage(&program);
            ExitCode::from(2)
        } else {
            run_m5_mdoc_smoke(&program, &archive_path)
        };
    }
    if case_id == "--m5-mdoc-smoke-shard" {
        let Some(shard) = arguments.next() else {
            print_usage(&program);
            return ExitCode::from(2);
        };
        return if arguments.next().is_some() {
            print_usage(&program);
            ExitCode::from(2)
        } else {
            run_m5_mdoc_smoke_shard_command(&program, &archive_path, &shard)
        };
    }
    if case_id == "--m6-preprocess-smoke" {
        return if arguments.next().is_some() {
            print_usage(&program);
            ExitCode::from(2)
        } else {
            run_m6_preprocess_smoke(&program, &archive_path)
        };
    }
    let mode = match arguments.next() {
        None => CaseMode::Inspect,
        Some(argument) if argument == "--parse" => CaseMode::Parse,
        Some(_) => {
            print_usage(&program);
            return ExitCode::from(2);
        }
    };
    if arguments.next().is_some() {
        print_usage(&program);
        return ExitCode::from(2);
    }
    run_case_command(&program, &archive_path, &case_id, mode)
}

fn run_inventory(program: &OsStr, archive_path: &Path) -> ExitCode {
    match stable_1_14_6_inventory(archive_path) {
        Ok(inventory) => {
            println!("corpus_id={}", inventory.corpus_id);
            println!("archive_sha256={}", inventory.archive_sha256);
            println!("input_count={}", inventory.cases.len());
            println!("expected_output_count={}", inventory.expected_output_count);
            println!("case_set_sha256={}", inventory.case_set_sha256);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            ExitCode::from(1)
        }
    }
}

fn run_m3_execution(program: &OsStr, archive_path: &Path) -> ExitCode {
    match run_m3_execution_gate(archive_path) {
        Ok(results) => {
            for result in results {
                println!("case_id={}", result.case_id);
                println!("source_sha256={}", result.source_sha256);
                println!("ast_nodes={}", result.ast_nodes);
                println!("diagnostic_count={}", result.diagnostic_count);
                println!("expansion_steps={}", result.expansion_steps);
                println!("truncated={}", result.truncated);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            ExitCode::from(1)
        }
    }
}

fn run_m3_execution_report(program: &OsStr, archive_path: &Path) -> ExitCode {
    match inspect_m3_execution_reports(archive_path) {
        Ok(results) => {
            for result in results {
                println!("case_id={}", result.case_id);
                println!("source_sha256={}", result.source_sha256);
                println!("ast_nodes={}", result.ast_nodes);
                println!("diagnostic_count={}", result.diagnostic_count);
                println!("expansion_steps={}", result.expansion_steps);
                println!("truncated={}", result.truncated);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            ExitCode::from(1)
        }
    }
}

fn run_m4_man_smoke(program: &OsStr, archive_path: &Path) -> ExitCode {
    match run_m4_man_smoke_gate(archive_path) {
        Ok(result) => {
            println!("case_count={}", result.case_count);
            println!("diagnostic_case_count={}", result.diagnostic_case_count);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            ExitCode::from(1)
        }
    }
}

fn run_m5_mdoc_smoke(program: &OsStr, archive_path: &Path) -> ExitCode {
    match run_m5_mdoc_smoke_gate(archive_path) {
        Ok(result) => {
            println!("case_count={}", result.case_count);
            println!("diagnostic_case_count={}", result.diagnostic_case_count);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            ExitCode::from(1)
        }
    }
}

fn run_m5_mdoc_smoke_shard_command(
    program: &OsStr,
    archive_path: &Path,
    shard: &OsStr,
) -> ExitCode {
    let (shard_index, shard_count) = match parse_shard(shard) {
        Ok(shard) => shard,
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            return ExitCode::from(2);
        }
    };
    match run_m5_mdoc_smoke_shard(archive_path, shard_index, shard_count) {
        Ok(result) => {
            println!("shard_index={shard_index}");
            println!("shard_count={shard_count}");
            println!("case_count={}", result.case_count);
            println!("diagnostic_case_count={}", result.diagnostic_case_count);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            ExitCode::from(1)
        }
    }
}

fn run_m6_preprocess_smoke(program: &OsStr, archive_path: &Path) -> ExitCode {
    match run_m6_preprocess_smoke_gate(archive_path) {
        Ok(result) => {
            println!("case_count={}", result.case_count);
            println!("diagnostic_case_count={}", result.diagnostic_case_count);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            ExitCode::from(1)
        }
    }
}

fn run_case_command(
    program: &OsStr,
    archive_path: &Path,
    case_id: &OsStr,
    mode: CaseMode,
) -> ExitCode {
    let case_id = case_id.to_string_lossy();
    let payload = match stable_1_14_6_case(archive_path, &case_id) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            return ExitCode::from(1);
        }
    };
    println!("case_id={}", payload.case.id);
    println!("input_archive_path={}", payload.case.input_archive_path);
    println!("source_sha256={}", payload.case.source_sha256);
    println!("source_bytes={}", payload.source_bytes.len());
    println!(
        "expected_output_count={}",
        payload.case.expected_outputs.len()
    );
    match mode {
        CaseMode::Inspect => ExitCode::SUCCESS,
        CaseMode::Parse => run_parse_command(program, archive_path, &case_id),
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

fn run_parse_command(program: &OsStr, archive_path: &Path, case_id: &str) -> ExitCode {
    let backend = MantdocBackend::default();
    let input = match stable_1_14_6_case_input(archive_path, case_id, backend.parser_config()) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            return ExitCode::from(1);
        }
    };
    let run = match run_case(&backend, &input) {
        Ok(run) => run,
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            return ExitCode::from(1);
        }
    };
    let report = match run.outcome {
        Ok(report) => report,
        Err(error) => {
            eprintln!("{}: {error:?}", program.to_string_lossy());
            return ExitCode::from(1);
        }
    };
    println!("backend={}", run.backend);
    println!("ast_nodes={}", report.document.node_count());
    println!("diagnostic_count={}", report.diagnostics.len());
    println!("expansion_steps={}", report.statistics.expansion_steps);
    println!("truncated={}", report.statistics.truncated);
    for finding in report.diagnostics {
        println!("diagnostic={}", finding.code);
        println!("diagnostic_level={:?}", finding.severity);
        println!("diagnostic_message={}", finding.message);
        if let Some(span) = finding.primary {
            println!("diagnostic_span={}-{}", span.start, span.end);
        }
    }
    ExitCode::SUCCESS
}

fn print_usage(program: &OsStr) {
    eprintln!(
        "usage: {} <mandoc-1.14.6.tar.gz> [--m3-execution | --m3-execution-report | --m4-man-smoke | --m5-mdoc-smoke | --m5-mdoc-smoke-shard INDEX/COUNT | --m6-preprocess-smoke | case-id [--parse]]",
        program.to_string_lossy()
    );
}
