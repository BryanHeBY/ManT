//! A native BODY origin survives repeated leading source rows and IR drains.
use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind};
use mant_ir::{Block, Inline, ResolvedContent, inline_plain_text};

#[path = "origins/fixture.rs"]
mod fixture;

use super::{load, rows, synopsis};

fn name_block(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token == Some(MacroToken::Mdoc(MdocMacro::Nm)) {
        Some(node)
    } else {
        node.children.iter().find_map(name_block)
    }
}

fn assert_native_owner(content: &ResolvedContent, case: &fixture::Case) {
    let [
        Block::Paragraph {
            children,
            inline_layout,
            layout,
            source,
        },
    ] = synopsis(content)
    else {
        panic!("{}: one complete Nm owner", case.id)
    };
    // termp_nm_pre(BODY) adds width("probe") + 1 = six columns. Both
    // hard-row origins and visual-wrap origins retain this resolved fact;
    // an IR drain must not lose it or add it twice to a prior row hint.
    assert_eq!(layout.indent_columns, 0, "{}", case.id);
    assert_eq!(layout.continuation_indent_columns, 6, "{}", case.id);
    let source = source.expect("original Nm source owner");
    assert_eq!((source.line, source.column), (8, 2), "{}", case.id);
    assert_eq!(
        inline_plain_text(children),
        case.portable_rows().join("\n"),
        "{}: authored leading SP remains in text, not row hints",
        case.id
    );
    for row in 1..case.native_rows.len() {
        let hint = inline_layout.row_layout(row);
        assert_eq!(
            layout.indent_columns
                + layout.continuation_indent_columns
                + hint.indent_columns
                + hint.first_line_indent_columns,
            6,
            "{}/{row}: final hard-row origin before its authored SP",
            case.id
        );
    }
    assert!(matches!(&children[0], Inline::Strong { .. }));
    assert!(
        children
            .iter()
            .any(|node| matches!(node, Inline::Emphasis { .. }))
    );
}

#[test]
fn name_body_origins_keep_source_owners_authored_blanks_and_continuation_layout() {
    // These two exact sources ran pristine ASCII/UTF-8 at widths 40 and 78,
    // plus HTML/tree/lint, before these assertions. print_mdoc_node() restores
    // offset after source text; Nm BODY's own offset persists until its post
    // (mdoc_term.c:354-378,437-439,966-1040). Neither second-row commit nor
    // an IR-owner drain may overwrite an already accepted six-column origin.
    for case in fixture::cases() {
        let ast = libmandoc_rs::Parser::default()
            .parse_bytes("name-body-origin.1", case.source.as_bytes())
            .unwrap();
        let name = name_block(&ast.document.root).expect("actual Nm BLOCK");
        let body = name
            .children
            .iter()
            .find(|node| node.kind == NodeKind::Body)
            .unwrap();
        let leading = body
            .children
            .iter()
            .filter(|node| node.kind == NodeKind::Text)
            .collect::<Vec<_>>();
        assert_eq!(leading.len(), case.native_rows.len() - 1, "{}", case.id);
        for (offset, node) in leading.iter().enumerate() {
            assert!(node.flags.line_start);
            assert_eq!(
                (node.line, node.column),
                (10 + u32::try_from(offset).unwrap(), 1)
            );
            assert_eq!(node.text.as_deref(), Some(case.portable_rows()[offset + 1]));
            assert_eq!(
                case.source.lines().nth(9 + offset),
                node.text.as_deref(),
                "{}: original authored source slice",
                case.id
            );
        }
        let content = load(&case.source);
        assert_native_owner(&content, &case);
        assert_eq!(rows(&content), case.native_rows, "{}: native rows", case.id);
        for width in [40, 78] {
            let output = mant_render::render_query_man_at_width(&content, width);
            for expected in &case.native_rows {
                assert!(
                    output.lines().any(|line| line == expected),
                    "{}/{width}",
                    case.id
                );
            }
        }
    }
}

#[test]
fn name_body_authored_leading_spaces_survive_two_actual_markdown_reads() {
    for case in fixture::cases() {
        let mut content = load(&case.source);
        for round in 1..=2 {
            let markdown = mant_codec::encode::render_markdown(&content);
            content = super::super::json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
            let [Block::Paragraph { children, .. }] = synopsis(&content) else {
                panic!("{}: one portable hard-row owner", case.id)
            };
            assert_eq!(
                inline_plain_text(children),
                case.portable_rows().join("\n"),
                "{}/read{round}: geometry may simplify, authored SP and hard rows cannot",
                case.id
            );
        }
    }
}
