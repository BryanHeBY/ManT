//! Search complete outline fields, then project hits onto viewport rows.
use crate::document::OutlineField;
#[cfg(test)]
use crate::document::OutlineRecord;
use std::{ops::Range, sync::Arc};

mod presentation;
mod scan;

#[cfg(test)]
use presentation::covered_by_label;
pub(crate) use presentation::{OutlineLabels, OutlinePresentation};
pub(crate) use scan::OutlineScan;
#[cfg(test)]
pub(crate) use scan::find;
#[cfg(test)]
use scan::{MAX_RANGES, MAX_RESULTS};

#[derive(Debug, Clone)]
pub(crate) struct FieldHit {
    pub(crate) field: OutlineField,
    pub(crate) ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone)]
pub(crate) struct OutlineHit {
    pub(crate) node_index: usize,
    label: Arc<str>,
    pub(crate) fields: Vec<FieldHit>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(index: usize, value: &str) -> OutlineRecord {
        let text: Arc<str> = Arc::from(value);
        OutlineRecord {
            node_index: index,
            label: Arc::clone(&text),
            fields: vec![OutlineField {
                text,
                target: false,
                anchor: format!("node-{index}"),
            }],
        }
    }

    #[test]
    fn result_limit_preserves_source_order_and_reports_incomplete_coverage() {
        let records = (0..=MAX_RESULTS)
            .map(|index| record(index, "needle"))
            .collect::<Vec<_>>();
        let (hits, limited) = find(&records, "needle");
        assert!(limited);
        assert_eq!(hits.len(), MAX_RESULTS);
        assert_eq!(hits[0].node_index, 0);
        assert_eq!(hits.last().unwrap().node_index, MAX_RESULTS - 1);
    }

    #[test]
    fn text_range_limit_keeps_the_first_real_hit_instead_of_claiming_a_miss() {
        let (hits, limited) = find(&[record(0, &"a".repeat(MAX_RANGES + 2))], "a");
        assert!(limited);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fields[0].ranges.len(), MAX_RANGES);
        assert_eq!(hits[0].fields[0].ranges[0], 0..1);
    }

    #[test]
    fn independent_fields_never_manufacture_a_cross_field_match() {
        let mut source = record(0, "abc");
        source.fields.push(OutlineField {
            text: Arc::from("def"),
            target: true,
            anchor: "target".into(),
        });
        let (hits, limited) = find(&[source], "cde");
        assert!(!limited);
        assert!(hits.is_empty());
    }

    #[test]
    fn complete_fields_match_independently_at_every_scan_budget() {
        use mant_protocol::{SearchCase, SearchSyntax};

        let mut heading = record(0, "cmd extra");
        let mut entry = record(1, "--help <ARG>");
        let mut embedded = record(2, "xcmdx");
        for (record, text, target) in [
            (&mut heading, "cmd", false),
            (&mut entry, "--help", false),
            (&mut embedded, "cmd", false),
        ] {
            record.fields.push(OutlineField {
                text: Arc::from(text),
                target,
                anchor: format!("field-{}", record.node_index),
            });
        }
        heading.fields.push(OutlineField {
            text: Arc::from("target.md"),
            target: true,
            anchor: "reference".into(),
        });
        let records = [heading, entry, embedded];
        for (pattern, syntax, expected) in [
            (
                "^cmd$",
                SearchSyntax::Regex,
                vec![(0, vec!["cmd"]), (2, vec!["cmd"])],
            ),
            (
                "^cmd extra$",
                SearchSyntax::Regex,
                vec![(0, vec!["cmd extra"])],
            ),
            (
                "\\bcmd\\b",
                SearchSyntax::Regex,
                vec![(0, vec!["cmd extra", "cmd"]), (2, vec!["cmd"])],
            ),
            (
                "cmd",
                SearchSyntax::Literal,
                vec![(0, vec!["cmd extra", "cmd"]), (2, vec!["xcmdx", "cmd"])],
            ),
            ("^--help$", SearchSyntax::Regex, vec![(1, vec!["--help"])]),
            (
                "^--help <ARG>$",
                SearchSyntax::Regex,
                vec![(1, vec!["--help <ARG>"])],
            ),
            (
                "^target\\.md$",
                SearchSyntax::Regex,
                vec![(0, vec!["target.md"])],
            ),
            ("cmdtarget", SearchSyntax::Literal, vec![]),
        ] {
            for budget in [1, 2, 7, 4096] {
                let stream =
                    crate::document::PatternStream::compile(pattern, syntax, SearchCase::Sensitive)
                        .unwrap();
                let mut scan = OutlineScan::with_stream(stream);
                let mut hits = Vec::new();
                let mut finished = false;
                for _ in 0..10_000 {
                    let (done, limited) = scan.advance(&records, &mut hits, budget);
                    assert!(!limited);
                    if done {
                        finished = true;
                        break;
                    }
                }
                assert!(finished, "{pattern}/{budget}");
                let actual = hits
                    .iter()
                    .map(|hit| {
                        (
                            hit.node_index,
                            hit.fields
                                .iter()
                                .map(|field| field.field.text.as_ref())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{pattern}/{budget}");
                for hit in &hits {
                    for field in &hit.fields {
                        for range in &field.ranges {
                            assert_ne!(&field.field.text[range.clone()], "");
                            if field.field.text.as_ref() == "cmd" {
                                assert_eq!(range, &(0..3));
                                assert_eq!(field.field.anchor, format!("field-{}", hit.node_index));
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn display_deduplication_requires_the_same_matched_label_range() {
        let field = FieldHit {
            field: OutlineField {
                text: Arc::from("cmd"),
                target: false,
                anchor: "reference".into(),
            },
            ranges: std::iter::once(0..3).collect(),
        };
        assert!(covered_by_label(
            &field,
            "cmd extra",
            std::slice::from_ref(&(0..3))
        ));
        assert!(!covered_by_label(&field, "cmd extra", &[]));
        assert!(!covered_by_label(
            &field,
            "cmd extra",
            std::slice::from_ref(&(4..9))
        ));
        assert!(covered_by_label(
            &field,
            "cmd cmd",
            std::slice::from_ref(&(4..7))
        ));
        let target = FieldHit {
            field: OutlineField {
                target: true,
                ..field.field.clone()
            },
            ..field
        };
        assert!(!covered_by_label(
            &target,
            "cmd extra",
            std::slice::from_ref(&(0..3))
        ));
    }
}
