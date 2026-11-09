//! Mode, incoming NOSPACE and live input cells retain independent histories.
use mant_ir::ResolvedContent;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

#[path = "word_spacing/marker_blanks.rs"]
mod marker_blanks;
#[path = "word_spacing/cases.rs"]
mod recorded;

fn roundtrip(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let decoded = json_string_roundtrip(&content);
    assert_eq!(content.document, decoded.document);
    decoded
}

fn rows(content: &ResolvedContent) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim_start_matches(' ') == "ORIGIN")
        .unwrap();
    let end = lines
        .iter()
        .position(|line| line.trim_start_matches(' ') == "TAIL")
        .unwrap();
    let origin = lines[start].find("ORIGIN").unwrap();
    let prefix = " ".repeat(origin);
    lines[start..=end]
        .iter()
        .map(|line| {
            line.strip_prefix(prefix.as_str())
                .unwrap_or(line)
                .to_string()
        })
        .collect()
}

#[test]
fn empty_words_keep_their_executed_nospace_across_mode_changes_and_owners() {
    // Every exact source ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // term_word() consumes incoming NOSPACE before updating it from the
    // current NONOSPACE mode, even for zero-cell operands (term.c:573-589).
    // Sm only clears NOSPACE when its final mode is on AND p->col is nonzero
    // (mdoc_term.c::termp_sm_pre); projection drains are not native flushes.
    verify_cases(
        recorded::cases(),
        |case| &case.id,
        |case| {
            assert_eq!(rows(&roundtrip(&case.source)), case.rows, "{}", case.id);
        },
    );
}

#[test]
fn spacing_core_and_controls_survive_two_real_markdown_readbacks() {
    verify_cases(
        recorded::cases(),
        |case| &case.id,
        |case| {
            if case.cohort == "risk" {
                return;
            }
            let expected = case.markdown_rows();
            let mut content = roundtrip(&case.source);
            for round in 1..=2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = mant_loader::load_markdown_text(&markdown, None).unwrap();
                assert_eq!(
                    rows(&content),
                    expected,
                    "{} round {round}: {markdown}",
                    case.id
                );
            }
        },
    );
}

#[test]
fn zero_cell_words_and_real_cells_have_opposite_on_results() {
    for (id, expected) in [
        ("core-empty-on-text", "SECOND"),
        ("core-bold-on-text", "SECOND"),
        ("core-previous-on-text", "SECOND"),
        ("core-bare-z-on-text", "ECOND"),
        ("core-z-c-on-text", "SECOND"),
        ("control-on-empty-on", " SECOND"),
        ("control-off-nbrzw-on", " SECOND"),
        ("control-off-visible-on", "FIRST SECOND"),
        ("control-off-size-on", " SECOND"),
        ("control-off-color-on", " SECOND"),
        ("control-on-empty-off", " SECOND"),
        ("control-on-visible-off", "FIRST SECOND"),
    ] {
        let case = recorded::cases()
            .into_iter()
            .find(|case| case.id == id)
            .unwrap();
        assert_eq!(case.rows, ["ORIGIN", expected, "TAIL"], "pristine {id}");
        assert_eq!(rows(&roundtrip(&case.source)), case.rows, "{id}");
    }
}

#[test]
fn a_direct_field_flush_preserves_nospace_while_a_real_newline_selects_it() {
    // These exact Fn sources ran all pristine profiles before the assertions.
    // termp_fn_pre's pretty tail uses term_flushln(), not term_newln(). The
    // latter alone sets NOSPACE (term.c:475-481); term_flushln's device-row
    // end and buffer retirement preserve the word register (233-253).
    for (identifier, row) in [
        ("flush-on-empty-on", "  SECOND"),
        ("flush-on-empty-br-on", " SECOND"),
        ("flush-off-empty-on", "SECOND"),
        ("flush-on-off-empty-on", "  SECOND"),
    ] {
        let case = recorded::cases()
            .into_iter()
            .find(|case| case.id == identifier)
            .unwrap();
        assert_eq!(case.rows, ["ORIGIN", "call();", row, "TAIL"]);
        assert_eq!(rows(&roundtrip(&case.source)), case.rows, "{identifier}");
    }
}
