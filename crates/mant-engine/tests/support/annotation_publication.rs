//! Shared source records for the annotation publication consumer matrix.
use serde::Deserialize;

const CASES: &str = include_str!("../roff_lowering/annotation_publication/cases.jsonl");

#[derive(Deserialize)]
pub struct Case<'a> {
    pub id: &'a str,
    pub family: &'a str,
    pub axes: Vec<&'a str>,
    pub source: String,
    pub queries: Vec<&'a str>,
}

/// Iterate over the frozen source records in fixture order.
///
/// # Panics
///
/// Panics when a fixture record is malformed.
pub fn cases() -> impl Iterator<Item = Case<'static>> {
    CASES
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
}

/// Load the source record for one stable case ID.
///
/// # Panics
///
/// Panics when the ID is missing or a fixture record is malformed.
#[must_use]
pub fn case(id: &str) -> Case<'static> {
    cases()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("unknown annotation publication case {id}"))
}
