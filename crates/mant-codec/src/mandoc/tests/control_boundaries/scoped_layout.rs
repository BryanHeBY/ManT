//! Source node scopes restore geometry without reusing another owner's origin.

use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    case_count: usize,
    profile_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    source_sha256: String,
    profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
struct Profile {
    status: i32,
}

fn recorded_cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!("scoped_layout/cases.json")).unwrap();
    assert_eq!(fixture.case_count, 16);
    assert_eq!(fixture.cases.len(), fixture.case_count);
    assert_eq!(fixture.profile_count, fixture.case_count * 5);
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    for case in &fixture.cases {
        let mut source_hash = String::with_capacity(64);
        for byte in &Sha256::digest(case.source.as_bytes()) {
            write!(source_hash, "{byte:02x}").unwrap();
        }
        assert_eq!(source_hash, case.source_sha256, "{}", case.id);
        assert_eq!(
            case.profiles.keys().map(String::as_str).collect::<Vec<_>>(),
            ["ascii", "html", "lint", "tree", "utf8"]
        );
        assert!(case.profiles.values().all(|profile| profile.status == 0));
    }
    fixture.cases
}

fn definition_items(blocks: &[Block]) -> &[mant_ir::DefinitionItem] {
    let Block::DefinitionList { items, .. } = &blocks[0] else {
        panic!("expected original definition owner: {blocks:#?}");
    };
    items
}

fn nested_body<'a>(id: &str, blocks: &'a [Block]) -> &'a [Block] {
    if id.starts_with("nested-") {
        &definition_items(blocks)[0].description
    } else {
        blocks
    }
}

fn assert_term_origins(blocks: &[Block], id: &str) {
    let items = definition_items(nested_body(id, blocks));
    assert_eq!(items.len(), 2, "{id}");
    for (item, label) in items.iter().zip(["#", "-"]) {
        assert_eq!(inline_text(&item.terms[0]), label, "{id}");
        assert_eq!(
            item.terms[0].inline_layout.row_layout(0).indent_columns,
            0,
            "{id}/{label}: the list already owns its source origin"
        );
    }
}

fn assert_literal_origin(blocks: &[Block], id: &str) {
    let literal = if id == "plain-display" {
        &blocks[0]
    } else {
        &definition_items(blocks)[0].description[0]
    };
    let Block::Preformatted {
        children,
        inline_layout,
        layout,
        ..
    } = literal
    else {
        panic!("expected the authored literal owner: {literal:#?}");
    };
    assert_eq!(inline_text(children), "CODE", "{id}");
    assert_eq!(
        layout.indent_columns,
        if id == "negative-display-reading-origin" {
            -2
        } else {
            6
        },
        "{id}"
    );
    assert_eq!(inline_layout.row_layout(0).indent_columns, 0, "{id}");
}

fn assert_column_head_row(blocks: &[Block], id: &str) {
    let Block::Table { rows, .. } = &nested_body(id, blocks)[0] else {
        panic!("expected original column table: {blocks:#?}");
    };
    assert_eq!(rows.len(), 2, "{id}");
    assert_eq!(rows[0].cells.len(), 3, "{id}");
    for (cell, label) in rows[0].cells.iter().zip(["Backend", "Path", "Description"]) {
        assert!(!cell.break_after, "{id}/{label}: It pre reinstalls NOBREAK");
        let Block::Paragraph { children, .. } = &cell.blocks[0] else {
            panic!("{id}: expected original heading owner");
        };
        assert_eq!(inline_text(children), label, "{id}");
    }
}

fn assert_paragraph_distance(blocks: &[Block], id: &str) {
    let body = nested_body(id, blocks);
    assert!(matches!(body[0], Block::List { .. }), "{id}");
    let empty_rows = body
        .iter()
        .filter_map(|block| match block {
            Block::VerticalSpace { lines, .. } => Some(*lines),
            _ => None,
        })
        .sum::<u16>();
    assert_eq!(empty_rows, 1, "{id}: the real Pp emits one empty row");
    let Block::Paragraph { children, .. } = body.last().unwrap() else {
        panic!("{id}: expected AFTER owner");
    };
    assert_eq!(inline_text(children), "AFTER", "{id}");
}

fn assert_nested_offset_origins(blocks: &[Block]) {
    let outer = definition_items(blocks);
    assert_eq!(outer.len(), 2);
    let inner = definition_items(&outer[0].description);
    for (items, labels) in [(outer, ["OUTER", "LATER"]), (inner, ["FIRST", "SECOND"])] {
        for (item, label) in items.iter().zip(labels) {
            assert_eq!(inline_text(&item.terms[0]), label);
            // It pre applies the list offset to the native field as well;
            // the list's structural placement already represents it.
            assert_eq!(item.terms[0].inline_layout.row_layout(0).indent_columns, 0);
        }
    }
}

fn assert_ragged_post_distance(blocks: &[Block]) {
    let body = &definition_items(blocks)[0].description;
    let Block::Paragraph {
        children, layout, ..
    } = &body[0]
    else {
        panic!("ragged display must retain its paragraph owner: {body:#?}");
    };
    assert_eq!(inline_text(children), ".Ft\n.Fc");
    assert_eq!(layout.indent_columns, 6);
    let spaces = body
        .iter()
        .filter_map(|block| match block {
            Block::VerticalSpace { lines, .. } => Some(*lines),
            _ => None,
        })
        .sum::<u16>();
    assert_eq!(spaces, 1, "Ed's completed row cannot become Pp's extra row");
    let Block::Paragraph { children, .. } = body.last().unwrap() else {
        panic!("missing AFTER owner");
    };
    assert_eq!(inline_text(children), "AFTER");
}

