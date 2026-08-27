//! Create or verify the native canonical regression snapshot used after the
//! temporary C oracle is retired.

use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use mantdoc::{Parser, ParserConfig, Source, SourceName};
use mantdoc_conformance::{
    CANONICAL_MDOC_OPERATING_SYSTEM, canonicalize_mantdoc, stable_1_14_6_case,
    stable_1_14_6_inventory,
};
use sha2::{Digest, Sha256};

const SNAPSHOT_SCHEMA: &str = "mantdoc.native-canonical-snapshot/v1";

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let Some(archive) = arguments.next().map(PathBuf::from) else {
        usage(&program);
        return ExitCode::from(2);
    };
    let Some(mode) = arguments.next() else {
        usage(&program);
        return ExitCode::from(2);
    };
    let Some(snapshot) = arguments.next().map(PathBuf::from) else {
        usage(&program);
        return ExitCode::from(2);
    };
    if arguments.next().is_some() {
        usage(&program);
        return ExitCode::from(2);
    }

    let records = match snapshot_records(&archive) {
        Ok(records) => records,
        Err(error) => {
            eprintln!("{}: {error}", program.to_string_lossy());
            return ExitCode::from(1);
        }
    };
    match mode.to_string_lossy().as_ref() {
        "--write" => match write_snapshot(&snapshot, &records) {
            Ok(()) => success(&snapshot, &records),
            Err(error) => {
                eprintln!("{}: {error}", program.to_string_lossy());
                ExitCode::from(1)
            }
        },
        "--verify" => match verify_snapshot(&snapshot, &records) {
            Ok(()) => success(&snapshot, &records),
            Err(error) => {
                eprintln!("{}: {error}", program.to_string_lossy());
                ExitCode::from(1)
            }
        },
        _ => {
            usage(&program);
            ExitCode::from(2)
        }
    }
}

fn usage(program: &OsStr) {
    eprintln!(
        "usage: {} <mandoc-1.14.6.tar.gz> --write|--verify <snapshot-path>",
        program.to_string_lossy()
    );
}

fn success(snapshot: &Path, records: &[String]) -> ExitCode {
    println!("snapshot={}", snapshot.display());
    println!("case_count={}", records.len());
    println!("records_sha256={}", records_sha256(records));
    ExitCode::SUCCESS
}

fn snapshot_records(archive: &Path) -> Result<Vec<String>, String> {
    let inventory = stable_1_14_6_inventory(archive).map_err(|error| error.to_string())?;
    let parser = Parser::new(ParserConfig {
        operating_system: Some(CANONICAL_MDOC_OPERATING_SYSTEM.into()),
        ..ParserConfig::default()
    });
    let mut records = Vec::with_capacity(inventory.cases.len());
    for case in inventory.cases {
        let payload = stable_1_14_6_case(archive, &case.id).map_err(|error| error.to_string())?;
        let name =
            SourceName::new(&payload.case.input_archive_path).map_err(|error| error.to_string())?;
        let report = parser
            .parse(Source::new(&name, &payload.source_bytes))
            .map_err(|error| format!("{}: {error}", payload.case.id))?;
        let canonical = canonicalize_mantdoc(&report);
        let canonical = serde_json::to_vec(&canonical)
            .map_err(|error| format!("{}: {error}", payload.case.id))?;
        records.push(format!(
            "{}\t{}\t{}",
            payload.case.id,
            payload.case.source_sha256,
            sha256_hex(&canonical)
        ));
    }
    Ok(records)
}

fn write_snapshot(path: &Path, records: &[String]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| format!("snapshot path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, snapshot_contents(records))
        .map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

fn verify_snapshot(path: &Path, records: &[String]) -> Result<(), String> {
    let expected =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let actual = snapshot_contents(records);
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "native canonical snapshot differs: {} (expected {}, actual {})",
            path.display(),
            sha256_hex(expected.as_bytes()),
            sha256_hex(actual.as_bytes())
        ))
    }
}

fn snapshot_contents(records: &[String]) -> String {
    let mut contents = format!(
        "schema={SNAPSHOT_SCHEMA}\ncorpus_id=mandoc-stable-1.14.6\noracle_id={}\ncanonical_mdoc_os={CANONICAL_MDOC_OPERATING_SYSTEM}\ncase_count={}\nrecords_sha256={}\n\n",
        mantdoc::LEGACY_ORACLE_ID,
        records.len(),
        records_sha256(records)
    );
    for record in records {
        contents.push_str(record);
        contents.push('\n');
    }
    contents
}

fn records_sha256(records: &[String]) -> String {
    let mut hasher = Sha256::new();
    for record in records {
        hasher.update(record.as_bytes());
        hasher.update(b"\n");
    }
    hex_encode(&hasher.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    text
}
