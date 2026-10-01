//! Declared column geometry, boundaries and consumers for `.Bl -column` and tbl.
//!
//! Cases `cw03`–`cw12` pin `mdoc_term.c::termp_it_pre`: Unicode widths,
//! escape-executed declarations, the 4/3/1 dcol gap, extra cells, multiline
//! cell content, signed origins and tbl rules/spans. Extreme parsed widths
//! are covered separately by `column_safety_boundaries`.
//!
//! Additional contracts require plain/ANSI row parity, reject handcrafted
//! and inbound JSON geometry outside the same IR bounds, and bound padding
//! at 1/10/100/1000 rows. Every matching native case also runs an actual JSON
//! text round trip. Approved reading layouts retain dedicated exact-row,
//! owner and topology assertions in `column_contracts`; real TUI cell and
//! depth tests cover the interactive consumers independently.

use std::fmt::Write as _;
use std::panic::AssertUnwindSafe;

use super::{MatrixRun, actual_rows, case_names, load_case};

/// The pinned reference is retained for these three cases. Their approved
/// reading geometry is asserted separately with exact rows, semantic owners,
/// rules and spans in `column_contracts`; they are not unresolved red cases.
const READING_LAYOUT_CASES: &[&str] = &["cw10_list", "cw12_box", "cw12_span"];

/// Strip SGR control sequences so plain and decorated renders compare on
/// visible text only (geometry must be identical after
/// ANSI is removed).
fn strip_sgr(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('\u{1b}') {
        stripped.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('m') {
            Some(close) => rest = &after[close + 1..],
            None => {
                rest = "";
            }
        }
    }
    stripped.push_str(rest);
    stripped
}

/// CW04/CW15 CLI half: an SGR-decorated render must project to the same
/// rows as the plain render; the declared-column flow may not measure
/// decorated strings (`blocks.rs` line-length arithmetic).
fn ansi_parity(query: &mant_ir::ResolvedContent, expected: &[String]) -> Option<String> {
    let decorated =
        mant_render::render_query_text_with(query, |_, text| format!("\u{1b}[1m{text}\u{1b}[0m"));
    let rows = actual_rows(&strip_sgr(&decorated));
    (rows != expected).then(|| format!("ANSI-stripped rows diverged: {rows:?}"))
}

#[test]
fn declared_column_matrix_matches_the_pinned_reference() {
    let mut matrix = MatrixRun::new();
    for name in case_names("cw", 27) {
        if !READING_LAYOUT_CASES.contains(&name.as_str()) {
            matrix.evaluate(&name, Some(&ansi_parity));
        }
    }
    matrix.finish("CW");
}

/// Mutate every table of a loaded case to the given declared widths.
fn with_column_widths(
    mut query: mant_ir::ResolvedContent,
    widths: &[u16],
) -> mant_ir::ResolvedContent {
    use mant_ir::Block;
    let document = query.document.as_mut().expect("case document");
    for block in &mut document.blocks {
        if let Block::Table { column_widths, .. } = block {
            column_widths.clone_from(&widths.to_vec());
        }
    }
    for section in &mut document.sections {
        for block in &mut section.blocks {
            if let Block::Table { column_widths, .. } = block {
                column_widths.clone_from(&widths.to_vec());
            }
        }
    }
    query
}

#[test]
fn handcrafted_ir_extremes_validate_like_the_parsed_entry() {
    // Review §25.2 CW14: protection must not live in the roff parser
    // only. u16::MAX-declared widths reach the same geometry unit the
    // parsed entry uses; the previous debug overflow is a hard failure.
    let (query, _) = load_case("cw03");
    let extremes = with_column_widths(query, &[u16::MAX, u16::MAX]);

    let rendered = std::panic::catch_unwind(AssertUnwindSafe(|| {
        mant_render::render_query_man(&extremes)
    }));
    let output = rendered.expect("cw14 handcrafted IR must not panic");
    let rows = actual_rows(&output);
    assert!(
        rows.iter()
            .all(|row| row.chars().count() <= mant_ir::geometry::MAX_COLUMN_ADVANCE + 64),
        "cw14 handcrafted IR produced unbounded rows: {rows:?}"
    );
}

