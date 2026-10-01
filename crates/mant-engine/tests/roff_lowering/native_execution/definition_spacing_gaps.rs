// Copyright (c) 2010-2022, 2025, 2026 Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! Executed vertical spacing and the final HEAD/BODY word separation.
//!
//! Every complete source below ran pristine ASCII/UTF-8/HTML/tree/lint
//! before these expectations were frozen (`-Tlint` exit 0 for all). The
//! rows record the CVS reference output with the common five-column
//! manual page margin removed; the section's blank-row counts are exact
//! (`roff_term.c::roff_term_pre_sp()` runs one `term_vspace()` per requested
//! row after the leading `term_newln()`, term.c:489-497, and that flush's
//! own tail endline, term.c:250-253, closes an occupied row or completes
//! an empty one). Indentation and inter-word gap widths stay responsive
//! reading geometry, so the AFTER-bearing rows compare word tokens: a
//! glued pair collapses into one token, an ordinary boundary stays two.

use mant_ir::ResolvedContent;

#[derive(serde::Deserialize)]
struct Case {
    source: String,
    /// Rows before the AFTER row, left margin trimmed, blanks exact.
    prefix_rows: Vec<String>,
    /// Word tokens per row from the AFTER row to the end of the section.
    after_rows_words: Vec<Vec<String>>,
}

fn round_trip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    assert!(!json.contains("\\u0000mant:"));
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

fn description_rows(source: &str) -> Vec<String> {
    let query = round_trip(source);
    let text = mant_render::render_query_man(&query);
    let body = text.split_once("DESCRIPTION\n").unwrap().1;
    let mut rows: Vec<String> = body
        .lines()
        .take_while(|row| row.trim() != "NEXT")
        .map(|row| row.trim_matches(' ').to_owned())
        .collect();
    // Trailing blank rows belong to the section spacing, not the case.
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

fn assert_case(case: &Case) {
    let rows = description_rows(&case.source);
    let after_at = rows
        .iter()
        .position(|row| row.contains("AFTER"))
        .unwrap_or_else(|| panic!("no AFTER row for {}", case.source));
    let prefix: Vec<String> = rows[..after_at].to_vec();
    assert_eq!(prefix, case.prefix_rows, "prefix rows\n{}", case.source);
    let after_tokens: Vec<Vec<String>> = rows[after_at..]
        .iter()
        .map(|row| row.split_whitespace().map(str::to_owned).collect())
        .collect();
    assert_eq!(
        after_tokens, case.after_rows_words,
        "AFTER-bearing rows\n{}",
        case.source
    );
}

#[test]
fn executed_vertical_rows_count_endline_events_in_definition_heads() {
    // R2436 (repair guide RR03) with the V01-V05 families: each .sp row
    // is one endline; the leading term_newln()'s flush also ends one row
    // through its tail overrun rule after an in-field `\p` break already
    // closed the printed row, so `.sp 1` leaves two blank rows and
    // consecutive requests accumulate; a negative request banks debt that
    // only a real word clears; a fitting TAG row stays open under NOBREAK
    // and the first requested endline closes it without a blank.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/definition_spacing_gaps.json")).unwrap();
    assert_eq!(cases.len(), 22);
    for case in &cases {
        assert_case(case);
    }
}

#[test]
fn proved_hang_joins_do_not_gain_a_forced_gap() {
    // The 8n `D\p E` + `.br` negative (repair guide RR04): the reference
    // prints `E AFTERBodyWord` — the final HEAD row reached the body
    // column, so no separator remains. The proof must record the consumed
    // gap (minTermGapColumns == 0, also pinned by the auxiliary
    // join_proven_hang acceptance case); a blanket "always space out
    // run-in heads" fix would fail here. E and AFTER stay ordinary head
    // words separated on the shared row; whether the renderer already
    // reproduces the native AFTER/BodyWord glue is a separate, responsive
    // reading-geometry question this test does not pin.
    let src = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n\
.Sh DESCRIPTION\n.Bl -hang -width 8n\n.It Xo\n.No \"D\\p E\"\n.br\n.No AFTER\n.Xc\n\
.No BodyWord\n.El\n.Sh NEXT\n.No END\n";
    let query = round_trip(src);
    let item = &query.document.as_ref().unwrap().sections[1].blocks[0];
    let mant_ir::Block::DefinitionList { items, .. } = item else {
        panic!("definition list expected: {item:#?}")
    };
    assert_eq!(items[0].layout.min_term_gap_columns, 0);
    let rows = description_rows(src);
    let head = rows
        .iter()
        .find(|row| row.split_whitespace().any(|word| word == "E"))
        .unwrap();
    let words = head.split_whitespace().collect::<Vec<_>>();
    assert_eq!(words[..2], ["E", "AFTER"]);
}

#[test]
fn unproven_hang_gap_keeps_the_body_word_boundary() {
    // R2502 (repair guide RR04): `.br` in a HANG head leaves the final
    // HEAD row short of the body column, so the BODY word keeps an
    // ordinary readable boundary — never `AFTERBodyWord`.
    let src = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n\
.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo\n.No \"D \\p E\"\n.br\n.No AFTER\n.Xc\n\
.No BodyWord\n.El\n.Sh NEXT\n.No END\n";
    let rows = description_rows(src);
    let after_row = rows.iter().find(|row| row.contains("AFTER")).unwrap();
    assert_eq!(
        after_row.split_whitespace().collect::<Vec<_>>(),
        ["AFTER", "BodyWord"]
    );
}
