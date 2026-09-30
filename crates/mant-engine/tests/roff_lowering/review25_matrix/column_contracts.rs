//! Approved responsive table geometry has an explicit contract, rather than
//! passing a known-red terminal comparison by folding whitespace. Pristine
//! CVS ASCII/UTF-8/HTML runs preceded these assertions. The original oracle
//! snapshots stay unchanged; authored spacing, rules, spans and owners are
//! checked independently from device box drawing and nested-list placement.

use mant_ir::{Block, ResolvedContent, TableRowKind, inline_plain_text as inline_text};

use super::{actual_rows, load_case};

fn reading_roundtrip(name: &str, body: &[&str], fence: &str) -> ResolvedContent {
    let (query, _) = load_case(name);
    let mut rows = vec!["NAME", "test – probe", "", "DESCRIPTION"];
    rows.extend_from_slice(body);
    rows.extend_from_slice(&["", "", "NEXT", "END"]);
    assert_eq!(
        actual_rows(&mant_render::render_query_man(&query)),
        rows,
        "{name}"
    );
    let decorated =
        mant_render::render_query_text_with(&query, |_, text| format!("\u{1b}[1m{text}\u{1b}[0m"));
    let plain = decorated.replace("\u{1b}[1m", "").replace("\u{1b}[0m", "");
    assert_eq!(actual_rows(&plain), rows, "{name}: ANSI geometry");
    let markdown = mant_codec::encode::render_markdown(&query);
    assert!(markdown.contains(fence), "{name}: {markdown}");

    let json = mant_render::render_query_json(&query, false).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let decoded: ResolvedContent = decoded.into();
    assert_eq!(
        mant_render::render_query_man(&decoded),
        mant_render::render_query_man(&query)
    );
    assert_eq!(mant_codec::encode::render_markdown(&decoded), markdown);
    assert_eq!(
        decoded.document, query.document,
        "{name}: exact JSON semantics"
    );
    decoded
}

fn paragraph(block: &Block) -> String {
    let Block::Paragraph { children, .. } = block else {
        panic!("expected paragraph: {block:?}");
    };
    inline_text(children)
}

#[test]
fn nested_definition_cells_keep_content_order_and_entry_identity() {
    // Native termp_it_pre/term_flushln reuse the device field for the nested
    // tag, producing A x / B / blank / C. The reviewed v0.12 responsive
    // contract preserves the definition owner and its actually closed HEAD
    // row, while the outer source-order cell origin puts A on its own row.
    // A prior native viscol must not disappear merely because IR drained.
    let (_, native) = load_case("cw10_list");
    assert_eq!(&native[4..8], ["A x", "B", "", "C"]);
    let query = reading_roundtrip(
        "cw10_list",
        &["A", "x", "B", "", "C"],
        "```\nA; x: B | C\n```",
    );
    let document = query.document.as_ref().unwrap();
    let [
        Block::Table {
            rows,
            column_widths,
            ..
        },
        Block::VerticalSpace { lines: 1, .. },
    ] = document.sections[1].blocks.as_slice()
    else {
        panic!("nested table boundaries changed: {:?}", document.sections);
    };
    assert_eq!(column_widths, &[1, 1]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 2);
    let [
        leading,
        Block::DefinitionList {
            items,
            compact: true,
            ..
        },
        // Nested It post cleared NOBREAK and completed its graph row.
        // Outer column It post's empty flush emits a further real row.
        Block::VerticalSpace { lines: 1, .. },
    ] = rows[0].cells[0].blocks.as_slice()
    else {
        panic!("nested definition escaped its column: {:?}", rows[0]);
    };
    assert_eq!(paragraph(leading), "A");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].terms.len(), 1);
    assert_eq!(inline_text(&items[0].terms[0]), "x");
    assert_eq!(
        items[0].layout.head_body_relation,
        mant_ir::HeadBodyRelation::Separate
    );
    let entry = items[0].entry.as_ref().expect("nested definition entry");
    assert_eq!(entry.names, ["x"]);
    assert_eq!(entry.id, "term-x");
    assert_eq!(items[0].description.len(), 1);
    assert_eq!(paragraph(&items[0].description[0]), "B");
    assert_eq!(rows[0].cells[1].blocks.len(), 1);
    assert_eq!(paragraph(&rows[0].cells[1].blocks[0]), "C");
    let explanation = mant_query::explain_query(
        &query,
        &mant_protocol::ExplanationQuery {
            entry: "x".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(
        explanation
            .evidence
            .iter()
            .filter(|evidence| evidence.class == mant_protocol::EvidenceClass::DirectEntry)
            .count(),
        1
    );
    let wire = serde_json::to_string(&explanation).unwrap();
    let decoded: mant_protocol::QueryExplanation = serde_json::from_str(&wire).unwrap();
    assert_eq!(decoded, explanation);
    assert!(mant_render::render_explanation_text(&decoded).contains('B'));
}

#[test]
fn boxed_tbl_keeps_its_single_cell_without_copying_device_frame_glyphs() {
    // tbl_term.c::term_tbl draws the optional frame around the data span.
    // The frozen IR contract retains table data and topology, while ASCII
    // frame glyphs and their device-only rows are outside portable layout.
    let (_, native) = load_case("cw12_box");
    assert_eq!(&native[4..7], ["┌─────────┐", "│ A B     │", "└─────────┘"]);
    let query = reading_roundtrip("cw12_box", &["A B"], "```\nA B\n```");
    let document = query.document.as_ref().unwrap();
    let [
        Block::Table {
            rows,
            column_widths,
            ..
        },
        Block::VerticalSpace { lines: 1, .. },
    ] = document.sections[1].blocks.as_slice()
    else {
        panic!("boxed table boundaries changed: {:?}", document.sections);
    };
    assert!(column_widths.is_empty());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, TableRowKind::Data);
    assert_eq!(rows[0].cells.len(), 1);
    assert_eq!(rows[0].cells[0].column_span, 1);
    assert_eq!(paragraph(&rows[0].cells[0].blocks[0]), "A B");
}

