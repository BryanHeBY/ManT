//! Independent two-source origin controls; the 45-case handoff matrix stays sealed.
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
    pub source: String,
    source_sha256: String,
    pub native_rows: Vec<String>,
    pub native_styles: Vec<Vec<u8>>,
    profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
struct Profile {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
}

impl Case {
    pub fn portable_rows(&self) -> Vec<&'static str> {
        match self.id.as_str() {
            "one-leading-row" => vec!["probe int", " leading"],
            "two-leading-rows" => vec!["probe int", " leading", " second"],
            _ => panic!("unknown frozen source control"),
        }
    }
}

pub fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!("cases.json")).unwrap();
    assert_eq!(fixture.case_count, 2);
    assert_eq!(fixture.profile_count, 14);
    assert_eq!(fixture.cases.len(), fixture.case_count);
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    for case in &fixture.cases {
        assert_eq!(case.source_sha256.len(), 64);
        assert_eq!(case.native_rows.len(), case.native_styles.len());
        for (row, styles) in case.native_rows.iter().zip(&case.native_styles) {
            assert_eq!(row.chars().count(), styles.len());
        }
        assert_eq!(case.profiles.len(), 7);
        for receipt in case.profiles.values() {
            assert_eq!(receipt.status, 0, "{}", case.id);
            assert_eq!(receipt.stdout_sha256.len(), 64);
            assert_eq!(receipt.stderr_sha256.len(), 64);
        }
    }
    fixture.cases
}
