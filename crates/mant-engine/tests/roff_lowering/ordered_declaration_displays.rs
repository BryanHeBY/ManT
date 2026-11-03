//! Complete native declaration/display children keep their original order.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{
    Inline, LinkTarget, ResolvedContent,
    visit::{self, Visit},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    r#macro: String,
    kind: String,
    line: u32,
    direct_text: Vec<DirectText>,
    heading: String,
    native_rows: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct DirectText {
    text: String,
    column: u32,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("ordered_declaration_displays/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), 38);
    fixture.cases
}

fn native_owner<'a>(node: &'a Node, case: &Case) -> Option<&'a Node> {
    let kind = if case.kind == "block" {
        NodeKind::Block
    } else {
        NodeKind::Element
    };
    if node.kind == kind
        && node.macro_token.as_deref() == Some(case.r#macro.as_str())
        && node.line == case.line
        && node.column == 2
    {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| native_owner(child, case))
    }
}

fn json_roundtrip(case: &Case) -> ResolvedContent {
    let loaded = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let bundle = mant_protocol::QueryBundle::from(&loaded);
    let json = serde_json::to_string(&bundle).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&bundle).unwrap(),
        "{}: JSON string roundtrip",
        case.id
    );
    decoded.into()
}

fn rendered_rows(content: &ResolvedContent, heading: &str) -> Vec<String> {
    let rendered = mant_render::render_query_man(content);
    let (_, body) = rendered.split_once(&format!("{heading}\n")).unwrap();
    // This cohort authors no ordinary whitespace runs. Only horizontal
    // native device positioning is responsive; empty rows, every glyph,
    // sibling order and zero/nonzero word separators remain observable.
    body.trim_end_matches('\n')
        .split('\n')
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn ordered_declaration_display_children_match_exact_native_rows_after_json() {
    // All 38 complete sources ran pristine ASCII/UTF-8(width78)/HTML/tree/
    // lint before recording expectations. print_mdoc_node() visits every
    // child in order (mdoc_term.c:406-407). Vt HEAD returns0, BODY selects
    // underline, and direct tail TEXT follows its font pop (1232-1246).
    // D1/Dl pre/post belong only to BLOCK (1325-1337,1144-1151): prefix,
    // BODY and trailing non-DELIMC TEXT all execute before that final post.
    // Escape contrasts also follow term.c:662-667,773-774,901-928: an
    // authored prefix may legitimately be consumed by pending BACKAFTER;
    // merely prepending lost punctuation after execution would fail them.
    for case in cases() {
        let report = Parser::default()
            .parse_bytes(&case.id, case.source.as_bytes())
            .unwrap();
        let owner = native_owner(&report.document.root, &case).unwrap();
        let direct: Vec<_> = owner
            .children
            .iter()
            .filter(|child| child.kind == NodeKind::Text)
            .map(|child| DirectText {
                text: child.text.clone().unwrap(),
                column: child.column,
            })
            .collect();
        assert_eq!(
            direct, case.direct_text,
            "{}: original ordered AST direct children",
            case.id
        );
        let content = json_roundtrip(&case);
        assert_eq!(
            mant_ir::validate_document(content.document.as_ref().unwrap()).len(),
            0,
            "{}",
            case.id
        );
        assert_eq!(
            rendered_rows(&content, &case.heading),
            case.native_rows,
            "{}: complete rows and word seams",
            case.id
        );
    }
}

#[test]
fn vt_body_font_and_manual_link_end_before_authored_tail_text() {
    for case in cases()
        .into_iter()
        .filter(|case| matches!(case.id.as_str(), "vt-prefix" | "vt-strong" | "vt-xr"))
    {
        let content = json_roundtrip(&case);
        if case.id == "vt-xr" {
            #[derive(Default)]
            struct Links(Vec<(String, LinkTarget)>);
            impl<'ir> Visit<'ir> for Links {
                fn visit_inline(&mut self, inline: &'ir Inline) {
                    if let Inline::Link {
                        children, target, ..
                    } = inline
                    {
                        self.0
                            .push((mant_ir::inline_plain_text(children), target.clone()));
                    }
                    visit::walk_inline(self, inline);
                }
            }
            let mut links = Links::default();
            links.visit_document(content.document.as_ref().unwrap());
            assert_eq!(
                links.0,
                [(
                    "printf(3)".into(),
                    LinkTarget::Manual {
                        name: "printf".into(),
                        manual_section: Some("3".into())
                    }
                )]
            );
        } else {
            super::font_boundaries::assert_style(&content, "int", 2);
            super::font_boundaries::assert_style(&content, "value", 2);
        }
        super::font_boundaries::assert_style(&content, ";", u8::from(case.id == "vt-strong"));
        super::font_boundaries::assert_style(&content, "AFTER", 0);
        let exported = mant_codec::encode::render_markdown(&content);
        let read = mant_loader::load_markdown_text(&exported, None).unwrap();
        assert_eq!(
            rendered_rows(&read, &case.heading),
            case.native_rows,
            "{}: actual Markdown reader",
            case.id
        );
    }
}

#[test]
fn single_line_display_direct_children_survive_the_actual_markdown_reader() {
    #[derive(Default)]
    struct Displays(Vec<String>);
    impl<'ir> Visit<'ir> for Displays {
        fn visit_block(&mut self, block: &'ir mant_ir::Block) {
            if let mant_ir::Block::Preformatted { children, .. } = block {
                self.0.push(mant_ir::inline_plain_text(children));
            }
            visit::walk_block(self, block);
        }
    }
    for case in cases().into_iter().filter(|case| {
        matches!(
            case.id.as_str(),
            "d1-prefix"
                | "dl-prefix"
                | "d1-multi-prefix"
                | "dl-multi-prefix"
                | "d1-tail"
                | "dl-tail"
                | "d1-xr"
                | "dl-xr"
        )
    }) {
        let content = json_roundtrip(&case);
        let mut direct = Displays::default();
        direct.visit_document(content.document.as_ref().unwrap());
        assert_eq!(
            direct.0,
            [case.native_rows[1].clone()],
            "{}: one original display owner",
            case.id
        );
        let exported = mant_codec::encode::render_markdown(&content);
        let read = mant_loader::load_markdown_text(&exported, None).unwrap();
        let mut imported = Displays::default();
        imported.visit_document(read.document.as_ref().unwrap());
        assert_eq!(
            imported.0, direct.0,
            "{}: actual Markdown code block content",
            case.id
        );
    }
}
