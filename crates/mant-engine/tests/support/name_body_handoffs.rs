//! Source-bound ordinary It/Nm receipts, shared by engine and painted UI tests.
use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    case_count: usize,
    profile_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
pub struct Case {
    pub id: String,
    pub cohort: String,
    pub context: String,
    pub source: String,
    pub oracle: Oracle,
}

#[derive(Deserialize)]
pub struct Oracle {
    source_sha256: String,
    pub rows: Vec<String>,
    pub styles: Vec<Vec<u8>>,
    profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
struct Profile {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
    diagnostics: String,
}

pub fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../roff_lowering/name_body_handoffs/cases.json"
    ))
    .unwrap();
    assert_eq!(fixture.case_count, 45);
    assert_eq!(fixture.cases.len(), fixture.case_count);
    assert_eq!(fixture.profile_count, fixture.case_count * 5);
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    for case in &fixture.cases {
        assert_eq!(case.oracle.source_sha256.len(), 64);
        assert_eq!(case.oracle.rows.len(), case.oracle.styles.len());
        for (row, styles) in case.oracle.rows.iter().zip(&case.oracle.styles) {
            assert_eq!(row.chars().count(), styles.len());
        }
        assert_eq!(case.oracle.profiles.len(), 5);
        for (profile, receipt) in &case.oracle.profiles {
            let expected = i32::from(case.context == "table" && profile == "lint") * 4;
            assert_eq!(receipt.status, expected, "{}/{profile}", case.id);
            assert_eq!(receipt.stdout_sha256.len(), 64);
            assert_eq!(receipt.stderr_sha256.len(), 64);
            if expected == 0 {
                assert_eq!(receipt.diagnostics, "", "{}/{profile}", case.id);
            }
        }
    }
    fixture.cases
}
