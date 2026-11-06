//! Normal It BODY names share native fields instead of detached interpreters.
use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind};
use mant_ir::{Block, HeadBodyRelation, Inline, ResolvedContent, inline_plain_text};

#[path = "../support/name_body_handoffs.rs"]
mod fixture;

#[path = "name_body_handoffs/origins.rs"]
mod origins;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

fn load(source: &str) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let restored = json_string_roundtrip(&original);
    assert_eq!(original, restored);
    assert_eq!(
        mant_ir::validate_document(restored.document.as_ref().unwrap()),
        []
    );
    restored
}

fn synopsis(content: &ResolvedContent) -> &[Block] {
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

fn rows(content: &ResolvedContent) -> Vec<String> {
    mant_render::render_query_man(content)
        .split_once("SYNOPSIS\n")
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0
        .strip_suffix("\n\n")
        .unwrap()
        .split('\n')
        // Match the pristine recorder's removal of device edge padding;
        // every leading/internal cell and every hard row stays exact.
        .map(|row| row.trim_end_matches(' ').to_owned())
        .collect()
}

fn native_rows(case: &fixture::Case) -> Vec<String> {
    // Inset's generated fixed word is a U+00A0 native glyph. The selected
    // definition seam projects its surviving separator as one reading SP;
    // row boundaries and source-authored blanks remain independent facts.
    case.oracle
        .rows
        .iter()
        .map(|row| row.replace('\u{a0}', " ").trim_end_matches(' ').to_owned())
        .collect()
}

fn ordinary_body_contains_name(node: &Node) -> bool {
    (node.kind == NodeKind::Body
        && node.macro_token == Some(MacroToken::Mdoc(MdocMacro::It))
        && node.children.iter().any(|child| {
            child.kind == NodeKind::Block
                && child.macro_token == Some(MacroToken::Mdoc(MdocMacro::Nm))
        }))
        || node.children.iter().any(ordinary_body_contains_name)
}

fn line_breaks(children: &[Inline]) -> usize {
    children
        .iter()
        .map(|child| match child {
            Inline::LineBreak { .. } => 1,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => line_breaks(children),
            _ => 0,
        })
        .sum()
}

#[test]
fn ordinary_name_body_consumes_the_outer_item_row_once() {
    // Exact sources and all five pristine profiles precede these assertions.
    // termp_it_pre(BODY) assigns offset, termp_nm_head/post consumes the same
    // tcol buffer, then Nm BODY adds its name width before the final flush.
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| case.cohort == "core"),
        |case| &case.id,
        |case| {
            let tree = libmandoc_rs::Parser::default()
                .parse_bytes("name-body.1", case.source.as_bytes())
                .unwrap();
            assert!(
                ordinary_body_contains_name(&tree.document.root),
                "{}: actual It BODY",
                case.id
            );
            let content = load(&case.source);
            assert_eq!(
                rows(&content),
                native_rows(case),
                "{}: CLI78 exact hard rows/origins",
                case.id
            );
            let [Block::DefinitionList { items, .. }] = synopsis(&content) else {
                panic!("one outer list")
            };
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(
                item.head_body_relation,
                if case.id == "core-inset" {
                    HeadBodyRelation::joined()
                } else {
                    HeadBodyRelation::separated()
                }
            );
            assert_eq!(inline_plain_text(&item.terms[0].content), "key");
            let [
                Block::Paragraph {
                    children, layout, ..
                },
            ] = item.description.as_slice()
            else {
                panic!("one active BODY owner")
            };
            assert_eq!(
                layout.continuation_indent_columns, 6,
                "{}: resolved Nm BODY wrap origin",
                case.id
            );
            assert_eq!(line_breaks(children), usize::from(case.id == "core-inset"));
            assert_eq!(inline_plain_text(children).matches("probe").count(), 1);
            assert_eq!(inline_plain_text(children).matches("int").count(), 1);
        },
    );
}

#[test]
fn normal_names_keep_exact_logical_rows_across_state_and_owner_boundaries() {
    verify_cases(
        fixture::cases(),
        |case| &case.id,
        |case| {
            let content = load(&case.source);
            assert_eq!(
                rows(&content),
                native_rows(case),
                "{}: native physical rows",
                case.id
            );
        },
    );
}

#[test]
fn ordinary_definition_names_preserve_hard_rows_in_two_markdown_readbacks() {
    verify_cases(
        fixture::cases()
            .into_iter()
            .filter(|case| case.cohort == "core"),
        |case| &case.id,
        |case| {
            let mut content = load(&case.source);
            let expected = if case.id == "core-inset" {
                vec!["key probe", "int"]
            } else {
                vec!["key probe int"]
            };
            for round in 1..=2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = mant_loader::load_markdown_text(&markdown, None).unwrap();
                // Definition export is an ordinary Markdown list. Inspect
                // that item's content, not its newly authored bullet marker.
                let [Block::List { items, .. }] = synopsis(&content) else {
                    panic!("one imported list")
                };
                assert_eq!(items.len(), 1);
                let [Block::Paragraph { children, .. }] = items[0].blocks.as_slice() else {
                    panic!("one imported hard-row paragraph")
                };
                let actual = inline_plain_text(children)
                    .split('\n')
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{}/round{round}\n{markdown}", case.id);
            }
        },
    );
}
