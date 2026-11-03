//! Ordinary list HEADs skip their arguments, never their direct siblings.

use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind};
use mant_ir::{Block, ResolvedContent};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    direct: bool,
    expected: String,
}

fn item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token == Some(MacroToken::Mdoc(MdocMacro::It)) {
        Some(node)
    } else {
        node.children.iter().find_map(item)
    }
}

fn words(blocks: &[Block]) -> String {
    let mut output = String::new();
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                if !output.is_empty() && !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str(&mant_ir::inline_plain_text(children));
            }
            _ => panic!("unexpected item content: {block:?}"),
        }
    }
    output
}

#[test]
fn ordinary_items_keep_direct_prefixes_and_skip_native_unexecuted_head_arguments() {
    // All twenty exact sources ran pristine ASCII/UTF-8/tree/lint first.
    // mdoc_macro.c::blk_full places DELIMO before HEAD; mdoc_term.c
    // termp_it_pre returns 0 for item/bullet/dash/hyphen/enum HEAD children.
    // Source rows still execute in the common driver before BODY. The IR's
    // structural marker remains consumer-owned, while the authored '(' stays
    // in its original item and is never silently discarded or promoted.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("ordered_plain_list_children/cases.json")).unwrap();
    assert_eq!(cases.len(), 20);
    for case in cases {
        let report = libmandoc_rs::Parser::default()
            .parse_bytes("ordinary-item.1", case.source.as_bytes())
            .unwrap();
        let original = item(&report.document.root).unwrap();
        let head = original
            .children
            .iter()
            .position(|part| part.kind == NodeKind::Head)
            .unwrap();
        assert_eq!(head > 0, case.direct, "{}: real AST ownership", case.id);
        let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&content, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let restored = ResolvedContent::from(restored);
        assert_eq!(restored, content);
        let section = restored
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "DESCRIPTION")
            .unwrap();
        let [Block::List { items, .. }] = section.blocks.as_slice() else {
            panic!("{}: prefix changed list ownership: {section:#?}", case.id)
        };
        assert_eq!(items.len(), 1, "{}", case.id);
        let owner = &items[0];
        assert_eq!(owner.source.unwrap().line, original.line);
        assert_eq!(
            words(&owner.blocks),
            case.expected,
            "{}: {owner:#?}",
            case.id
        );
        let markdown = mant_codec::encode::render_markdown(&restored);
        let reread = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let text = mant_render::render_query_text(&reread);
        assert_eq!(
            text.matches('(').count(),
            usize::from(case.direct),
            "{}: {markdown}",
            case.id
        );
        assert!(
            !text.contains("PREFIX"),
            "{}: skipped HEAD executed",
            case.id
        );
        assert_eq!(text.matches("BODY").count(), 1, "{}", case.id);
    }
}
