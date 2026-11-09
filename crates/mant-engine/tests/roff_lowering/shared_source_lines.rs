//! Physical source rows are observed by native TEXT visitors, not AST owners.

use flate2::read::GzDecoder;
use serde::Deserialize;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

const MARKERS: [&str; 10] = [
    "BEFOREMARK",
    "MIDDLEMARK",
    "AFTERMARK",
    "FIRSTMARK",
    "NEXTMARK",
    "HEADMARK",
    "BODYMARK",
    "TAILMARK",
    "ENDMARK",
    "TAGMARK",
];

#[derive(Deserialize)]
struct Matrix {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    group: String,
    id: String,
    source: String,
    native_rows: Vec<(String, usize)>,
    projection_rows: Option<Vec<(String, usize)>>,
    glyph_rows: Option<Vec<String>>,
}

/// Read the complete order and multiplicity before measuring the rows. A
/// moved or duplicated marker cannot be hidden by taking only its first hit.
fn locate_markers(text: &str) -> Vec<(&'static str, usize)> {
    let mut hits = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let start = hits.len();
        for marker in MARKERS {
            hits.extend(
                line.match_indices(marker)
                    .map(|(col, _)| (row, col, marker)),
            );
        }
        hits[start..].sort_unstable_by_key(|&(_, col, _)| col);
    }
    hits.into_iter()
        .map(|(row, _, marker)| (marker, row))
        .collect()
}

#[test]
fn shared_source_line_matrix_matches_pristine_execution_rows() {
    // See the fixture README for CVS profiles, independently recorded
    // geometry, and the two explicitly responsive tbl controls.
    let archive = include_bytes!("shared_source_lines/cases.json.gz");
    let matrix: Matrix = serde_json::from_reader(GzDecoder::new(archive.as_slice())).unwrap();
    assert_eq!(matrix.cases.len(), 4189);
    verify_cases(
        matrix.cases,
        |case| &case.id,
        |case| {
            let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let content = json_string_roundtrip(&content);
            let plain = mant_render::render_query_man(&content);
            let hits = locate_markers(&plain);
            let origin = hits.first().map_or(0, |(_, row)| *row);
            let actual: Vec<_> = hits
                .iter()
                .map(|(marker, row)| (*marker, row - origin))
                .collect();
            let expected: Vec<_> = case
                .projection_rows
                .as_ref()
                .unwrap_or(&case.native_rows)
                .iter()
                .map(|(marker, row)| (marker.as_str(), *row))
                .collect();
            assert_eq!(actual, expected, "{}: {plain:?}", case.group);

            // M0 also checks accepted glyphs between the first and last marker.
            // Spacing cells remain portable, but interior empty rows, characters
            // affected by zero advance and punctuation may not disappear.
            if let Some(expected_glyphs) = &case.glyph_rows {
                let (_, end) = hits.last().unwrap();
                let actual_glyphs: Vec<String> = plain
                    .split('\n')
                    .skip(origin)
                    .take(end + 1 - origin)
                    .map(|row| row.chars().filter(|ch| !ch.is_whitespace()).collect())
                    .collect();
                assert_eq!(&actual_glyphs, expected_glyphs, "{plain:?}");
            }
        },
    );
}
