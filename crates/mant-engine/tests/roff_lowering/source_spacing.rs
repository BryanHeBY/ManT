//! Source rows and macro operands share the native formatter word boundary.

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
    recovery: bool,
    native_rows: Vec<String>,
    native_text: Vec<TextEvent>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct TextEvent {
    value: String,
    line: u32,
    column: u32,
    line_start: bool,
    no_fill: bool,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!("source_spacing/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), 24);
    fixture.cases
}

fn description_body(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block
        && matches!(
            node.macro_token
                .as_ref()
                .map(libmandoc_rs::MacroToken::as_str),
            Some("Sh" | "SH")
        )
        && node.children.iter().any(|part| {
            part.kind == NodeKind::Head
                && part
                    .children
                    .iter()
                    .any(|text| text.text.as_deref() == Some("DESCRIPTION"))
        })
    {
        node.children
            .iter()
            .find(|part| part.kind == NodeKind::Body)
    } else {
        node.children.iter().find_map(description_body)
    }
}

fn collect_text(node: &Node, output: &mut Vec<TextEvent>) {
    if let Some(value) = &node.text {
        output.push(TextEvent {
            value: value.clone(),
            line: node.line,
            column: node.column,
            line_start: node.flags.line_start,
            no_fill: node.flags.no_fill,
        });
    }
    for child in &node.children {
        collect_text(child, output);
    }
}

fn roundtrip(source: &str) -> ResolvedContent {
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let original = mant_protocol::QueryBundle::from(&loaded);
    let json = serde_json::to_string(&original).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(original).unwrap(),
        serde_json::to_value(&decoded).unwrap()
    );
    decoded.into()
}

fn rows(content: &ResolvedContent) -> Vec<String> {
    let rendered = mant_render::render_query_man(content);
    let body = rendered
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("\nNEXT\n")
        .unwrap()
        .0;
    let body = body.strip_suffix('\n').unwrap_or(body);
    if body.is_empty() {
        Vec::new()
    } else {
        body.split('\n').map(str::to_owned).collect()
    }
}

#[test]
fn source_word_events_keep_native_spacing_after_json_and_real_markdown_readback() {
    // Every complete source first ran pinned ASCII/UTF-8(width78)/HTML/tree/lint.
    // Filled NODE_LINE only observes TABREF (mdoc_term.c:318-322). Sm changes
    // NONOSPACE globally (1820-1832); term_word consumes NOSPACE before it
    // applies NONOSPACE (term.c:573-589). Thus BEFORE retains its released
    // separator, while later FIRST/SECOND join. No whitespace is collapsed:
    // authored blanks, NBSP, Unicode, no-fill hard rows and c are independent.
    for case in cases() {
        let parsed = Parser::default()
            .parse_bytes(&case.id, case.source.as_bytes())
            .unwrap();
        assert_eq!(
            !parsed.diagnostics.is_empty(),
            case.recovery,
            "{}: native recovery",
            case.id
        );
        let mut original_text = Vec::new();
        collect_text(
            description_body(&parsed.document.root).unwrap(),
            &mut original_text,
        );
        assert_eq!(
            original_text, case.native_text,
            "{}: original word events and source positions",
            case.id
        );
        let content = roundtrip(&case.source);
        assert_eq!(
            mant_ir::validate_document(content.document.as_ref().unwrap()).len(),
            0,
            "{}",
            case.id
        );
        assert_eq!(
            rows(&content),
            case.native_rows,
            "{}: exact hard rows and word seams",
            case.id
        );
        let markdown = mant_codec::encode::render_markdown(&content);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_eq!(
            rows(&read),
            case.native_rows,
            "{}: actual Markdown reader\n{markdown}",
            case.id
        );
    }
}

#[test]
fn adjoining_words_do_not_enter_the_link_owner() {
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
    let case = cases()
        .into_iter()
        .find(|case| case.id == "off-link")
        .unwrap();
    let content = roundtrip(&case.source);
    let markdown = mant_codec::encode::render_markdown(&content);
    let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&content, &read] {
        let mut links = Links::default();
        links.visit_document(content.document.as_ref().unwrap());
        assert_eq!(
            links.0,
            [(
                "SECOND".into(),
                LinkTarget::External {
                    uri: "https://example.test/item".into()
                }
            )]
        );
    }
}
