//! Author hard rows remain with their originating owner through public projections.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{
    Block, DiagnosticImpact, DiagnosticLevel, HeadBodyRelation, Inline, LinkTarget,
    ResolvedContent, inline_plain_text,
    visit::{self, Visit},
};

use super::json_roundtrip::json_string_roundtrip;

const TAB: &str = include_str!("source_line_consumers/tab.1");
const BO: &str = include_str!("source_line_consumers/bo-space.1");
const BO_CONTINUED: &str = include_str!("source_line_consumers/bo-continued-space.1");
const XO: &str = include_str!("source_line_consumers/xo-space.1");
const INSET: &str = include_str!("source_line_consumers/inset.1");
const OHANG: &str = include_str!("source_line_consumers/ohang.1");
const LINK_TAB: &str = include_str!("source_line_consumers/link-tab.1");
const LINK_SPACE: &str = include_str!("source_line_consumers/link-space.1");
const URI: &str = "https://x.io";

fn native_text<'a>(node: &'a Node, spelling: &str) -> Option<&'a Node> {
    if node.kind == NodeKind::Text && node.text.as_deref() == Some(spelling) {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| native_text(child, spelling))
    }
}

fn load(source: &str, spelling: &str) -> ResolvedContent {
    let parsed = Parser::default()
        .parse_bytes("probe.1", source.as_bytes())
        .unwrap();
    let text = native_text(&parsed.document.root, spelling).expect("actual native TEXT");
    assert!(text.flags.line_start && !text.flags.no_fill, "{text:?}");
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&loaded);
    if spelling.starts_with('\t') {
        assert!(
            content
                .document
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .any(|diagnostic| {
                    diagnostic.level == DiagnosticLevel::Warning
                        && diagnostic.impact == DiagnosticImpact::None
                        && diagnostic.message == "tab in filled text"
                        && diagnostic.source.is_some_and(|span| span.line == text.line)
                }),
            "the native TAB warning survives JSON without claiming content loss"
        );
    }
    content
}

fn description(query: &ResolvedContent) -> &[Block] {
    &query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| inline_plain_text(&section.heading.content) == "DESCRIPTION")
        .unwrap()
        .blocks
}

fn first_paragraph(blocks: &[Block]) -> &[Inline] {
    let Block::Paragraph { children, .. } = &blocks[0] else {
        panic!("expected paragraph: {blocks:?}");
    };
    children
}

fn row_offsets(text: &str, markers: &[&str]) -> Vec<usize> {
    let offsets = markers
        .iter()
        .map(|marker| {
            let mut occurrences = text.match_indices(marker);
            let offset = occurrences
                .next()
                .unwrap_or_else(|| panic!("{marker}: {text:?}"))
                .0;
            assert!(occurrences.next().is_none(), "duplicate {marker}: {text:?}");
            offset
        })
        .collect::<Vec<_>>();
    assert!(offsets.is_sorted(), "source order: {text:?}");
    offsets
        .iter()
        .map(|&offset| text[offsets[0]..offset].matches('\n').count())
        .collect()
}

fn assert_text_and_markdown(
    content: &ResolvedContent,
    markers: &[&str],
    rows: &[usize],
    check: impl Fn(&ResolvedContent, &str),
) {
    for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
        let mut current = content.clone();
        for round in 0..=2 {
            let plain = mant_render::render_query_text(&current);
            let ansi = mant_render::render_query_text_with(&current, |_, text| {
                format!("\x1b[1m{text}\x1b[0m")
            });
            assert!(ansi.contains("\x1b[1m"));
            assert_eq!(ansi.replace("\x1b[1m", "").replace("\x1b[0m", ""), plain);
            assert_eq!(
                row_offsets(&plain, markers),
                rows,
                "round {round}: {plain:?}"
            );
            check(&current, &plain);

            if round < 2 {
                let markdown = render_markdown_with_options(&current, options);
                current = json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
            }
        }
    }
}

#[test]
fn source_line_tab_stays_filled_and_enclosures_break_after_generated_prefix() {
    // All exact sources ran pinned CVS ASCII/UTF-8(160)/HTML/tree/lint first.
    // man_term.c::print_man_node and mdoc_term.c::print_mdoc_node use ASCII
    // space, not TAB. termp_quote_pre prints "[" before the child visitor;
    // term_word then clears the previous \c before the child's ASCII SP.
    let content = load(TAB, "\tMIDDLE");
    let text = inline_plain_text(first_paragraph(description(&content)));
    assert!(
        text.contains("\tMIDDLE") && !text.contains('\n'),
        "{text:?}"
    );
    assert_text_and_markdown(
        &content,
        &["BEFORE", "MIDDLE", "AFTER"],
        &[0, 0, 0],
        |_, _| {},
    );

    for (source, before, after) in [
        (BO, "BEFOREMARK [", " MIDDLEMARK] AFTERMARK"),
        (BO_CONTINUED, "BEFOREMARK[", " MIDDLEMARK] AFTERMARK"),
        (XO, "BEFOREMARK", " MIDDLEMARK AFTERMARK"),
    ] {
        let content = load(source, " MIDDLEMARK");
        let expected = format!("{before}\n{after}");
        assert_eq!(
            inline_plain_text(first_paragraph(description(&content))),
            expected
        );
        assert_text_and_markdown(
            &content,
            &["BEFOREMARK", "MIDDLEMARK", "AFTERMARK"],
            &[0, 1, 1],
            |_, plain| assert!(plain.contains(&expected), "{plain:?}"),
        );
    }
}

