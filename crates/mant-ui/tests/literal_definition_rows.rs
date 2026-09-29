//! A run-in literal definition row keeps its authored spacing when the TUI
//! wraps it. The shared HEAD/BODY row wraps as characters (term.c buffers
//! literal rows cell by cell); a word wrap would collapse the authored
//! blank runs that the row machine preserved into the IR.
use mant_ir::ResolvedContent;
use mant_ui::DocumentView;

fn literal_view() -> DocumentView {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/literal-definition-gap.1");
    let document =
        mant_loader::parse_manual_source(&fixture).expect("parse literal definition fixture");
    DocumentView::new(&ResolvedContent {
        address: None,
        label: "literal-definition-gap".to_owned(),
        document: Some(document),
        tldr: None,
    })
}

fn row_texts(view: &DocumentView, width: u16) -> Vec<String> {
    view.render(width)
        .text
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn run_in_literal_definition_row_keeps_authored_spacing() {
    let view = literal_view();
    // The whole shared row fits: every authored blank run survives.
    let wide = row_texts(&view, 44);
    let shared = wide
        .iter()
        .find(|row| row.contains("ABCDEFGHIJ"))
        .expect("shared row renders");
    assert_eq!(
        shared.trim(),
        "X     1234567890    ABCDEFGHIJ    LAST",
        "no authored blank run collapses: {shared:?}"
    );
    // Below the row width, the literal wraps by character: the blank run
    // between ABCDEFGHIJ and LAST stays measurable on the wrapped rows.
    let narrow = row_texts(&view, 40);
    let joined = narrow.join("\n");
    assert!(
        joined.contains("ABCDEFGHIJ    LAS\n") || joined.contains("ABCDEFGHIJ    LAST"),
        "character wrap keeps the blank run: {narrow:?}"
    );
    assert!(
        joined.contains("ROW2    SPACE"),
        "unwrapped literal rows keep their spacing: {narrow:?}"
    );
}
