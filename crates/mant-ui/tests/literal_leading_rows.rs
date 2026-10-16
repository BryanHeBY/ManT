//! A deliberate roff reading exception, applied once before public identities.
//!
//! Native empty TEXT still executes `term_vspace()` in `man_term.c`/`mdoc_term.c`.
//! The registered pristine fixture retains that row; readingException records
//! the separately approved single-initial-empty-row policy. This is not a claim
//! that mandoc's output is incorrect, and Markdown authors do not opt into it.
use mant_ir::{Block, Inline, ResolvedContent, visit::Visit};
use mant_ui::DocumentView;
use serde_json::Value;

mod literal_leading_rows {
    pub(super) mod markdown;
    pub(super) mod navigation;
    pub(super) mod real;
    mod seams;
}

const SOURCES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/roff/literal_leading_rows"
);

fn cases() -> Vec<Value> {
    let matrix: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/roff/literal_leading_rows/cases.json"
    ))
    .unwrap();
    assert_eq!(matrix["expectationsFromProduct"], false);
    let cases = matrix["cases"].as_array().unwrap().clone();
    assert_eq!(cases.len(), 23);
    cases
}

fn load(case: &Value) -> ResolvedContent {
    let name = case["file"].as_str().unwrap();
    let source = std::fs::read(format!("{SOURCES}/{name}")).unwrap();
    mant_loader::load_roff_bytes(&source).unwrap()
}

fn restored(content: &ResolvedContent) -> ResolvedContent {
    let json = mant_render::render_query_json(content, false).unwrap();
    let restored: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&json)
        .unwrap()
        .into();
    assert_eq!(restored, *content, "actual JSON string roundtrip");
    assert_eq!(
        mant_ir::validate_document(restored.document.as_ref().unwrap()),
        Vec::<mant_ir::Diagnostic>::new()
    );
    restored
}

fn expected_rows(case: &Value) -> Vec<String> {
    let mut rows = case["native"]["bodyRows"]["utf8"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let drop = usize::try_from(
        case["readingException"]["dropLeadingEmptyRows"]
            .as_u64()
            .unwrap(),
    )
    .unwrap();
    rows.drain(..drop);
    match case["id"].as_str().unwrap() {
        "nf_authored_spaces" => rows[0] = "  ".into(),
        // This literal source authors U+0009; IR, CLI and Markdown retain it.
        // The TUI expands that scalar relative to its viewport row origin.
        "ex_authored_tab" => rows[0] = "\t".into(),
        "nf_independent_literal_owners" => {
            rows = vec![
                "BLOCK_A".into(),
                String::new(),
                "BETWEEN".into(),
                "BLOCK_B".into(),
            ];
        }
        _ => {}
    }
    rows
}

fn body_rows(rows: &[String]) -> Vec<String> {
    let before = rows
        .iter()
        .position(|row| row.ends_with("BEFORE"))
        .expect("BEFORE row");
    let after = rows
        .iter()
        .enumerate()
        .skip(before + 1)
        .find(|(_, row)| row.ends_with("AFTER"))
        .unwrap()
        .0;
    let prefix = rows[before].strip_suffix("BEFORE").unwrap();
    assert!(prefix.chars().all(|character| character == ' '));
    rows[before + 1..after]
        .iter()
        .map(|row| row.strip_prefix(prefix).unwrap_or(row).to_owned())
        .collect()
}

fn plain_rows(content: &ResolvedContent) -> Vec<String> {
    mant_render::render_query_man(content)
        .split('\n')
        .map(str::to_owned)
        .collect()
}

fn literal_texts(content: &ResolvedContent) -> Vec<String> {
    struct Literals(Vec<String>);
    impl<'ir> Visit<'ir> for Literals {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::Preformatted { children, .. } = block {
                self.0.push(mant_ir::inline_plain_text(children));
            }
            mant_ir::visit::walk_block(self, block);
        }
    }
    let mut visitor = Literals(vec![]);
    visitor.visit_document(content.document.as_ref().unwrap());
    visitor.0
}

fn terminal_tabs(value: &str, origin: usize) -> String {
    let mut column = origin;
    let mut output = String::new();
    for glyph in mant_render::cells::graphemes(value) {
        if glyph.text() == "\t" {
            let width = 8 - column % 8;
            output.push_str(&" ".repeat(width));
            column += width;
        } else {
            output.push_str(glyph.text());
            column += glyph.columns();
        }
    }
    output
}

#[test]
fn complete_roff_owners_share_exact_rows_after_json_and_consumer_projection() {
    // All 23 exact files ran pinned pristine UTF-8 before these assertions;
    // cases.json also records all four original reference profiles. Never
    // collapse whitespace or infer gold from the candidate's normalized IR.
    for case in cases() {
        let expected = expected_rows(&case);
        let loaded = load(&case);
        let content = restored(&loaded);
        for value in [&loaded, &content] {
            let plain = plain_rows(value);
            assert_eq!(body_rows(&plain), expected, "{} plain", case["id"]);
            let decorated = mant_render::render_query_text_with(value, |_, text| {
                format!("\x1b[1m{text}\x1b[0m")
            });
            let stripped = decorated.replace("\x1b[1m", "").replace("\x1b[0m", "");
            assert_eq!(
                body_rows(&stripped.split('\n').map(str::to_owned).collect::<Vec<_>>()),
                expected,
                "{} ANSI",
                case["id"]
            );
            for width in [80, 120] {
                let rendered = DocumentView::new(value).render(width);
                let rows = rendered
                    .text
                    .lines
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                let actual = body_rows(&rows);
                assert_eq!(
                    actual.len(),
                    expected.len(),
                    "{} TUI {width}: {actual:?}",
                    case["id"]
                );
                let origin = rows
                    .iter()
                    .find_map(|row| row.strip_suffix("BEFORE"))
                    .unwrap()
                    .len();
                for (actual, expected) in actual.iter().zip(&expected) {
                    let expected = terminal_tabs(expected, origin);
                    let padding = actual.strip_prefix(&expected).unwrap_or_else(|| {
                        panic!(
                            "{} TUI {width}: expected {expected:?}, actual {actual:?}",
                            case["id"]
                        )
                    });
                    assert!(
                        padding.chars().all(|character| character == ' '),
                        "{} TUI non-surface suffix {padding:?}",
                        case["id"]
                    );
                }
            }
        }
        literal_leading_rows::markdown::assert_roff_readback(&case, &content, &expected);
    }
}