#[test]
fn source_line_break_belongs_to_definition_head_before_body_handoff() {
    // CVS mdoc_term.c::print_mdoc_node calls term_newln inside the HEAD text
    // traversal. The tree places BEFORE/MIDDLE/AFTER in It HEAD and BODYMARK
    // in It BODY. -inset keeps BODYMARK on the last row; -ohang ends the HEAD.
    for (source, rows) in [(INSET, [0, 1, 1, 1]), (OHANG, [0, 1, 1, 2])] {
        let content = load(source, " MIDDLEMARK");
        let [Block::DefinitionList { items, .. }] = description(&content) else {
            panic!("expected a definition list");
        };
        let [item] = items.as_slice() else {
            panic!("one complete definition owner");
        };
        let [term] = item.terms.as_slice() else {
            panic!("one complete definition head");
        };
        assert_eq!(
            inline_plain_text(&term.content),
            "BEFOREMARK\n MIDDLEMARK AFTERMARK"
        );
        let body = inline_plain_text(first_paragraph(&item.description));
        // The inset handoff can retain its generated separator in BODY.
        assert_eq!(body.strip_prefix(' ').unwrap_or(&body), "BODYMARK");
        assert_eq!(
            matches!(item.head_body_relation, HeadBodyRelation::Shared { .. }),
            rows[2] == rows[3]
        );
        assert_text_and_markdown(
            &content,
            &["BEFOREMARK", "MIDDLEMARK", "AFTERMARK", "BODYMARK"],
            &rows,
            |_, _| {},
        );
    }
}

#[test]
fn source_line_first_definition_body_respects_the_live_head_row() {
    // All four exact sources ran pinned CVS first. mdoc_term.c::termp_it_post
    // can leave the field's physical row open. The first BODY TEXT starts
    // with raw SP, so print_mdoc_node closes that row unless the HEAD's
    // continuation is still live.
    for (source, continued) in [
        (
            include_str!("source_line_consumers/tag-body-space.1"),
            false,
        ),
        (
            include_str!("source_line_consumers/tag-continued-body-space.1"),
            true,
        ),
        (
            include_str!("source_line_consumers/hang-body-space.1"),
            false,
        ),
        (
            include_str!("source_line_consumers/hang-continued-body-space.1"),
            true,
        ),
    ] {
        let content = load(source, " BODYMARK");
        let [Block::DefinitionList { items, .. }] = description(&content) else {
            panic!("expected a definition list");
        };
        let [item] = items.as_slice() else {
            panic!("one complete definition owner");
        };
        assert_eq!(item.terms.len(), 1);
        assert_eq!(inline_plain_text(&item.terms[0].content), "HEADMARK");
        let body = inline_plain_text(first_paragraph(&item.description));
        assert!(
            body.contains("BODYMARK TAILMARK") && !body.contains("HEADMARK"),
            "{body:?}"
        );
        assert_eq!(
            matches!(item.head_body_relation, HeadBodyRelation::Shared { .. }),
            continued,
            "{source}"
        );
        let offset = usize::from(!continued);
        assert_text_and_markdown(
            &content,
            &["HEADMARK", "BODYMARK", "TAILMARK"],
            &[0, offset, offset],
            |_, _| {},
        );
    }
}

#[test]
fn source_line_tag_head_consumes_field_without_opening_an_extra_row() {
    // In mdoc_term.c::termp_it_pre a tag HEAD sets NOBREAK and trailspace
    // while its own raw source SP executes term_newln(). term_flushln keeps
    // the device row open and contributes two cells before the authored SP.
    let source = include_str!("source_line_consumers/tag-head-space.1");
    let content = load(source, " MIDDLEMARK");
    let [Block::DefinitionList { items, .. }] = description(&content) else {
        panic!("expected a definition list");
    };
    let [item] = items.as_slice() else {
        panic!("one complete definition owner");
    };
    let [term] = item.terms.as_slice() else {
        panic!("one complete definition head");
    };
    let head = "BEFOREMARK   MIDDLEMARK AFTERMARK";
    assert_eq!(inline_plain_text(&term.content), head);
    assert_text_and_markdown(
        &content,
        &["BEFOREMARK", "MIDDLEMARK", "AFTERMARK", "BODYMARK"],
        &[0, 0, 0, 0],
        |_, plain| assert!(plain.contains(head), "{plain:?}"),
    );
}

