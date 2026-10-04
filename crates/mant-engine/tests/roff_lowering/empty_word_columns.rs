//! Empty native buffers still execute newline flags before the next word.
use libmandoc_rs::{Node, NodeKind};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, ResolvedContent};
use serde::Deserialize;

#[derive(Deserialize)]
struct Matrix {
    header: Header,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Header {
    count: usize,
    unique_sources: usize,
    asserted_row_count: usize,
    uncovered_row_count: usize,
    expectations_from_product: bool,
    oracle_sha256: String,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    source_sha256: String,
    ordinary_body_owners: Vec<Owner>,
    expected_rows: Vec<String>,
    body_positions: Vec<Position>,
    row_observation: String,
}

#[derive(Deserialize)]
struct Owner {
    label: String,
    line: u32,
    column: u32,
    line_start: bool,
    no_fill: bool,
    #[serde(rename = "owner")]
    list_context: Option<String>,
    ancestors: Vec<(String, String, u32)>,
}

#[derive(Deserialize)]
struct Position {
    value: String,
    row: usize,
    column: usize,
    prefix: String,
}

fn cases() -> Vec<Case> {
    let matrix: Matrix =
        serde_json::from_str(include_str!("empty_word_columns/cases.json")).unwrap();
    let header = matrix.header;
    assert_eq!((header.count, header.unique_sources), (472, 472));
    assert_eq!(
        (header.asserted_row_count, header.uncovered_row_count),
        (460, 12)
    );
    assert!(!header.expectations_from_product);
    assert_eq!(
        header.oracle_sha256,
        "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6"
    );
    assert_eq!(matrix.cases.len(), header.count);
    matrix.cases
}

fn owns_body(node: &Node, in_description: bool, owner: &Owner) -> bool {
    let in_description = in_description
        || (node.kind == NodeKind::Body
            && node.macro_token.as_deref() == Some("Sh")
            && node.line == 7);
    (in_description
        && node.kind == NodeKind::Text
        && node.line == owner.line
        && node.column == owner.column
        && node.text.as_deref() == Some(owner.label.as_str())
        && node.flags.line_start == owner.line_start
        && node.flags.no_fill == owner.no_fill)
        || node
            .children
            .iter()
            .any(|child| owns_body(child, in_description, owner))
}

fn assert_ast(case: &Case) {
    let parsed = libmandoc_rs::Parser::default()
        .parse_bytes("empty-word-columns.1", case.source.as_bytes())
        .unwrap();
    assert_eq!(case.source_sha256.len(), 64);
    assert!(!case.ordinary_body_owners.is_empty(), "{}", case.id);
    for owner in &case.ordinary_body_owners {
        assert!(
            owner.list_context.is_none(),
            "{}: ordinary text became It content",
            case.id
        );
        assert!(owner.ancestors.contains(&("Sh".into(), "body".into(), 7)));
        assert!(
            owns_body(&parsed.document.root, false, owner),
            "{}: BODY owner",
            case.id
        );
    }
}

fn roundtrip(case: &Case) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let text = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
    assert!(!text.contains("\\u0000mant:"), "{}", case.id);
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&text).unwrap();
    let decoded = ResolvedContent::from(decoded);
    assert_eq!(decoded, content, "{}: real JSON", case.id);
    decoded
}

fn rows(rendered: &str) -> Vec<String> {
    let region = rendered
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0;
    let mut rows: Vec<_> = region.split_terminator('\n').map(str::to_owned).collect();
    // Remove only NEXT's fixed section-spacing row. Author blank rows, SP,
    // NBSP, combining characters and every physical line remain observable.
    assert_eq!(rows.pop().as_deref(), Some(""));
    rows
}

fn description(content: &ResolvedContent) -> &[Block] {
    &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
        .blocks
}

fn markdown_payloads(content: &ResolvedContent, exporting: bool) -> Vec<String> {
    description(content)
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => {
                let raw = mant_ir::inline_plain_text(children);
                let value = if exporting {
                    // Accepted children are the only visible body. Paragraph
                    // edge padding remains responsive. This
                    // explicit encoder contract does not trim authored NBSP or LF.
                    raw.split('\n')
                        .map(|row| row.trim_matches([' ', '\t']))
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    raw
                };
                (!value.is_empty()).then_some(value)
            }
            Block::Preformatted { children, .. } => Some(mant_ir::inline_plain_text(children)),
            // Markdown has no vertical layout primitive. Such omissions are a
            // separate declared axis; executed inline hard rows above stay exact.
            Block::VerticalSpace { .. } => None,
            other => panic!("unexpected ordinary source container: {other:#?}"),
        })
        .collect()
}

fn assert_columns(case: &Case, actual: &[String]) {
    for position in &case.body_positions {
        let row = &actual[position.row];
        let start = row.find(&position.value).unwrap();
        assert_eq!(&row[..start], position.prefix, "{}: exact prefix", case.id);
        assert_eq!(
            mant_ir::geometry::text_width(&row[..start]),
            position.column,
            "{}: native display column",
            case.id
        );
    }
}

#[test]
fn empty_buffer_newlines_preserve_exact_native_rows_and_real_json_columns() {
    // Every complete source ran pristine ASCII/UTF-8/HTML/tree/lint before
    // writing this fixture. term.c::term_newln (475-481) sets NOSPACE before
    // checking lastcol/viscol; term_word (573-589) then writes at most the
    // executed automatic blank. No final field or device-padding guess is used.
    let mut failures = Vec::new();
    let mut asserted = 0;
    for case in cases() {
        assert_ast(&case);
        let content = roundtrip(&case);
        if case.row_observation != "asserted" {
            continue;
        }
        asserted += 1;
        let actual = rows(&mant_render::render_query_man(&content));
        if actual == case.expected_rows {
            assert_columns(&case, &actual);
        } else {
            failures.push(format!(
                "{}: expected {:?}, got {:?}",
                case.id, case.expected_rows, actual
            ));
        }
    }
    assert_eq!(asserted, 460);
    assert!(
        failures.is_empty(),
        "{} exact-column failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn native_markdown_readback_preserves_literal_columns_and_phrasing_hard_rows() {
    let mut failures = Vec::new();
    for case in cases()
        .into_iter()
        .filter(|case| case.row_observation == "asserted")
    {
        let content = roundtrip(&case);
        let markdown = render_markdown_with_options(&content, MarkdownOptions::default());
        let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let expected = markdown_payloads(&content, true);
        let actual = markdown_payloads(&decoded, false);
        if expected != actual {
            failures.push(format!(
                "{}: expected {expected:?}, got {actual:?}\n{markdown}",
                case.id
            ));
        }
        assert!(!markdown.contains('\u{0000}'));
    }
    assert!(
        failures.is_empty(),
        "{} reader payload failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
