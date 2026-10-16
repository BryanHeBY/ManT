//! Finite current-oracle supplements leave historical recordings immutable.
#[path = "oracle_migrations/literal_reading.rs"]
pub(super) mod literal_reading;

use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    count: usize,
    profiles_per_source: usize,
    expectations_from_product: bool,
    oracle_identity: String,
    oracle_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    cohort: String,
    name: String,
    source: String,
    source_sha256: String,
    matrix_rows: Vec<String>,
    historical_matrix_rows: Option<Vec<String>>,
    body_rows: Option<Vec<String>>,
    historical_body_rows: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Fixture {
    header: Header,
    cases: Vec<Case>,
}

static MIGRATIONS: LazyLock<Fixture> = LazyLock::new(|| {
    let fixture: Fixture =
        serde_json::from_str(include_str!("oracle_migrations/breakpoints.json")).unwrap();
    assert_eq!(fixture.header.count, 11);
    assert_eq!(fixture.header.profiles_per_source, 5);
    assert!(!fixture.header.expectations_from_product);
    assert_eq!(libmandoc_rs::LIBMANDOC_VERSION, "cvs-20261013T025419Z");
    assert_eq!(
        fixture.header.oracle_identity,
        "cvs-20261013T025419Z-linux-x86_64-gcc-16.2.1-reproducible"
    );
    assert_eq!(
        fixture.header.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), fixture.header.count);
    for case in &fixture.cases {
        assert_eq!(case.source_sha256.len(), 64);
        assert!(
            case.source_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
    }
    fixture
});

#[derive(Clone, Copy)]
pub(super) enum Window {
    /// Historical matrix projection includes all authored section headings.
    Matrix,
    /// Acceptance JSON retains only the exact DESCRIPTION/NEXT window.
    Body,
}

fn source_case(cohort: &str, name: &str, source: &str) -> Option<&'static Case> {
    let case = MIGRATIONS
        .cases
        .iter()
        .find(|case| case.cohort == cohort && case.name == name)?;
    // Full byte equality is stronger than selecting by a name or a claimed
    // digest. Python's permanent contract independently verifies source SHA.
    assert_eq!(
        case.source.as_bytes(),
        source.as_bytes(),
        "{name}: exact source drift"
    );
    Some(case)
}

pub(super) fn expected_rows(
    cohort: &str,
    name: &str,
    source: &str,
    historical: &[String],
    window: Window,
) -> Option<&'static [String]> {
    let case = source_case(cohort, name, source)?;
    // The original snapshots remain assertion inputs, not files overwritten
    // by the current renderer. A supplement also binds their original rows.
    let (old, current) = match window {
        Window::Matrix => (
            case.historical_matrix_rows.as_deref(),
            Some(case.matrix_rows.as_slice()),
        ),
        Window::Body => (
            case.historical_body_rows.as_deref(),
            case.body_rows.as_deref(),
        ),
    };
    assert_eq!(
        old.expect("recorded historical window"),
        historical,
        "{name}: sealed rows drift"
    );
    Some(current.expect("recorded current window"))
}

pub(super) fn hang_rows(name: &str, source: &str) -> &'static [String] {
    let case = source_case("hang", name, source).expect("exact captured hang source");
    let start = case
        .matrix_rows
        .iter()
        .position(|row| row == "DESCRIPTION")
        .unwrap()
        + 1;
    &case.matrix_rows[start..]
}

#[test]
fn migration_supplement_is_finite_and_keeps_window_scopes_distinct() {
    let cases = &MIGRATIONS.cases;
    assert_eq!(
        cases.iter().filter(|case| case.cohort == "escape").count(),
        3
    );
    assert_eq!(
        cases.iter().filter(|case| case.cohort == "shared").count(),
        7
    );
    assert_eq!(cases.iter().filter(|case| case.cohort == "hang").count(), 1);
    for case in cases.iter().filter(|case| case.cohort == "shared") {
        assert!(case.matrix_rows.iter().any(|row| row == "DESCRIPTION"));
        assert!(
            !case
                .body_rows
                .as_ref()
                .unwrap()
                .iter()
                .any(|row| row == "DESCRIPTION")
        );
    }
}

#[test]
#[should_panic(expected = "exact source drift")]
fn migration_supplement_rejects_a_same_named_different_source() {
    let case = &MIGRATIONS.cases[0];
    let changed = format!("{}\n", case.source);
    let _ = source_case(&case.cohort, &case.name, &changed);
}
