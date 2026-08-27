//! Validation for the checked-in upstream conformance capability matrix.

use std::collections::BTreeSet;

use serde::Deserialize;

const UPSTREAM_COVERAGE_MANIFEST: &str = include_str!("data/upstream-1.14.6-coverage.toml");
const EXPECTED_INPUT_COUNT: usize = 572;
const EXPECTED_OUTPUT_COUNT: usize = 1_189;

#[derive(Debug, Deserialize)]
struct CoverageManifest {
    schema: String,
    corpus_id: String,
    input_family: Vec<InputFamily>,
    output_format: Vec<OutputFormat>,
}

#[derive(Debug, Deserialize)]
struct InputFamily {
    name: String,
    count: usize,
    coverage: String,
    additional_gate: String,
}

#[derive(Debug, Deserialize)]
struct OutputFormat {
    name: String,
    count: usize,
    classification: String,
    gate: String,
    comparison: String,
}

fn parse_manifest() -> Result<CoverageManifest, String> {
    let manifest: CoverageManifest = toml::from_str(UPSTREAM_COVERAGE_MANIFEST)
        .map_err(|error| format!("invalid upstream coverage matrix: {error}"))?;
    if manifest.schema != "mantdoc.upstream-coverage/v1" {
        return Err(format!(
            "unexpected upstream coverage schema: {}",
            manifest.schema
        ));
    }
    if manifest.corpus_id != "mandoc-stable-1.14.6" {
        return Err(format!(
            "unexpected upstream coverage corpus: {}",
            manifest.corpus_id
        ));
    }
    validate_input_families(&manifest.input_family)?;
    validate_output_formats(&manifest.output_format)?;
    Ok(manifest)
}

fn validate_input_families(families: &[InputFamily]) -> Result<(), String> {
    let expected = BTreeSet::from(["char", "eqn", "man", "mdoc", "roff", "tbl"]);
    let observed = families
        .iter()
        .map(|family| family.name.as_str())
        .collect::<BTreeSet<_>>();
    if observed != expected || families.len() != expected.len() {
        return Err(format!("unexpected upstream input families: {observed:?}"));
    }
    if families.iter().map(|family| family.count).sum::<usize>() != EXPECTED_INPUT_COUNT {
        return Err(format!(
            "upstream input-family count must total {EXPECTED_INPUT_COUNT}"
        ));
    }
    if families
        .iter()
        .any(|family| family.coverage.is_empty() || family.additional_gate.is_empty())
    {
        return Err("every upstream input family needs coverage and an additional gate".into());
    }
    Ok(())
}

fn validate_output_formats(formats: &[OutputFormat]) -> Result<(), String> {
    let expected = BTreeSet::from(["ascii", "html", "lint", "markdown", "tag", "utf8"]);
    let observed = formats
        .iter()
        .map(|format| format.name.as_str())
        .collect::<BTreeSet<_>>();
    if observed != expected || formats.len() != expected.len() {
        return Err(format!("unexpected upstream output formats: {observed:?}"));
    }
    if formats.iter().map(|format| format.count).sum::<usize>() != EXPECTED_OUTPUT_COUNT {
        return Err(format!(
            "upstream output-format count must total {EXPECTED_OUTPUT_COUNT}"
        ));
    }
    let strict = formats
        .iter()
        .filter(|format| format.classification == "strict")
        .map(|format| format.name.as_str())
        .collect::<BTreeSet<_>>();
    if strict != BTreeSet::from(["ascii", "html", "lint", "utf8"]) {
        return Err(format!("unexpected strict output formats: {strict:?}"));
    }
    if formats.iter().any(|format| {
        !matches!(format.classification.as_str(), "strict" | "classified")
            || format.gate.is_empty()
            || format.comparison.is_empty()
    }) {
        return Err(
            "every upstream output needs a valid classification, gate, and comparison".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EXPECTED_INPUT_COUNT, EXPECTED_OUTPUT_COUNT, parse_manifest};

    #[test]
    fn upstream_coverage_matrix_is_complete_and_unambiguous() {
        let manifest = parse_manifest().expect("checked-in upstream coverage matrix");
        assert_eq!(
            manifest
                .input_family
                .iter()
                .map(|family| family.count)
                .sum::<usize>(),
            EXPECTED_INPUT_COUNT
        );
        assert_eq!(
            manifest
                .output_format
                .iter()
                .map(|format| format.count)
                .sum::<usize>(),
            EXPECTED_OUTPUT_COUNT
        );
    }
}
