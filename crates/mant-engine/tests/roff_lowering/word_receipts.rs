//! Exact source visits and portable rows across word-cell ownership.

use flate2::read::GzDecoder;
use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::visit::{self, Visit};
use mant_ir::{Inline, LinkTarget, inline_plain_text};
use serde::Deserialize;

use super::{case_failures::verify_cases, json_roundtrip::json_string_roundtrip};

#[derive(Deserialize)]
struct Matrix {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    comparison: Comparison,
    rows: Vec<String>,
    tree_words: Vec<TreeWord>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Comparison {
    Exact,
    DeviceTab,
    DeviceField,
    NativeField,
    ReadingOneInitialRow,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct TreeWord(u32, u32, String, String, String, bool, bool);

#[derive(Default)]
struct Labels(Vec<(String, String)>);

impl<'a> Visit<'a> for Labels {
    fn visit_inline(&mut self, node: &'a Inline) {
        if let Inline::Link {
            target: LinkTarget::External { uri },
            children,
            ..
        } = node
        {
            self.0.push((uri.clone(), inline_plain_text(children)));
        }
        visit::walk_inline(self, node);
    }
}

fn text_visits(node: &Node, start: u32, words: &mut Vec<TreeWord>) {
    for child in &node.children {
        if child.kind == NodeKind::Text && child.line > start {
            let parent = node.macro_token.as_ref().map_or("", |token| token.as_str());
            let kind = match node.kind {
                NodeKind::Element => "elem",
                NodeKind::Block => "block",
                NodeKind::Head => "head",
                NodeKind::Body => "body",
                NodeKind::Tail => "tail",
                other => panic!("unexpected text parent {other:?}"),
            };
            words.push(TreeWord(
                child.line,
                child.column,
                child
                    .native_text
                    .as_ref()
                    .or(child.text.as_ref())
                    .unwrap()
                    .clone(),
                parent.to_owned(),
                kind.to_owned(),
                child.flags.line_start,
                child.flags.no_fill,
            ));
        }
        text_visits(child, start, words);
    }
}

fn description_rows<'a>(text: &'a str, source: &str) -> Vec<&'a str> {
    let (_, body) = text.split_once("\nDESCRIPTION\n").unwrap();
    let next_section = if source.contains("ENDTEST\n") {
        Some("\nENDTEST\n")
    } else if source.contains(".Sh END\n") {
        Some("\nEND\n")
    } else {
        None
    };
    if let Some(next_section) = next_section {
        let (body, _) = body.split_once(next_section).unwrap();
        // One inter-section layout row belongs to the surrounding document.
        body.strip_suffix('\n')
            .unwrap_or(body)
            .split('\n')
            .collect()
    } else {
        // A closed final row can have a terminator. lines() consumes only
        // the terminator and retains any preceding completed empty rows.
        body.lines().collect()
    }
}

fn tab_columns(row: &str) -> (&str, &str) {
    // The width of the tab interval immediately before the successor is a
    // terminal stop. It is separate from every other inter-word separator.
    let Some(index) = ["\u{a0}BETA", "⟨BETA", "BETA", "ETA"]
        .into_iter()
        .find_map(|word| row.find(word))
    else {
        return (row, "");
    };
    (row[..index].trim_end_matches([' ', '\t']), &row[index..])
}

fn field_spacing(row: &str) -> String {
    let Some(content) = row.strip_prefix("HEAD") else {
        return row.to_owned();
    };
    let width = content
        .chars()
        .take_while(|character| matches!(character, ' ' | '\u{a0}'))
        .count();
    let text = content.trim_start_matches([' ', '\u{a0}']);
    format!("HEAD{}{text}", " ".repeat(width))
}

fn native_field_content(row: &str) -> String {
    // These probes use HEAD and BODY as field boundaries. Ignore only
    // their device columns, leaving the complete text and spaces between
    // the probe operands in place.
    let row = row.trim_start_matches([' ', '\u{a0}']);
    let mut content = row.to_owned();
    if let Some(after) = row.strip_prefix("HEAD") {
        let word = after.trim_start_matches([' ', '\u{a0}']);
        if word != after {
            content = format!("HEAD {word}");
        }
    }
    if let Some(before) = content.strip_suffix("BODY") {
        let word = before.trim_end_matches([' ', '\u{a0}']);
        if word != before {
            content = format!("{word} BODY");
        }
    }
    content
}