#[derive(Default)]
struct Links(Vec<(String, LinkTarget)>);

impl<'ir> Visit<'ir> for Links {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Link {
            target, children, ..
        } = inline
        {
            self.0.push((inline_plain_text(children), target.clone()));
        }
        visit::walk_inline(self, inline);
    }
}

#[test]
fn source_line_inside_link_body_keeps_break_and_label_in_the_same_owner() {
    // The pinned man tree puts both actual TEXT children in UR BODY. Only
    // the ASCII SP visitor breaks; post_UR emits its URI outside the label.
    for (source, spelling, middle_row) in [(LINK_TAB, "\tBETA", 0), (LINK_SPACE, " BETA", 1)] {
        let content = load(source, spelling);
        assert_text_and_markdown(
            &content,
            &["PRE", "ALPHA", "BETA", URI, "POST"],
            &[0, 0, middle_row, middle_row, middle_row + 1],
            |query, plain| {
                let mut links = Links::default();
                links.visit_document(query.document.as_ref().unwrap());
                let [(label, target)] = links.0.as_slice() else {
                    panic!("one retained link owner: {:?}", links.0);
                };
                assert_eq!(target, &LinkTarget::External { uri: URI.into() });
                assert_eq!(row_offsets(label, &["ALPHA", "BETA"]), [0, middle_row]);
                assert!(!label.contains(URI) && !label.contains("PRE") && !label.contains("POST"));
                assert!(plain.contains(&format!("<{URI}>")));
            },
        );
    }
}

#[test]
fn source_line_field_flush_keeps_the_accepted_generated_cells() {
    // Pristine UTF-8 prints two U+00A0 cells for an empty diag HEAD before
    // the raw-SP BODY or explicit .br. termp_it_pre wrote a native word;
    // term_fill and term_field accept those graph cells even without text.
    // Inset without a HEAD executes no such word.
    for (source, expected) in [
        (
            include_str!("source_line_consumers/diag-empty-space.1"),
            "  \n BODYMARK",
        ),
        (
            include_str!("source_line_consumers/diag-empty-br.1"),
            "  \nBODYMARK",
        ),
        (
            include_str!("source_line_consumers/inset-empty-space.1"),
            " BODYMARK",
        ),
    ] {
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let [Block::DefinitionList { items, .. }] = description(&content) else {
            panic!("expected definition list");
        };
        let [item] = items.as_slice() else {
            panic!("expected a single empty-head definition");
        };
        assert_eq!(item.terms, [] as [mant_ir::DefinitionTerm; 0]);
        assert_eq!(
            inline_plain_text(first_paragraph(&item.description)),
            expected,
        );
        assert!(mant_render::render_query_text(&content).contains(expected));
    }
}

#[test]
fn empty_text_uses_its_native_visit_or_operand_path() {
    #[derive(serde::Deserialize)]
    struct Case {
        id: String,
        source: String,
        source_sha256: String,
        expected: String,
        profiles: std::collections::BTreeMap<String, (i32, String, String)>,
    }
    #[derive(serde::Deserialize)]
    struct Matrix {
        oracle_sha256: String,
        cases: Vec<Case>,
    }

    // The fixture records exact pristine UTF-8 rows after undoing terminal
    // overstrikes and removing the fixed five-column page margin. In's
    // in_line_argn() emits its later empty argument as a sibling. The mdoc
    // TEXT visitor uses term_word() for it, while man visits even a
    // non-NODE_LINE empty B child via term_vspace() unless \c is live.
    let matrix: Matrix =
        serde_json::from_str(include_str!("source_line_consumers/empty_text_cases.json")).unwrap();
    assert_eq!(
        matrix.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(matrix.cases.len(), 54);
    for case in matrix.cases {
        assert_eq!(case.source_sha256.len(), 64, "{}", case.id);
        assert_eq!(case.profiles.len(), 5, "{}", case.id);
        for (profile, (status, stdout, stderr)) in &case.profiles {
            assert_eq!(stdout.len(), 64, "{}: {profile}", case.id);
            assert_eq!(stderr.len(), 64, "{}: {profile}", case.id);
            if profile != "lint" {
                assert_eq!(*status, 0, "{}: {profile}", case.id);
            }
        }
        assert_empty_text_visit_path(&case.source, &case.id);
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap());
        let plain = mant_render::render_query_text(&content);
        let (actual, _) = plain
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("\n\nEND\n")
            .unwrap();
        assert_eq!(actual, case.expected, "{}", case.id);
        if case.id == "no-fill/c/in-sibling" {
            assert_published_include_span(&content);
        }
        if case.id == "filled/plain/in-sibling" {
            let search = mant_query::search_query(
                &content,
                &mant_protocol::SearchQuery {
                    pattern: "<header.h>  AFTER".into(),
                    syntax: mant_protocol::SearchSyntax::Literal,
                    case: mant_protocol::SearchCase::Sensitive,
                    scope: mant_protocol::SearchScope::Visible,
                    word: false,
                    context_lines: 0,
                    limit: 10,
                    offset: 0,
                },
            )
            .unwrap();
            assert_eq!(search.total, 1);
            assert_eq!(
                search.matches[0].occurrences[0].matched_text,
                "<header.h>  AFTER"
            );
        }
        if case.id.contains("/in-") {
            assert!(
                description(&content)
                    .iter()
                    .all(|block| !matches!(block, Block::VerticalSpace { .. })),
                "{}",
                case.id
            );
        }
        if matches!(
            case.id.as_str(),
            "filled/plain/in-sibling"
                | "filled/p/in-expanded"
                | "no-fill/plain/in-sibling"
                | "no-fill/c/in-sibling"
                | "no-fill/c/in-expanded"
                | "no-fill/plain/mdoc-empty-source"
                | "filled/plain/man-operand"
        ) {
            let rows = row_offsets(&case.expected, &["BEFORE", "AFTER"]);
            assert_text_and_markdown(&content, &["BEFORE", "AFTER"], &rows, |_, plain| {
                assert!(plain.contains(&case.expected), "{}: {plain:?}", case.id);
            });
        }
    }
}

