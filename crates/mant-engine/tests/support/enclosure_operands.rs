//! Shared, independently recorded native operand fixtures for real consumers.
use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    generated_count: usize,
    unique_source_count: usize,
    profile_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
pub struct Case {
    pub id: String,
    pub cohort: String,
    pub context: String,
    pub no_fill: bool,
    pub source: String,
    pub source_sha256: String,
    pub rows: Vec<String>,
    pub styles: Vec<Vec<u8>>,
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
pub struct Profile {
    pub status: i32,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub diagnostics: String,
}

pub fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../roff_lowering/enclosure_operands/cases.json"
    ))
    .unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.generated_count, 192);
    assert_eq!(fixture.unique_source_count, 185);
    assert_eq!(fixture.profile_count, 960);
    assert_eq!(fixture.cases.len(), fixture.generated_count);
    for case in &fixture.cases {
        assert_eq!(case.no_fill, case.source.contains(".nf\n"));
        assert_eq!(case.source_sha256.len(), 64);
        assert_eq!(case.profiles.len(), 5);
        assert_eq!(case.styles.len(), case.rows.len());
        for (row, style) in case.rows.iter().zip(&case.styles) {
            assert_eq!(row.chars().count(), style.len());
        }
        for profile in case.profiles.values() {
            assert_eq!(profile.stdout_sha256.len(), 64);
            assert_eq!(profile.stderr_sha256.len(), 64);
        }
        for (name, profile) in &case.profiles {
            if name == "lint" {
                assert!(matches!(profile.status, 0 | 2));
                if case.source.contains(".Es") || case.source.contains(".En") {
                    assert_eq!(profile.status, 2);
                    assert!(profile.diagnostics.contains("obsolete macro"));
                }
            } else {
                assert_eq!(profile.status, 0);
            }
        }
    }
    assert_eq!(
        fixture
            .cases
            .iter()
            .filter(|case| case.cohort == "cursor-core")
            .count(),
        64
    );
    fixture.cases
}

/// Portable Markdown preserves separate prose/list owners, with its required
/// paragraph gaps, and replaces the native six-column definition field with
/// ordinary list structure. These are structural projection rules, not word
/// trimming: an operand's own leading SP and every authored hard row remain.
pub fn portable_rows(case: &Case) -> Vec<String> {
    let mut rows: Vec<_> = case
        .rows
        .iter()
        .map(|row| {
            if case.context == "tag-body" {
                row.strip_prefix("TAG   ").map_or_else(
                    || row.strip_prefix("      ").unwrap_or(row).to_owned(),
                    |body| format!("TAG {body}"),
                )
            } else if case.context == "tag-head" {
                row.strip_prefix("      ").unwrap_or(row).to_owned()
            } else {
                row.clone()
            }
        })
        .collect();
    if matches!(case.context.as_str(), "tag-head" | "tag-body" | "display") {
        // The final TAIL belongs to the original outer paragraph, outside
        // this independent display/list. CommonMark must close that block.
        let tail = rows.len() - 1;
        assert_eq!(rows[tail], "TAIL");
        rows.insert(tail, String::new());
        if case.context == "display" {
            // A filled display is another complete Paragraph owner, not a
            // merged inline root; Markdown cannot retain compact zero gap.
            assert_eq!(rows[0], "ORIGIN");
            rows.insert(1, String::new());
        }
    }
    rows
}