fn fixture(bytes: &[u8]) -> Matrix {
    serde_json::from_reader(GzDecoder::new(bytes)).unwrap()
}

#[test]
fn word_receipts_match_the_pristine_text_visits_and_accepted_rows_after_json() {
    // term_word() writes separators before encode1() can retract a SP/TAB.
    // term_fill() stops an armed word-end break at ASCII_BREAK and the
    // ensuing term_flushln() emits the accepted row (term.c:289-305,
    // 573-589, 901-908, 205-220). The fixture seals all five profiles for
    // every exact source, independently of the IR under test.
    verify_matrix(fixture(include_bytes!("word_receipts/cases.json.gz")));
}

#[test]
fn word_projection_preserves_native_cell_identity_and_printed_separators() {
    // term.c::term_word() writes separators before encode1() establishes
    // the overwritten cell. term_fill() accepts buffer ranges, while
    // term_field() prints an advance only before an actual output cell.
    verify_matrix(fixture(include_bytes!("word_projection/cases.json.gz")));
}

fn verify_matrix(matrix: Matrix) {
    verify_cases(
        matrix.cases,
        |case| &case.id,
        |case| {
            let parsed = Parser::default()
                .parse_bytes("word-receipt.1", case.source.as_bytes())
                .unwrap();
            let section_line = case
                .source
                .lines()
                .position(|line| matches!(line, ".Sh DESCRIPTION" | ".SH DESCRIPTION"))
                .unwrap();
            let mut words = Vec::new();
            text_visits(
                &parsed.document.root,
                u32::try_from(section_line + 1).unwrap(),
                &mut words,
            );
            assert_eq!(words, case.tree_words, "native text nodes");

            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let transported = json_string_roundtrip(&original);
            let rendered = mant_render::render_query_man(&transported);
            let rows = description_rows(&rendered, &case.source);
            match case.comparison {
                Comparison::Exact => assert_eq!(rows, case.rows),
                Comparison::ReadingOneInitialRow => {
                    assert_eq!(case.rows, ["", "ALPHA"]);
                    assert_eq!(rows, &case.rows[1..]);
                }
                Comparison::DeviceTab => assert_eq!(
                    rows.iter().map(|row| tab_columns(row)).collect::<Vec<_>>(),
                    case.rows
                        .iter()
                        .map(|row| tab_columns(row))
                        .collect::<Vec<_>>()
                ),
                Comparison::DeviceField => assert_eq!(
                    rows.iter()
                        .map(|row| field_spacing(row))
                        .collect::<Vec<_>>(),
                    case.rows
                        .iter()
                        .map(|row| field_spacing(row))
                        .collect::<Vec<_>>()
                ),
                Comparison::NativeField => assert_eq!(
                    rows.iter()
                        .map(|row| native_field_content(row))
                        .collect::<Vec<_>>(),
                    case.rows
                        .iter()
                        .map(|row| native_field_content(row))
                        .collect::<Vec<_>>()
                ),
            }
        },
    );
}

#[test]
fn source_label_can_represent_a_surviving_separator_until_the_word_rearms() {
    // These exact sources ran all five profiles. term_word() emits an
    // automatic separator before the authored operand. When encode1()
    // retracts the lead blank, its surviving equivalent can stay styled
    // inside the label. A second \z instead overstrikes B with E.
    let cases = fixture(include_bytes!("word_receipts/cases.json.gz")).cases;
    for (id, expected) in [
        ("W5/link-authored-space", " BETA"),
        ("W5/link-rearm", "ETA"),
    ] {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap());
        let mut labels = Labels::default();
        labels.visit_document(content.document.as_ref().unwrap());
        assert_eq!(labels.0, [("https://x.io".into(), expected.into())], "{id}");
    }
}
