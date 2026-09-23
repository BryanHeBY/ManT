use mant_ir::{
    Document, Provenance, SourceCoordinates, SourceIdentity, SourceKey, SourceRecord, SourceSpan,
    TextRange, TextSize, validate_document_sources,
};
use serde_json::{Value, json};

fn document_value() -> Value {
    json!({
        "sources": [
            {
                "key": 1,
                "identity": {"kind": "path", "name": "/manual/root.1"},
                "format": "man",
                "decodedByteLength": 10,
                "contentSha256": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
                    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
                "coordinates": {"kind": "decoded-utf8-bytes"}
            },
            {
                "key": 2,
                "identity": {"kind": "bundle-member", "name": "includes/common.roff"},
                "format": "man",
                "decodedByteLength": 4,
                "coordinates": {"kind": "native-normalized-bytes"}
            }
        ],
        "rootSource": 1,
        "contentStore": {
            "owners": [], "roots": [], "atoms": [], "points": [], "links": []
        },
        "meta": {},
        "sections": []
    })
}

fn span(source: u32) -> Value {
    json!({
        "source": source,
        "byteRange": {"start": 1, "end": 3},
        "line": 1,
        "column": 2,
        "endLine": 1,
        "endColumn": 4
    })
}

fn assert_document_rejected(value: Value) {
    assert!(
        serde_json::from_value::<Document>(value).is_err(),
        "invalid document crossed the real serde boundary"
    );
}

#[test]
fn source_shapes_and_provenance_round_trip_without_narrowing_offsets() {
    let identities = [
        SourceIdentity::Path {
            name: "/manual/root.1".into(),
        },
        SourceIdentity::BundleMember {
            name: "includes/common.roff".into(),
        },
        SourceIdentity::Anonymous {
            name: "stdin".into(),
        },
    ];
    for identity in identities {
        let encoded = serde_json::to_value(&identity).unwrap();
        assert_eq!(
            serde_json::from_value::<SourceIdentity>(encoded).unwrap(),
            identity
        );
    }

    for coordinates in [
        SourceCoordinates::DecodedUtf8Bytes,
        SourceCoordinates::NativeNormalizedBytes,
    ] {
        let encoded = serde_json::to_value(coordinates).unwrap();
        assert_eq!(
            serde_json::from_value::<SourceCoordinates>(encoded).unwrap(),
            coordinates
        );
    }

    let exact = SourceSpan {
        source: SourceKey::FIRST,
        byte_range: Some(TextRange::new(
            TextSize::new(u64::from(u32::MAX) + 7),
            TextSize::new(u64::from(u32::MAX) + 9),
        )),
        line: 2,
        column: 3,
        end_line: Some(2),
        end_column: Some(5),
    };
    let provenances = [
        Provenance::Authored { span: exact },
        Provenance::Generated {
            trigger: Some(exact),
        },
        Provenance::Unknown,
    ];
    for provenance in provenances {
        let encoded = serde_json::to_value(provenance).unwrap();
        assert_eq!(
            serde_json::from_value::<Provenance>(encoded).unwrap(),
            provenance
        );
    }
}

