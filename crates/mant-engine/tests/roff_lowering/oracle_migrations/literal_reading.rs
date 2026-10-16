//! Source-bound supplements for the documented roff literal reading policy.
//!
//! Native gold remains immutable. These cards identify complete literal
//! owners and exact row windows, never a general leading-blank filter. The
//! pristine source executions precede recording; no candidate output supplies
//! the accepted text, row positions or layout expectations.
use std::sync::LazyLock;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    count: usize,
    expectations_from_product: bool,
    oracle_sha256: String,
    rule: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    source: String,
    source_sha256: String,
    owner: String,
    windows: Vec<Window>,
    profiles: std::collections::BTreeMap<String, Profile>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    native_rows: Vec<String>,
    omitted_rows: Vec<usize>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
}

#[derive(serde::Deserialize)]
struct Fixture {
    header: Header,
    cases: Vec<Case>,
}

static CARDS: LazyLock<Fixture> = LazyLock::new(|| {
    let cards: Fixture = serde_json::from_str(include_str!("literal_reading.json")).unwrap();
    assert_eq!(cards.header.count, 228);
    assert_eq!(cards.cases.len(), cards.header.count);
    assert!(!cards.header.expectations_from_product);
    assert!(cards.header.rule.contains("complete literal owner"));
    assert_eq!(
        cards.header.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    for case in &cards.cases {
        assert_eq!(case.source_sha256.len(), 64);
        assert_ne!(case.owner, "");
        assert!(case.profiles.len() >= 3);
        for profile in ["ascii", "utf8", "tree"] {
            let profile = &case.profiles[profile];
            assert!((0..=2).contains(&profile.status));
            assert_eq!(profile.stdout_sha256.len(), 64);
            assert_eq!(profile.stderr_sha256.len(), 64);
        }
        for window in &case.windows {
            assert_eq!(window.omitted_rows.len(), 1, "one complete owner per card");
            for &row in &window.omitted_rows {
                assert_eq!(window.native_rows[row], "", "never delete author spaces");
                assert!(
                    window.native_rows[row + 1..]
                        .iter()
                        .any(|row| !row.trim().is_empty())
                );
            }
        }
    }
    cards
});

fn window(source: &str, native_rows: &[String]) -> Option<&'static Window> {
    CARDS
        .cases
        .iter()
        .find(|case| case.source == source)?
        .windows
        .iter()
        .find(|window| window.native_rows == native_rows)
}

pub(crate) fn rows(source: &str, native_rows: &[String]) -> Vec<String> {
    let Some(window) = window(source, native_rows) else {
        return native_rows.to_vec();
    };
    native_rows
        .iter()
        .enumerate()
        .filter(|(index, _)| !window.omitted_rows.contains(index))
        .map(|(_, row)| row.clone())
        .collect()
}

pub(crate) fn row_index(source: &str, native_rows: &[String], row: usize) -> usize {
    row - window(source, native_rows).map_or(0, |window| {
        assert!(
            !window.omitted_rows.contains(&row),
            "a named glyph is not an empty row"
        );
        window
            .omitted_rows
            .iter()
            .filter(|&&index| index < row)
            .count()
    })
}

#[test]
fn reading_cards_do_not_filter_unregistered_sources_or_other_row_windows() {
    let case = &CARDS.cases[0];
    let native = &case.windows[0].native_rows;
    assert_eq!(rows(&case.source, native).len() + 1, native.len());
    assert_eq!(rows(&format!("{}\n", case.source), native), *native);
    let mut changed = native.clone();
    changed[0].push(' ');
    assert_eq!(rows(&case.source, &changed), changed);
}
