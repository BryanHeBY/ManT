//! Small source-bound expectations, with full native profile identities nearby.

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
pub(super) struct Case {
    pub id: String,
    pub source: String,
    pub expected: Expected,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Expected {
    pub heading: String,
    pub prose_tokens: Vec<String>,
    pub prose_rows: Vec<ProseRow>,
    pub responsive_soft_wrap: bool,
    pub glyphs: BTreeMap<String, i32>,
    pub table_owners: Vec<TableOwner>,
    pub cell_semantics: Vec<CellSemantics>,
    pub links: Vec<ExpectedLink>,
    pub require_name_body: bool,
    pub authored_prefix: String,
    pub review_glyphs: BTreeMap<String, i32>,
}

#[derive(Deserialize)]
pub(super) struct CellSemantics {
    pub table: usize,
    pub row: usize,
    pub column: usize,
    pub emphasis: bool,
    pub manual_name: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ExpectedLink {
    pub kind: String,
    pub destination: String,
    pub label: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(super) struct ProseRow {
    pub text: String,
    pub column: i32,
}

#[derive(Deserialize)]
pub(super) struct TableOwner {
    pub first_span_line: u32,
    pub ts_line: u32,
    pub te_line: u32,
    pub declared_columns: u16,
    pub rows: Vec<TableRow>,
}

#[derive(Deserialize)]
pub(super) struct TableRow {
    pub source_line: u32,
    pub kind: String,
    pub cells: Vec<String>,
    #[serde(default)]
    pub cell_spans: Vec<CellSpan>,
}

#[derive(Deserialize)]
pub(super) struct CellSpan {
    pub column_span: u32,
    pub row_span: u32,
}

pub(super) fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!("cases.json")).unwrap();
    assert_eq!(fixture.case_count, fixture.cases.len());
    assert_eq!(fixture.profile_count, fixture.case_count * 5);
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    fixture.cases
}