fn assert_published_include_span(content: &ResolvedContent) {
    // The pinned HTML marks the complete accepted In spelling as code.
    let roots = description(content).iter().filter_map(|block| match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => Some(children),
        _ => None,
    });
    let mut code_spans = Vec::new();
    for inline in roots.flatten() {
        match inline {
            Inline::Code { value } => code_spans.push(value.as_str()),
            Inline::Link { .. } => panic!("private include annotation reached public IR"),
            _ => {}
        }
    }
    assert_eq!(code_spans, ["<header.h>"]);
}

fn assert_empty_text_visit_path(source: &str, id: &str) {
    use libmandoc_rs::{MacroToken, ManMacro, MdocMacro, RoffMacro};

    let parsed = Parser::default()
        .parse_bytes("probe.1", source.as_bytes())
        .unwrap();
    let body = parsed
        .document
        .root
        .children
        .iter()
        .find_map(|section| {
            let head = section
                .children
                .iter()
                .find(|node| node.kind == NodeKind::Head)?;
            (head.children.first()?.text.as_deref()? == "DESCRIPTION").then(|| {
                section
                    .children
                    .iter()
                    .find(|node| node.kind == NodeKind::Body)
                    .unwrap()
            })
        })
        .unwrap();
    let empty = |node: &Node| node.kind == NodeKind::Text && node.text.as_deref() == Some("");
    let filled = id.starts_with("filled/");
    if id.contains("/in-") {
        let index = body
            .children
            .iter()
            .position(|node| node.macro_token == Some(MacroToken::Mdoc(MdocMacro::In)))
            .unwrap();
        let include = &body.children[index];
        let sibling = &body.children[index + 1];
        assert_eq!(include.children.len(), 1, "{id}");
        assert!(empty(sibling) && !sibling.flags.line_start, "{id}");
        assert_eq!(sibling.line, include.line, "{id}");
        if id.ends_with("in-expanded") {
            assert_eq!(body.children[index + 2].line, sibling.line, "{id}");
            assert!(body.children[index + 2].flags.line_start, "{id}");
        }
    } else if id.ends_with("man-visited") || id.ends_with("man-operand") {
        let (name, count) = if id.ends_with("man-visited") {
            (ManMacro::B, 1)
        } else {
            (ManMacro::Br, 2)
        };
        let macro_node = body
            .children
            .iter()
            .find(|node| node.macro_token == Some(MacroToken::Man(name)))
            .unwrap();
        assert_eq!(macro_node.children.len(), count, "{id}");
        assert!(
            macro_node
                .children
                .iter()
                .all(|node| empty(node) && !node.flags.line_start),
            "{id}"
        );
    } else if filled {
        // man.c/mdoc.c consume a raw blank after a continued word.
        // Without continuation they normalize it to a sp request.
        let spacing = body.children.iter().any(|node| {
            node.macro_token == Some(MacroToken::Roff(RoffMacro::Sp)) && node.children.is_empty()
        });
        assert_eq!(spacing, !id.contains("/c/"), "{id}");
        assert!(!body.children.iter().any(empty), "{id}");
    } else {
        assert!(
            body.children
                .iter()
                .any(|node| { empty(node) && node.flags.line_start && node.flags.no_fill }),
            "{id}"
        );
    }
}
