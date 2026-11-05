//! Local recovered cells enter and leave the complete native Nm field session.
use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind};
use mant_ir::{Block, Inline, InlineLayout, ResolvedContent, RowLayoutHint, inline_plain_text};

#[path = "../support/name_field_sessions.rs"]
mod fixture;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

fn load(source: &str) -> ResolvedContent {
    let source = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&source);
    assert_eq!(source, content);
    assert_eq!(
        mant_ir::validate_document(content.document.as_ref().unwrap()),
        []
    );
    content
}

fn section(content: &ResolvedContent) -> &[Block] {
    &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")
        .unwrap()
        .blocks
}

fn first_cell(content: &ResolvedContent) -> (&[Inline], &InlineLayout) {
    let [Block::Table { rows, .. }] = section(content) else {
        panic!("one recovered table")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 1);
    let [
        Block::Paragraph {
            children,
            inline_layout,
            ..
        },
    ] = rows[0].cells[0].blocks.as_slice()
    else {
        panic!("one complete local inline owner")
    };
    (children, inline_layout)
}

fn rows(content: &ResolvedContent) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    text.split_once("SYNOPSIS\n")
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0
        .strip_suffix("\n\n")
        .unwrap()
        .split('\n')
        .map(str::to_owned)
        .collect()
}

fn genuine_name(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block
        && node.macro_token == Some(MacroToken::Mdoc(MdocMacro::Nm))
        && node
            .children
            .iter()
            .any(|child| child.kind == NodeKind::Body && !child.children.is_empty())
    {
        return Some(node);
    }
    node.children.iter().find_map(genuine_name)
}

#[test]
fn all_head_widths_and_genuine_body_kinds_share_the_native_field_lifecycle() {
    // Exact whole tables AND independent fragments ran all five pristine
    // profiles first. Whole-table lint 4 means unsupported tbl syntax; the
    // selected enhancement follows the clean independent Nm BLOCK/HEAD/BODY.
    // termp_nm_pre/post install and retire the field; termp_sh_post(BODY)
    // executes the final term_newln, not a second projection-owner flush.
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| case.independent.is_some()),
        |case| &case.id,
        |case| {
            let expected = case.expected();
            let content = load(&case.source);
            let (children, inline_layout) = first_cell(&content);
            let scalar_rows = if case.id == "risk-enclosure" {
                // Nm BODY's NOSPACE word is "int"; termp_nm_pre adds six
                // columns to offset, not six source cells to term_word.
                // Keep the native row and its sparse layout fact separate.
                assert_eq!(expected.rows, ["[probe", "      int", "]"]);
                assert_eq!(
                    inline_layout.row_hints,
                    [RowLayoutHint {
                        row: 1,
                        indent_columns: 6,
                        ..RowLayoutHint::default()
                    }]
                );
                vec!["[probe".to_owned(), "int".to_owned(), "]".to_owned()]
            } else {
                expected.rows.clone()
            };
            assert_eq!(
                inline_plain_text(children),
                scalar_rows.join("\n"),
                "{}: exact accepted cell",
                case.id
            );
            assert_eq!(rows(&content), expected.rows, "{}: exact CLI rows", case.id);
            if case.cohort == "core" {
                let parsed = libmandoc_rs::Parser::default()
                    .parse_bytes(
                        "nm-session.1",
                        case.independent_source.as_ref().unwrap().as_bytes(),
                    )
                    .unwrap();
                let name = genuine_name(&parsed.document.root).unwrap();
                assert!(name.children.iter().any(|node| node.kind == NodeKind::Head));
                assert!(
                    name.children
                        .iter()
                        .any(|node| node.kind == NodeKind::Body && !node.children.is_empty()),
                    "{}: actual BODY, not same-line HEAD words",
                    case.id
                );
            }
        },
    );
}

#[test]
fn bounded_session_words_and_hard_rows_survive_two_real_markdown_roundtrips() {
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| case.independent.is_some()),
        |case| &case.id,
        |case| {
            let mut content = load(&case.source);
            for round in 1..=2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = mant_loader::load_markdown_text(&markdown, None).unwrap();
                assert_eq!(
                    rows(&content),
                    case.expected().rows,
                    "{}: Markdown round {round}\n{markdown}",
                    case.id
                );
            }
        },
    );
}

#[test]
fn existing_enclosing_definition_heads_and_function_fields_are_not_replaced_by_cell_sessions() {
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| case.cohort == "nested" && !case.id.starts_with("nested-body-")),
        |case| &case.id,
        |case| {
            assert_eq!(rows(&load(&case.source)), case.whole.rows, "{}", case.id);
        },
    );
}

#[test]
fn each_adjacent_or_following_cell_starts_one_complete_session_and_keeps_its_owner() {
    // tbl_term.c clears BACKBEFORE/BACKAFTER at each data cell. The exact
    // whole and two independent sources ran five pristine profiles first;
    // the local recovered field cannot retain a previous cell's live input.
    verify_cases(
        fixture::cell_controls(),
        |case| &case.id,
        |case| {
            let content = load(&case.source);
            let [Block::Table { rows, .. }] = section(&content) else {
                panic!("one table")
            };
            assert_eq!(rows.len(), if case.adjacent { 1 } else { 2 });
            let cells = rows.iter().flat_map(|row| &row.cells).collect::<Vec<_>>();
            assert_eq!(cells.len(), 2);
            for (index, (cell, expected)) in cells.iter().zip(&case.independent).enumerate() {
                let [Block::Paragraph { children, .. }] = cell.blocks.as_slice() else {
                    panic!("one cell owner")
                };
                assert_eq!(
                    inline_plain_text(children),
                    expected.rows.join("\n"),
                    "{} cell{index}",
                    case.id
                );
            }
        },
    );
}
