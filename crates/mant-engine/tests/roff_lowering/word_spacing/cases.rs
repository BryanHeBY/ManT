//! Frozen pristine observations, shared by independent engine and UI consumers.
use std::collections::BTreeMap;

use serde::Deserialize;

use crate::oracle_receipt::{OracleReceipt, assert_receipts};

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    core_count: usize,
    control_count: usize,
    risk_count: usize,
    flush_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
pub(super) struct Case {
    pub(super) id: String,
    pub(super) cohort: String,
    pub(super) source: String,
    source_sha256: String,
    pub(super) rows: Vec<String>,
    profiles: BTreeMap<String, Profile>,
}

impl Case {
    pub(super) fn markdown_rows(&self) -> Vec<String> {
        let mut rows = self.rows.clone();
        if self.cohort == "flush" {
            // Frozen baseline exports the pretty declaration and following
            // paragraph as two owners. Keep that existing paragraph boundary,
            // while the native oracle independently supplies every word cell.
            rows.insert(2, String::new());
        }
        rows
    }
}

#[derive(Deserialize)]
struct Profile {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
    diagnostics: String,
}

pub(super) fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!("cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(
        (
            fixture.core_count,
            fixture.control_count,
            fixture.risk_count,
            fixture.flush_count
        ),
        (30, 18, 44, 18)
    );
    assert_eq!(fixture.cases.len(), 110);
    for case in &fixture.cases {
        assert_receipts(
            &case.id,
            &case.source_sha256,
            case.profiles.iter().map(|(profile, run)| {
                (
                    profile.as_str(),
                    OracleReceipt {
                        status: run.status,
                        stdout_sha256: &run.stdout_sha256,
                        stderr_sha256: Some(&run.stderr_sha256),
                        diagnostics: &run.diagnostics,
                    },
                )
            }),
        );
    }
    fixture.cases
}
