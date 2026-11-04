//! Display completion and following source/generated words keep distinct owners.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{
    Block, Inline, LinkTarget, ResolvedContent,
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
    display: String,
    display_count: usize,
    native_rows: Vec<String>,
    visible_rows: Vec<String>,
    portable_rows: Option<Vec<String>>,
    #[serde(default)]
    portable_owners: Vec<Owner>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Owner {
    kind: String,
    text: String,
    line: u32,
    column: u32,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!("display_return/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), 20);
    fixture.cases
}

fn display_count(node: &Node, token: &str) -> usize {
    usize::from(
        node.kind == NodeKind::Block
            && node
                .macro_token
                .as_ref()
                .map(libmandoc_rs::MacroToken::as_str)
                == Some(token),
    ) + node
        .children
        .iter()
        .map(|child| display_count(child, token))
        .sum::<usize>()
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

fn description(content: &ResolvedContent) -> &[Block] {
    &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.id.as_str() == "description")
        .unwrap()
        .blocks
}

fn assert_portable_owners(case: &Case, content: &ResolvedContent, read: &ResolvedContent) {
    if case.portable_rows.is_none() {
        assert_eq!(case.portable_owners, []);
        return;
    }
    let original: Vec<_> = description(content)
        .iter()
        .map(|block| {
            let (kind, children, source) = match block {
                Block::Paragraph {
                    children, source, ..
                } => ("paragraph", children, source),
                Block::Preformatted {
                    children, source, ..
                } => ("preformatted", children, source),
                _ => panic!("{}: independent phrasing/literal owners", case.id),
            };
            let source = source.unwrap();
            Owner {
                kind: kind.into(),
                text: mant_ir::inline_plain_text(children),
                line: source.line,
                column: source.column,
            }
        })
        .collect();
    assert_eq!(
        original, case.portable_owners,
        "{}: original owners",
        case.id
    );
    assert_eq!(
        description(read).len(),
        original.len(),
        "{}: read owners",
        case.id
    );
    for (owner, block) in original.iter().zip(description(read)) {
        let (kind, children) = match block {
            Block::Paragraph { children, .. } => ("paragraph", children),
            Block::Preformatted { children, .. } => ("preformatted", children),
            _ => panic!("{}: same portable owner kind", case.id),
        };
        assert_eq!(kind, owner.kind, "{}", case.id);
        assert_eq!(
            mant_ir::inline_plain_text(children),
            owner.text,
            "{}",
            case.id
        );
    }
}

#[test]
fn completed_displays_match_native_rows_and_the_actual_markdown_reader() {
    // All 20 complete inputs ran pinned ASCII/UTF-8(width78)/HTML/tree/lint
    // first. D1/Dl BLOCK post calls term_newln before geometry return
    // (mdoc_term.c:1142-1148,1325-1334,406-439). Xo has no post; quote post
    // emits its mark in the live state (1594-1703). Authored and generated
    // closes are independent. Bd variants, outer nf and recovery are controls.
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
        assert_eq!(
            display_count(&parsed.document.root, &case.display),
            case.display_count,
            "{}: original display blocks",
            case.id
        );
        let content = roundtrip(&case.source);
        assert_eq!(
            mant_ir::validate_document(content.document.as_ref().unwrap()).len(),
            0,
            "{}",
            case.id
        );
        // Remove only the reference's common five-cell device origin when
        // recording native_rows. Producer placement, every inner SP and hard
        // row are exact here; no horizontal run is folded.
        assert_eq!(
            rows(&content),
            case.native_rows,
            "{}: executed layout",
            case.id
        );
        let markdown = mant_codec::encode::render_markdown(&content);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        // Markdown's fenced display intentionally omits device placement.
        // This cohort authors no leading blank; only that placement is
        // excluded, retaining each complete row and all internal separators.
        // Four cases contain independent complete Paragraph owners; CommonMark
        // framing adds an explicit reading gap (encode/blocks/assembly.rs).
        // Preserve native_rows above and bind each explained portable gap to
        // exact original owner kinds, payloads and source spans below.
        assert_portable_owners(&case, &content, &read);
        let visible: Vec<_> = rows(&read)
            .iter()
            .map(|row| row.trim_start_matches(' ').to_owned())
            .collect();
        assert_eq!(
            visible,
            *case.portable_rows.as_ref().unwrap_or(&case.visible_rows),
            "{}: actual Markdown rows\n{markdown}",
            case.id
        );
    }
}

