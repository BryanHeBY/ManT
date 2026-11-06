//! Original finite M2 cell rows, independent of whole-page token uniqueness.
use serde_json::Value;

pub struct DataRow<'a> {
    pub physical_row: usize,
    pub labels: Vec<&'a str>,
}

pub fn data_rows(case: &Value) -> Vec<DataRow<'_>> {
    let owners = case["expected"]["table_owners"].as_array().unwrap();
    assert_eq!(owners.len(), 1, "the original M2 core has one TS/TE");
    owners[0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, row)| row["kind"] == "data")
        .map(|(physical_row, row)| DataRow {
            physical_row,
            labels: row["cells"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .filter(|label| !label.is_empty())
                .collect(),
        })
        .collect()
}

pub fn original_occurrences(case: &Value, label: &str) -> usize {
    let in_table = data_rows(case)
        .iter()
        .flat_map(|row| &row.labels)
        .filter(|candidate| **candidate == label)
        .count();
    // The sealed column source has an independent first BODY `A` before
    // the TS/TE in its second BODY. Preserve both original A owners; this
    // source fact is not inferred from a consumer's selected first match.
    let outside = if case["axes"]["context"] == "column" && label == "A" {
        assert_eq!(
            case["source"]
                .as_str()
                .unwrap()
                .lines()
                .filter(|line| *line == ".It A Ta")
                .count(),
            1,
            "{}: original independent column BODY witness",
            case["id"]
        );
        1
    } else {
        0
    };
    in_table + outside
}

/// Return (row, column) for one cell, using an independently unique native
/// cell and native physical-span distances to distinguish an outer same word.
/// Global occurrence counts separately protect that outer authored content.
pub fn cell(
    case: &Value,
    physical_row: usize,
    label: &str,
    locate: impl Fn(&str) -> Vec<(usize, usize)>,
) -> (usize, usize) {
    let all = locate(label);
    assert_eq!(
        all.len(),
        original_occurrences(case, label),
        "{}: all original {label} owners survive exactly once",
        case["id"]
    );
    let rows = data_rows(case);
    let anchor = rows
        .iter()
        .flat_map(|row| {
            row.labels
                .iter()
                .map(move |label| (row.physical_row, *label))
        })
        .find(|(_, label)| original_occurrences(case, label) == 1)
        .expect("each sealed M2 table has a unique native cell C or D");
    let observed_anchor = locate(anchor.1);
    assert_eq!(
        observed_anchor.len(),
        1,
        "{}: unique original cell anchor",
        case["id"]
    );
    let first_row = observed_anchor[0].0.checked_sub(anchor.0).unwrap();
    let expected_row = first_row + physical_row;
    let candidates = all
        .into_iter()
        .filter(|(row, _)| *row == expected_row)
        .collect::<Vec<_>>();
    assert_eq!(
        candidates.len(),
        1,
        "{}: original table cell {label} on native data/rule row {physical_row}",
        case["id"]
    );
    candidates[0]
}
