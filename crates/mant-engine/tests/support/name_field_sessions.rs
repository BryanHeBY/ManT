//! Complete native sessions shared by the CLI and painted UI assertions.
use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    core_count: usize,
    risk_count: usize,
    nested_count: usize,
    cell_control_count: usize,
    profile_count: usize,
    cases: Vec<Case>,
    cell_controls: Vec<CellControl>,
}

#[derive(Deserialize)]
pub struct Case {
    pub id: String,
    pub cohort: String,
    pub source: String,
    pub independent_source: Option<String>,
    pub whole: Oracle,
    pub independent: Option<Oracle>,
}

#[derive(Deserialize)]
pub struct CellControl {
    pub id: String,
    pub source: String,
    pub adjacent: bool,
    whole: Oracle,
    pub independent: Vec<Oracle>,
}

impl Case {
    pub fn expected(&self) -> &Oracle {
        self.independent.as_ref().unwrap_or(&self.whole)
    }
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

fn fixture() -> Fixture {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../roff_lowering/name_field_sessions/cases.json"
    ))
    .unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(
        (fixture.core_count, fixture.risk_count, fixture.nested_count),
        (25, 20, 11)
    );
    assert_eq!(fixture.cell_control_count, 4);
    assert_eq!(fixture.cell_controls.len(), 4);
    assert_eq!(fixture.profile_count, 565);
    assert_eq!(fixture.cases.len(), 56);
    for case in &fixture.cases {
        assert_eq!(
            case.independent_source.is_some(),
            case.independent.is_some()
        );
        for (role, oracle) in [
            ("whole", Some(&case.whole)),
            ("fragment", case.independent.as_ref()),
        ] {
            let Some(oracle) = oracle else { continue };
            validate_oracle(&case.id, role, oracle, case.independent.is_some());
        }
    }
    for case in &fixture.cell_controls {
        validate_oracle(&case.id, "whole", &case.whole, true);
        assert_eq!(case.independent.len(), 2);
        for oracle in &case.independent {
            validate_oracle(&case.id, "fragment", oracle, false);
        }
    }
    fixture
}

fn validate_oracle(id: &str, role: &str, oracle: &Oracle, unsupported: bool) {
    assert_eq!(oracle.source_sha256.len(), 64);
    assert_eq!(oracle.rows.len(), oracle.styles.len());
    for (row, style) in oracle.rows.iter().zip(&oracle.styles) {
        assert_eq!(row.chars().count(), style.len());
    }
    assert_eq!(oracle.profiles.len(), 5);
    for (profile, run) in &oracle.profiles {
        assert_eq!(run.stdout_sha256.len(), 64);
        assert_eq!(run.stderr_sha256.len(), 64);
        let expected = match (id, role, profile.as_str()) {
            ("risk-prefix-Fn", "fragment", "html") => -6,
            (_, "whole", "lint") if unsupported => 4,
            _ => 0,
        };
        assert_eq!(run.status, expected, "{id}/{role}/{profile}");
        if expected != 0 {
            assert_ne!(run.diagnostics, "");
        }
    }
}

pub fn cases() -> Vec<Case> {
    fixture().cases
}

pub fn cell_controls() -> Vec<CellControl> {
    fixture().cell_controls
}
