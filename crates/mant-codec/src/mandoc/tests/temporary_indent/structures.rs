//! Actual node roles and delayed HEAD/BODY presentation receipts.

use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind, Parser};
use mant_ir::{HeadBodyRelation, LayoutHint};
use serde::Deserialize;

use super::{Block, Case, body, inline_text, parse, root_word_column, source_digest, word_column};

#[derive(Deserialize)]
struct StructuralCase {
    #[serde(flatten)]
    basic: Case,
    definition: bool,
    relation: Option<String>,
    body_column: Option<i32>,
}

fn item_head(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Head && node.macro_token == Some(MacroToken::Mdoc(MdocMacro::It)) {
        return Some(node);
    }
    node.children.iter().find_map(item_head)
}

fn has_text(node: &Node, text: &str) -> bool {
    node.text.as_deref() == Some(text) || node.children.iter().any(|child| has_text(child, text))
}

fn description_word_column(blocks: &[Block], word: &str, parent: i32) -> Option<i32> {
    blocks.iter().find_map(|block| match block {
        Block::Paragraph {
            children,
            inline_layout,
            layout,
            ..
        }
        | Block::Preformatted {
            children,
            inline_layout,
            layout,
            ..
        } => {
            let mut layout = *layout;
            layout.indent_columns += parent - 5;
            root_word_column(&inline_text(children), inline_layout, layout, word)
        }
        _ => None,
    })
}

fn scope_word_column(blocks: &[Block], word: &str) -> Option<i32> {
    word_column(blocks, word).or_else(|| {
        blocks.iter().find_map(|block| {
            let Block::List {
                kind,
                items,
                layout,
                ..
            } = block
            else {
                return None;
            };
            let marker = match kind {
                mant_ir::ListKind::Bullet | mant_ir::ListKind::Dash => 2,
                mant_ir::ListKind::Plain => 0,
                mant_ir::ListKind::Ordered { .. } => 3,
            };
            items.iter().find_map(|item| {
                description_word_column(&item.blocks, word, 5 + layout.indent_columns + marker)
            })
        })
    })
}

fn assert_definition(case: &StructuralCase, document: &mant_ir::Document) {
    let ast = Parser::default()
        .parse_bytes("temporary-head.7", case.basic.source.as_bytes())
        .unwrap();
    let head = item_head(&ast.document.root).expect("actual It HEAD");
    let item = body(document)
        .iter()
        .find_map(|block| {
            if let Block::DefinitionList { items, .. } = block {
                items.first()
            } else {
                None
            }
        })
        .expect("native definition");
    let relation = match case.relation.as_deref().unwrap() {
        "joined" => HeadBodyRelation::joined(),
        "separated" => HeadBodyRelation::separated(),
        "separate" => HeadBodyRelation::Separate,
        _ => unreachable!("checked native fixture relation"),
    };
    let actual_relation = if item.inline_description().is_some() {
        item.head_body_relation
    } else {
        HeadBodyRelation::Separate
    };
    assert_eq!(actual_relation, relation, "{}: {item:#?}", case.basic.id);
    for (word, expected) in &case.basic.native_columns {
        if word == "AFTER" {
            continue;
        }
        if expected.is_some() {
            // The source shape must really exercise HEAD lowering; a macro
            // spelled in BODY cannot stand in for an extended-head test.
            assert!(
                has_text(head, word),
                "{}: wrong native owner for {word}: {head:#?}",
                case.basic.id
            );
        }
        let actual = item.terms.iter().find_map(|term| {
            root_word_column(
                &inline_text(&term.content),
                &term.inline_layout,
                LayoutHint::default(),
                word,
            )
        });
        assert_eq!(actual, *expected, "{} / {word}: {item:#?}", case.basic.id);
    }
    if let Some(expected) = case.body_column {
        assert_eq!(
            description_word_column(
                &item.description,
                "BODY",
                5 + item.layout.body_indent_columns
            ),
            Some(expected),
            "{}: {item:#?}",
            case.basic.id
        );
    }
}

#[test]
fn temporary_indent_survives_scope_return_and_detached_native_head_ownership() {
    // All 43 complete inputs reran on the pristine ASCII/UTF-8/tree/lint
    // before these fixtures were written. print_mdoc_node():314-439 saves
    // source offset, not ti. inset HEAD post does not flush (939-945);
    // its actual BODY flush returns accepted layout to the original owner.
    // TAG/HANG source-line and explicit pre-br fields consume the same
    // term_flushln receipt; an outdent must not prove a spurious Joined word.
    let matrix: serde_json::Value = serde_json::from_str(include_str!("matrix.json")).unwrap();
    let cases: Vec<StructuralCase> =
        serde_json::from_value(matrix["structural_cases"].clone()).unwrap();
    assert_eq!(cases.len(), 43);
    for case in cases {
        assert_eq!(source_digest(&case.basic.source), case.basic.source_sha256);
        let document = parse(&case.basic.source);
        if case.definition {
            assert_definition(&case, &document);
            assert_eq!(
                word_column(body(&document), "AFTER"),
                case.basic.native_columns["AFTER"],
                "{}: {document:#?}",
                case.basic.id
            );
        } else {
            for (word, expected) in case.basic.native_columns {
                assert_eq!(
                    scope_word_column(body(&document), &word),
                    expected,
                    "{} / {word}: {document:#?}",
                    case.basic.id
                );
            }
        }
    }
}