#[test]
fn spanning_tbl_keeps_covered_slots_and_the_authored_rule_in_every_export() {
    // The tbl data '_' span is TBL_SPAN_HORIZ, not a box decoration. Its
    // rule survives as a separate row. Device width six becomes portable
    // '---'; covered slots remain empty rather than moving following cells.
    let (_, native) = load_case("cw12_span");
    assert_eq!(&native[4..7], ["A", "──────", "B C"]);
    let query = reading_roundtrip(
        "cw12_span",
        &["A |", "---", "B C |"],
        "```\nA | \n---\nB C | \n```",
    );
    let document = query.document.as_ref().unwrap();
    let [
        Block::Table { rows, .. },
        Block::VerticalSpace { lines: 1, .. },
    ] = document.sections[1].blocks.as_slice()
    else {
        panic!("span table boundaries changed: {:?}", document.sections);
    };
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].kind, TableRowKind::HorizontalRule);
    assert!(rows[1].cells.is_empty());
    for (row, expected) in [(&rows[0], "A"), (&rows[2], "B C")] {
        assert_eq!(row.kind, TableRowKind::Data);
        assert_eq!(row.cells.len(), 1);
        assert_eq!(row.cells[0].column_span, 2);
        assert_eq!(row.cells[0].row_span, 1);
        assert_eq!(paragraph(&row.cells[0].blocks[0]), expected);
    }
}

#[test]
fn portable_table_rows_retain_empty_rows_and_each_rule_strength() {
    // Exact pristine input runs preceded these assertions. term_tbl keeps
    // the empty data span and single/double rule spans in source order;
    // suppressed data under a layout rule must never become portable text.
    for (source, fence) in [
        (
            ".TH PROBE 1\n.SH DESCRIPTION\n.TS\nl.\nBEFORE\n\n_\n=\nAFTER\n.TE\n",
            "```\nBEFORE\n\n---\n===\nAFTER\n```",
        ),
        (
            ".TH PROBE 1\n.SH DESCRIPTION\n.TS\n_ l.\nIGNORED\tVISIBLE\n.TE\n",
            "```\n--- | VISIBLE\n```",
        ),
        (
            ".TH PROBE 1\n.SH DESCRIPTION\n.TS\n_ =\nl l.\nA\tB\n.TE\n",
            "```\n--- | ===\nA | B\n```",
        ),
    ] {
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let markdown = mant_codec::encode::render_markdown(&decoded.into());
        assert!(markdown.contains(fence), "{source}\n{markdown}");
        assert!(!markdown.contains("IGNORED"));
    }
}
