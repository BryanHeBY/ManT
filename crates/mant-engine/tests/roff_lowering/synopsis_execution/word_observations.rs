//! Source-bound corrections retain the original immutable oracle receipts.
use std::collections::BTreeMap;

use serde::Deserialize;

use super::{Case, PortableOwner};

#[derive(Debug, Deserialize, Eq, PartialEq)]
pub(super) struct Profile {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
    stderr: String,
}

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    original_fixture_sha256: String,
    cases: Vec<Observation>,
}

#[derive(Deserialize)]
struct Observation {
    id: String,
    source: String,
    source_sha256: String,
    original_profiles: BTreeMap<String, Profile>,
    profiles: BTreeMap<String, Profile>,
    native_rows: Vec<String>,
    reading_rows: Vec<String>,
    portable_rows: Vec<String>,
    portable_owners: Vec<PortableOwner>,
}

pub(super) fn apply(cases: &mut [Case]) {
    let fixture: Fixture = serde_json::from_str(include_str!("word_observations.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.original_fixture_sha256.len(), 64);
    assert_eq!(fixture.cases.len(), 20);
    let mut applied = std::collections::BTreeSet::new();
    for observation in fixture.cases {
        assert!(applied.insert(observation.id.clone()));
        let case = cases
            .iter_mut()
            .find(|case| case.id == observation.id)
            .unwrap();
        assert_eq!(
            case.source, observation.source,
            "{}: original source",
            case.id
        );
        assert_eq!(
            case.source_sha256, observation.source_sha256,
            "{}: source hash",
            case.id
        );
        assert_eq!(
            case.oracle, observation.original_profiles,
            "{}: unchanged old receipts",
            case.id
        );
        assert_eq!(observation.profiles.len(), 5);
        for (name, receipt) in &observation.profiles {
            assert_eq!(receipt.status, case.oracle[name].status);
            if name != "lint" {
                assert_eq!(receipt, &case.oracle[name]);
            }
        }
        case.rows = observation.native_rows;
        case.reading_rows = observation.reading_rows;
        case.portable_rows = observation.portable_rows;
        case.portable_owners = observation.portable_owners;
        case.exact_word_observation = true;
    }
}