#[test]
fn source_structural_objects_and_known_variants_reject_extra_fields() {
    assert!(serde_json::from_value::<SourceKey>(json!(0)).is_err());
    assert!(
        serde_json::from_value::<SourceIdentity>(
            json!({"kind":"anonymous", "name":"stdin", "path":"old"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<SourceCoordinates>(
            json!({"kind":"decoded-utf8-bytes", "unit":"legacy"})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<Provenance>(json!({"kind":"unknown", "span":null})).is_err());
    assert!(
        serde_json::from_value::<SourceSpan>(json!({
            "source":1, "line":1, "column":1, "legacyPath":"root.1"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SourceRecord>(json!({
            "key":1,
            "identity":{"kind":"anonymous","name":"stdin"},
            "format":"man",
            "decodedByteLength":0,
            "coordinates":{"kind":"decoded-utf8-bytes"},
            "path":"old"
        }))
        .is_err()
    );
}

#[test]
fn document_deserialization_enforces_dense_rooted_source_relations() {
    let document: Document = serde_json::from_value(document_value()).unwrap();
    assert_eq!(document.root_source, SourceKey::FIRST);
    assert_eq!(document.root_path(), Some("/manual/root.1"));
    assert_eq!(
        document
            .source_record(SourceKey::new(2).unwrap())
            .unwrap()
            .decoded_byte_length,
        4
    );
    validate_document_sources(&document).unwrap();

    let mut empty = document_value();
    empty["sources"] = json!([]);
    assert_document_rejected(empty);

    let mut non_dense = document_value();
    non_dense["sources"][1]["key"] = json!(3);
    assert_document_rejected(non_dense);

    let mut non_first_root = document_value();
    non_first_root["rootSource"] = json!(2);
    assert_document_rejected(non_first_root);

    let mut mixed_format = document_value();
    mixed_format["sources"][1]["format"] = json!("mdoc");
    assert_document_rejected(mixed_format);

    let mut bad_member = document_value();
    bad_member["sources"][1]["identity"]["name"] = json!("includes/../secret");
    assert_document_rejected(bad_member);

    let old_singular = json!({
        "source":{"format":"man","path":"root.1"},
        "meta":{},
        "sections":[]
    });
    assert_document_rejected(old_singular);
}

#[test]
fn table_cell_source_is_independent_and_checked_at_the_wire_boundary() {
    let mut document = document_value();
    document["blocks"] = json!([{
        "type": "table",
        "rows": [{
            "cells": [{
                "blocks": [],
                "source": {"source": 2, "line": 1, "column": 1}
            }]
        }]
    }]);
    let parsed: Document = serde_json::from_value(document.clone()).unwrap();
    assert!(validate_document_sources(&parsed).is_ok());
    assert!(mant_ir::document_has_source_spans(&parsed));

    document["blocks"][0]["rows"][0]["cells"][0]["source"]["source"] = json!(3);
    assert_document_rejected(document);
}

#[test]
fn every_nested_span_is_checked_against_its_selected_source() {
    let mut unknown_key = document_value();
    unknown_key["heading"] = json!({"content":[], "source":span(3)});
    assert_document_rejected(unknown_key);

    let mut overflow = document_value();
    overflow["heading"] = json!({"content":[], "source":span(1)});
    overflow["heading"]["source"]["byteRange"]["end"] = json!(11);
    assert_document_rejected(overflow);

    let mut inexact_domain = document_value();
    inexact_domain["heading"] = json!({"content":[], "source":span(2)});
    assert_document_rejected(inexact_domain);

    let mut incomplete_end = document_value();
    incomplete_end["heading"] = json!({
        "content":[],
        "source":{"source":1,"line":1,"column":1,"endLine":2}
    });
    assert_document_rejected(incomplete_end);

    let mut anchor = document_value();
    anchor["blocks"] = json!([{
        "type":"paragraph",
        "children":[{
            "type":"anchor", "id":"target", "ownerSource":{
                "source":9,"line":1,"column":1
            }
        }],
        "layout":{},
        "source":null
    }]);
    assert_document_rejected(anchor);
}

#[test]
fn a_valid_but_factually_wrong_source_key_remains_an_oracle_responsibility() {
    let mut root_binding = document_value();
    root_binding["heading"] = json!({
        "content":[],
        "source":{"source":1,"line":1,"column":1}
    });
    let root_document: Document = serde_json::from_value(root_binding).unwrap();

    let mut include_binding = document_value();
    include_binding["heading"] = json!({
        "content":[],
        "source":{"source":2,"line":1,"column":1}
    });
    let include_document: Document = serde_json::from_value(include_binding).unwrap();

    let root_span = root_document.heading.unwrap().source.unwrap();
    let include_span = include_document.heading.unwrap().source.unwrap();
    assert_ne!(root_span.source, include_span.source);
    assert_eq!(root_span.line, include_span.line);
    assert_eq!(root_span.column, include_span.column);
}