fn assert_column_return_origin(blocks: &[Block]) {
    let body = &definition_items(blocks)[0].description;
    let Block::Preformatted {
        children,
        inline_layout,
        layout,
        ..
    } = body.last().unwrap()
    else {
        panic!("missing authored CODE owner: {body:#?}");
    };
    assert_eq!(inline_text(children), "CODE");
    assert_eq!(layout.indent_columns, 6);
    assert_eq!(inline_layout.row_layout(0).indent_columns, 0);
}

fn assert_temporary_author_blank(blocks: &[Block]) {
    let body = &definition_items(blocks)[0].description;
    let Block::Paragraph {
        children,
        inline_layout,
        ..
    } = &body[0]
    else {
        panic!("missing BODY paragraph: {body:#?}");
    };
    assert_eq!(
        inline_text(children),
        "The default keyword for this option is:\n ipv4\nAFTER"
    );
    // .ti +4 survives term_newln over an empty buffer. The authored space
    // remains a scalar; only the four-column temporary origin is a hint.
    assert_eq!(
        inline_layout
            .row_layout(1)
            .resolve(8, 8)
            .first_visual_origin,
        12
    );
    assert_eq!(
        inline_layout
            .row_layout(2)
            .resolve(8, 8)
            .first_visual_origin,
        8
    );
}

fn assert_scoped_layout(document: &mant_ir::Document, id: &str) {
    let blocks = &document.sections[1].blocks;
    match id {
        "nested-tag-two-items" | "nested-tag-two-paragraphs" | "tag-two-items" => {
            assert_term_origins(blocks, id);
        }
        "tag-display-sibling" | "plain-display" | "negative-display-reading-origin" => {
            assert_literal_origin(blocks, id);
        }
        "nested-column-header" | "nested-column-plain-header" | "column-header" => {
            assert_column_head_row(blocks, id);
        }
        "nested-enum-paragraph" | "nested-item-paragraph" | "enum-paragraph" => {
            assert_paragraph_distance(blocks, id);
        }
        "nested-list-offset-origins" => assert_nested_offset_origins(blocks),
        "ragged-display-paragraph" => assert_ragged_post_distance(blocks),
        "column-return-literal-origin" => assert_column_return_origin(blocks),
        "temporary-origin-with-author-blank" => assert_temporary_author_blank(blocks),
        _ => panic!("unclassified sealed source: {id}"),
    }
}

#[test]
fn native_node_scopes_deliver_each_output_origin_and_real_body_post_once() {
    // Each complete source ran all five pristine profiles before assertions.
    // print_mdoc_node restores offset/rmargin, not device/word registers;
    // It BODY pre installs its own flags, and actual post consumes that row
    // before Pp's independent vspace (mdoc_term.c:437,817,936,1579).
    for case in recorded_cases() {
        let document = parse_manual_bytes(
            std::path::Path::new("scoped-layout-boundary.1"),
            case.source.as_bytes(),
        )
        .unwrap();
        assert_scoped_layout(&document, &case.id);
        let json = serde_json::to_string(&document).unwrap();
        let restored = serde_json::from_str::<mant_ir::Document>(&json).unwrap();
        assert_scoped_layout(&restored, &case.id);
    }
}

#[test]
fn term_origins_preserve_real_field_gaps_and_already_flushed_row_receipts() {
    // These complete sources ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // term_field emits minbl before a graph on an occupied row (term.c:
    // 113-116,397-427). A term owns only its declared origin, not a BODY's
    // replacement alignment. mc's accepted endline is already executed;
    // projecting it must not attempt another empty term_newln (475-481).
    let header =
        ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    let cases = [
        (
            format!("{header}.Bl -tag -width 8n\n.It Xo\n.No \"LABEL\"\n.br\n.No AFTER\n.Xc\n.No BodyWord\n.El\n.Sh NEXT\n.No END\n"),
            "LABEL  AFTER",
        ),
        (
            format!("{header}.Bl -tag -width 4n\n.It Xo\n.No \\zX\n.sp 0\n.No AfterWord\n.Xc\n.No BodyWord\n.El\n.Sh NEXT\n.No END\n"),
            "X  AfterWord",
        ),
        (
            format!("{header}.Bl -tag -width 8n\n.It Xo\n.No \"A\\c\"\n.nf\n.No MID\n.fi\n.No AFTER\n.Xc\n.No BodyWord\n.El\n.Sh NEXT\n.No END\n"),
            "A         MID\nAFTER",
        ),
        (
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo\n.No A\n.mc\n.No C\n.mc\n.No D\n.Xc\n.No BODY\n.El\n".to_owned(),
            "A   C\n D",
        ),
    ];
    for (source, expected) in cases {
        let document =
            parse_manual_bytes(std::path::Path::new("field-owner-gap.1"), source.as_bytes())
                .unwrap();
        for document in [
            document.clone(),
            serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap(),
        ] {
            let item = &definition_items(&document.sections[1].blocks)[0];
            assert_eq!(item.terms.len(), 1, "{source}");
            assert_eq!(inline_text(&item.terms[0]), expected, "{source}");
        }
    }
}