#[test]
fn inbound_json_cannot_bypass_the_geometry_guard() {
    // The same extreme IR serialized as JSON and decoded back through
    // the protocol bundle must validate identically — the inbound JSON
    // path may not skip whatever guard the parsed entry gets.
    let (query, _) = load_case("cw03");
    let extremes = with_column_widths(query, &[u16::MAX, u16::MAX]);
    let json =
        mant_render::render_query_json(&extremes, false).expect("serialize extreme column widths");
    let bundle = serde_json::from_str::<mant_protocol::QueryBundle>(&json)
        .expect("decode actual extreme JSON text");

    let rendered = std::panic::catch_unwind(AssertUnwindSafe(|| {
        mant_render::render_query_man(&bundle.into())
    }));
    let output = rendered.expect("cw14 actual inbound JSON must not panic");
    let rows = actual_rows(&output);
    assert!(
        rows.iter()
            .all(|row| row.chars().count() <= mant_ir::geometry::MAX_COLUMN_ADVANCE + 64),
        "cw14 inbound JSON produced unbounded rows: {rows:?}"
    );
}

#[test]
fn declared_column_output_cost_stays_bounded_with_scale() {
    // Review §25.2 CW18: 1/10/100/1000 rows with a declared width must
    // stay near-linear and bounded — no per-row padding explosion.
    for rows in [1_usize, 10, 100, 1000] {
        let mut source = String::from(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n\
             .Bl -column \"12345678\" \"b\"\n",
        );
        for index in 0..rows {
            let _ = writeln!(source, ".It a{index} Ta tail");
        }
        source.push_str(".El\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes())
            .unwrap_or_else(|error| panic!("cw18 {rows}: lower case: {error}"));
        let rendered = mant_render::render_query_man(&query);
        let projected = actual_rows(&rendered);
        // One furniture row (the DESCRIPTION heading) plus one per item.
        assert_eq!(projected.len(), rows + 1, "cw18 {rows}: row count drifted");
        assert!(
            rendered.len() <= rows.saturating_mul(64) + 512,
            "cw18 {rows}: output cost {} unbounded",
            rendered.len()
        );
    }
}

#[test]
fn column_declarations_keep_the_same_contract_in_actual_json_text() {
    let (query, _) = load_case("cw03");
    for widths in [
        vec![],
        vec![0],
        vec![1, 8, 9],
        vec![u16::MAX; 2],
        vec![u16::MAX; 257],
    ] {
        let candidate = with_column_widths(query.clone(), &widths);
        let before = mant_render::render_query_man(&candidate);
        let json = mant_render::render_query_json(&candidate, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let decoded: mant_ir::ResolvedContent = decoded.into();
        assert_eq!(mant_render::render_query_man(&decoded), before);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &mant_render::render_query_json(&decoded, false).unwrap()
            )
            .unwrap(),
            serde_json::from_str::<serde_json::Value>(&json).unwrap()
        );
        assert!(before.contains("ABCDEFGHIJKLM"));
        assert!(before.contains("SECOND"));
        assert!(!before.contains("ABCDEFGHIJKLMSECOND"));
        assert!(before.len() < 1024);
    }
}

#[test]
fn inbound_column_widths_reject_values_outside_the_serialized_cell_range() {
    let (query, _) = load_case("cw03");
    let json = mant_render::render_query_json(&query, false).unwrap();
    for invalid in [
        serde_json::json!([-1]),
        serde_json::json!([65536]),
        serde_json::json!([1.5]),
        serde_json::json!([null]),
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!("1"),
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut pending = vec![&mut value];
        let mut changes = 0;
        while let Some(value) = pending.pop() {
            match value {
                serde_json::Value::Object(object) => {
                    if let Some(widths) = object.get_mut("columnWidths") {
                        *widths = invalid.clone();
                        changes += 1;
                    } else {
                        pending.extend(object.values_mut());
                    }
                }
                serde_json::Value::Array(values) => pending.extend(values),
                _ => {}
            }
        }
        assert_eq!(changes, 1);
        assert!(serde_json::from_str::<mant_protocol::QueryBundle>(&value.to_string()).is_err());
    }
}
