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

fn assert_reparsed_column_cell(query: &ResolvedContent, expected: &str) {
    let markdown = mant_codec::encode::render_markdown(query);
    let reader = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let blocks = &reader
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
        .blocks;
    let Block::Preformatted { children, .. } = &blocks[0] else {
        panic!("column fence changed container: {blocks:?}");
    };
    assert_eq!(inline_text(children), expected);
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
    // Fresh pristine replay confirms the inner tag HEAD/BODY end on separate
    // rows (termp_it_post -> term_newln). Portable nesting may simplify A's
    // placement, but cannot turn the accepted x/B seam into an invented colon.
    let query = reading_roundtrip(
        "cw10_list",
        &["A", "x", "B", "", "C"],
        "```\nA; x\nB\n\nC\n```",
    );
    // The exact source ran pristine CVS first. termp_it_post closes B's
    // row. The definition's Paragraph frame retires its provisional empty
    // tail; the outer completed VerticalSpace owns exactly one blank row.
    // That accepted hard boundary replaces the portable topology pipe.
    assert_reparsed_column_cell(&query, "A; x\nB\n\nC");
    let document = query.document.as_ref().unwrap();
    let [
        Block::Table {
            rows,
            column_preferences,
            ..
        },
        Block::VerticalSpace { lines: 1, .. },
    ] = document.sections[1].blocks.as_slice()
    else {
        panic!("nested table boundaries changed: {:?}", document.sections);
    };
    assert_eq!(
        column_preferences,
        &mant_ir::ColumnPreferences {
            widths: vec![1, 1],
            gap_columns: 4,
            advance_limit_columns: Some(256),
            extra_width_columns: Some(10),
        }
    );
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
        items[0].head_body_relation,
        mant_ir::HeadBodyRelation::Separate
    );
    let entry = items[0].entry.as_ref().expect("nested definition entry");
    assert_eq!(entry.names, ["x"]);
    assert_eq!(entry.id, "term-x");
    assert_eq!(items[0].description.len(), 1);
    // The exact cw10_list source was replayed with pristine CVS first.
    // Inner tag BODY post calls term_newln() (mdoc_term.c::termp_it_post),
    // closing B before the outer column post flush. The paragraph owns that
    // executed close; the consumer must not count it as another blank row.
    assert_eq!(paragraph(&items[0].description[0]), "B\n");
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
fn list_control_groups_execute_only_their_source_nodes_on_the_live_column() {
    // All 48 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // Bl pre calls term_newln once, leaving a fitting column's viscol/minbl
    // live. Absent control slices execute no node; ft nodes change a font,
    // not the physical row (mdoc_term.c::print_mdoc_node(), roff_term_pre()).
    // The inner tag HEAD then overruns because its own field begins after
    // that prior device position (term.c:113-127,250-253). Responsive table
    // placement gives A its own output row while retaining x/B separation.
    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        source: String,
        carrier: String,
        rows: Vec<String>,
        native_rows: Vec<String>,
        reading_rule: String,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "../native_execution/fixtures/table_control_rows/cases.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 48);
    let mut failures = Vec::new();
    for case in cases {
        assert_eq!(case.reading_rule, "responsive-nested-definition-owner");
        let label = if case.carrier.starts_with("Lk") {
            "x: https://ex.org"
        } else {
            "x"
        };
        assert_eq!(case.native_rows[0].trim_start(), format!("A {label}"));
        assert_eq!(
            case.native_rows[1..]
                .iter()
                .map(|row| row.trim_start())
                .collect::<Vec<_>>(),
            ["B", "", "C", ""]
        );
        let query = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let wire = mant_render::render_query_json(&query, false).unwrap();
        assert!(!wire.contains("\\u0000mant:"), "{}", case.name);
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored: ResolvedContent = decoded.into();
        assert_eq!(restored.document, query.document, "{}", case.name);
        let mut expected = ["NAME", "test – probe", "", "DESCRIPTION"]
            .map(str::to_owned)
            .to_vec();
        expected.extend(case.rows);
        expected.extend(["", "NEXT", "END"].map(str::to_owned));
        let actual = actual_rows(&mant_render::render_query_man(&restored));
        if actual != expected {
            failures.push(format!(
                "{}\n{}\nactual {actual:?}\nreference {expected:?}",
                case.name, case.source
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
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
            column_preferences,
            ..
        },
        Block::VerticalSpace { lines: 1, .. },
    ] = document.sections[1].blocks.as_slice()
    else {
        panic!("boxed table boundaries changed: {:?}", document.sections);
    };
    assert_eq!(column_preferences, &mant_ir::ColumnPreferences::default());
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
    assert_eq!(rows[1].cells.len(), 0);
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