#[test]
fn source_and_generated_closes_start_a_new_filled_owner_after_the_display() {
    for (id, tail, line, column) in [
        ("xo-d1", ") AFTER", 10, 5),
        ("do-d1", "” AFTER", 8, 2),
        ("po-d1", ") AFTER", 8, 2),
    ] {
        let case = cases().into_iter().find(|case| case.id == id).unwrap();
        let content = roundtrip(&case.source);
        let blocks = description(&content);
        let displays: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Preformatted {
                    children, source, ..
                } => Some((mant_ir::inline_plain_text(children), source)),
                _ => None,
            })
            .collect();
        assert_eq!(displays.len(), 1, "{id}: completed owner was not reopened");
        assert_eq!(displays[0].0, "CONTENT", "{id}");
        let display_source = displays[0].1.unwrap();
        assert_eq!(
            (display_source.line, display_source.column),
            (9, 5),
            "{id}: original display source"
        );
        let Some(Block::Paragraph {
            children, source, ..
        }) = blocks.last()
        else {
            panic!("{id}: return owner is filled")
        };
        assert_eq!(mant_ir::inline_plain_text(children), tail, "{id}");
        let source = source.unwrap();
        assert_eq!(
            (source.line, source.column),
            (line, column),
            "{id}: return word source"
        );
    }
}

#[test]
fn display_link_identity_and_label_do_not_capture_the_return_mark() {
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
        .find(|case| case.id == "do-d1-link")
        .unwrap();
    let content = roundtrip(&case.source);
    let mut links = Links::default();
    links.visit_document(content.document.as_ref().unwrap());
    assert_eq!(
        links.0,
        [(
            "CONTENT".into(),
            LinkTarget::External {
                uri: "https://example.test/item".into()
            }
        )]
    );
}

#[test]
fn rejected_display_units_keep_their_empty_row_between_complete_literal_owners() {
    // These exact complete September 28 sources ran the pristine five-profile
    // oracle first. D1/Dl pre changes offset/TAB, not source NOFILL; BLOCK
    // post is termp_bl_post (mdoc_term.c:1142-1148,1325-1334). A leading p
    // marker followed by SP has no graph, so term_fill returns nbr=0, rejects
    // DROP, then its real flush tail closes one empty row (term.c:143-146,
    // 294-305,233-253). That Layout receipt has a VerticalSpace owner; it is
    // not an authored empty no-fill TEXT nor a missing Preformatted block.
    for display in ["D1", "Dl"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.{display} X\\p\n.{display} \"\\p DROP\"\n.{display} Tail\n.Sh NEXT\n.No END\n"
        );
        let content = roundtrip(&source);
        let [
            Block::Preformatted {
                children: first,
                layout: first_layout,
                source: first_source,
                ..
            },
            Block::VerticalSpace {
                lines: 1,
                source: None,
            },
            Block::Preformatted {
                children: last,
                layout: last_layout,
                source: last_source,
                ..
            },
        ] = description(&content)
        else {
            panic!("{display}: one completed layout row between its original owners")
        };
        assert_eq!(mant_ir::inline_plain_text(first), "X", "{display}");
        assert_eq!(mant_ir::inline_plain_text(last), "Tail", "{display}");
        assert_eq!(first_layout.indent_columns, 6, "{display}");
        assert_eq!(last_layout.indent_columns, 6, "{display}");
        let first_source = first_source.unwrap();
        let last_source = last_source.unwrap();
        assert_eq!(
            (first_source.line, first_source.column),
            (8, 5),
            "{display}"
        );
        assert_eq!((last_source.line, last_source.column), (10, 5), "{display}");
        assert_eq!(rows(&content), ["      X", "", "      Tail"], "{display}");
        assert!(!mant_render::render_query_man(&content).contains("DROP"));
        let markdown = mant_codec::encode::render_markdown(&content);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_eq!(rows(&read), ["X", "", "Tail"], "{display}: {markdown}");
        assert!(!mant_render::render_query_man(&read).contains("DROP"));
    }
}
