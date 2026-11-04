//! Fo argument routing preserves complete parent-local native adjacency.

use std::collections::BTreeMap;

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{
    Block, Inline, LinkTarget, ResolvedContent,
    visit::{self, Visit},
};
use serde::Deserialize;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;
use super::oracle_receipt::{OracleReceipt, assert_receipts};

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    core_count: usize,
    forward_count: usize,
    pairwise_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    section: String,
    source: String,
    source_sha256: String,
    cohort: String,
    context: String,
    rows: Vec<String>,
    portable_definition_gap: Option<DefinitionGap>,
    known_layout_difference: Option<DefinitionGap>,
    owners: Vec<NativeOwner>,
    profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
struct DefinitionGap {
    row: usize,
    start: usize,
    count: usize,
    projected_count: usize,
}

#[derive(Deserialize)]
struct Profile {
    status: i32,
    sha256: String,
    diagnostics: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct NativeOwner {
    r#macro: String,
    kind: String,
    line: u32,
    column: u32,
    line_start: bool,
    no_print: bool,
    parent: Option<String>,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("source_adjacency/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(
        (
            fixture.core_count,
            fixture.forward_count,
            fixture.pairwise_count
        ),
        (80, 24, 22)
    );
    assert_eq!(fixture.cases.len(), 129);
    for (cohort, count) in [
        ("core", 80),
        ("forward", 24),
        ("pairwise", 22),
        ("minimal", 3),
    ] {
        assert_eq!(
            fixture
                .cases
                .iter()
                .filter(|case| case.cohort == cohort)
                .count(),
            count
        );
    }
    for case in &fixture.cases {
        assert_receipts(
            &case.id,
            &case.source_sha256,
            case.profiles.iter().map(|(profile, run)| {
                (
                    profile.as_str(),
                    OracleReceipt {
                        status: run.status,
                        stdout_sha256: &run.sha256,
                        stderr_sha256: None,
                        diagnostics: &run.diagnostics,
                    },
                )
            }),
        );
    }
    fixture.cases
}

fn kind(node: &Node) -> Option<&'static str> {
    match node.kind {
        NodeKind::Element => Some("elem"),
        NodeKind::Block => Some("block"),
        NodeKind::Head => Some("head"),
        NodeKind::Body => Some("body"),
        NodeKind::Tail => Some("tail"),
        _ => None,
    }
}

fn native_owners(node: &Node, parent: Option<String>, output: &mut Vec<NativeOwner>) {
    let next_parent = if let (Some(token), Some(kind)) = (node.macro_token.as_deref(), kind(node)) {
        if node.line >= 8 && token != "Sh" {
            output.push(NativeOwner {
                r#macro: token.to_owned(),
                kind: kind.to_owned(),
                line: node.line,
                column: node.column,
                line_start: node.flags.line_start,
                no_print: node.flags.no_print,
                parent,
            });
        }
        Some(format!("{token}:{kind}:{}:{}", node.line, node.column))
    } else {
        parent
    };
    for child in &node.children {
        native_owners(child, next_parent.clone(), output);
    }
}

fn roundtrip(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let result = json_string_roundtrip(&content);
    assert_eq!(
        mant_ir::validate_document(result.document.as_ref().unwrap()),
        []
    );
    result
}

fn body_rows(content: &ResolvedContent, section: &str) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    let body = text
        .split_once(&format!("{section}\n"))
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0;
    // Exclude only the responsive device origin, row padding, and the final
    // section separator. Every internal SP/NBSP, hard row and empty row stays.
    body.trim_matches('\n')
        .split('\n')
        .map(|row| row.trim_matches(' ').to_owned())
        .collect()
}

fn portable_rows(case: &Case, content: &ResolvedContent) -> Vec<String> {
    let mut rows = case.rows.clone();
    if let Some(gap) = &case.portable_definition_gap {
        let section = content
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == case.section)
            .unwrap();
        let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
            panic!("{}: original native definition owner", case.id)
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].terms.len(), 1);
        assert_eq!(
            mant_ir::inline_plain_text(&items[0].terms[0].content),
            "LABEL"
        );
        // encode/blocks/definitions.rs::definition_content represents a
        // shared prose HEAD/BODY boundary with one SP. Native -tag geometry
        // remains checked above and is the only declared export difference.
        let mut scalars: Vec<_> = rows[gap.row].chars().collect();
        assert_eq!(&scalars[..gap.start], ['L', 'A', 'B', 'E', 'L']);
        assert_eq!(
            &scalars[gap.start..gap.start + gap.count],
            vec![' '; gap.count]
        );
        scalars.splice(
            gap.start..gap.start + gap.count,
            vec![' '; gap.projected_count],
        );
        rows[gap.row] = scalars.into_iter().collect();
    }
    rows
}

fn reading_rows(case: &Case, content: &ResolvedContent) -> Vec<String> {
    let Some(gap) = &case.known_layout_difference else {
        return case.rows.clone();
    };
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == case.section)
        .unwrap();
    let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
        panic!("{}: native tag definition", case.id)
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        mant_ir::inline_plain_text(&items[0].terms[0].content),
        "LABEL"
    );
    assert_eq!(items[0].layout.body_indent_columns, 8);
    assert_eq!(items[0].layout.min_term_gap_columns, 2);
    let [
        Block::Paragraph {
            children, source, ..
        },
    ] = items[0].description.as_slice()
    else {
        panic!("{}: complete function paragraph", case.id)
    };
    assert_eq!(source.unwrap().line, 10);
    assert_eq!(mant_ir::inline_plain_text(children), "  call(arg\nint);");
    // Bounded pre-existing column discrepancy, reproduced by baseline CLI
    // SHA 8f3d49ba...f37: CVS termp_it_pre uses width 6n+2=8, hence LABEL+3SP.
    // Current IR retains two additional Fo field-prefix SP in this BODY.
    // Record this boundary as a known layout difference, not column parity;
    // every other scalar, word separator and hard row remains exact.
    let mut rows = case.rows.clone();
    let mut scalars: Vec<_> = rows[gap.row].chars().collect();
    assert_eq!(&scalars[..gap.start], ['L', 'A', 'B', 'E', 'L']);
    assert_eq!(
        &scalars[gap.start..gap.start + gap.count],
        vec![' '; gap.count]
    );
    assert_eq!((gap.count, gap.projected_count), (3, 5));
    scalars.splice(
        gap.start..gap.start + gap.count,
        vec![' '; gap.projected_count],
    );
    rows[gap.row] = scalars.into_iter().collect();
    rows
}

fn verify_all(check: impl Fn(&Case) + std::panic::RefUnwindSafe) {
    verify_cases(cases(), |case| &case.id, check);
}

#[test]
fn complete_fo_parent_matrix_matches_native_rows_and_source_owners_after_json() {
    // All 129 complete sources ran the pinned ASCII/UTF-8(78)/HTML/tree/lint
    // profiles before these assertions. mdoc_term.c::print_mdoc_nodelist walks
    // the unchanged sibling chain; synopsis_pre reads roff_node_prev, Fa/Fl
    // read roff_node_next, while Pf post reads raw n->next. roff.c's
    // roff_node_transparent skips comments/NOPRT and its explicit token set.
    verify_all(|case| {
        let parsed = Parser::default()
            .parse_bytes(&case.id, case.source.as_bytes())
            .unwrap();
        let mut owners = Vec::new();
        native_owners(&parsed.document.root, None, &mut owners);
        assert_eq!(
            owners, case.owners,
            "{}: original native parents/roles",
            case.id
        );
        let content = roundtrip(&case.source);
        assert_eq!(
            body_rows(&content, &case.section),
            reading_rows(case, &content),
            "{}: exact source-executed rows",
            case.id
        );
    });
}

fn minimal_source(body: &str) -> String {
    format!(
        ".Dd November 4, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd source adjacency regression\n.Sh SYNOPSIS\n.Fo call\n{body}\n.Fc\n.Sh NEXT\nAFTER.\n"
    )
}

#[test]
fn fa_predecessor_flush_preserves_the_next_words_first_letter() {
    // Fixed CVS synopsis_pre(Ft) reads the preceding Fa and calls
    // term_newln(). That flush clears pending BACKAFTER before `int`.
    let source = minimal_source(".Fa \"arg\\z\"\n.Ft int");
    let case = cases()
        .into_iter()
        .find(|case| case.id == "minimal-fa-z-ft")
        .unwrap();
    assert_eq!(source, case.source);
    let content = roundtrip(&source);
    assert_eq!(body_rows(&content, "SYNOPSIS"), ["call(arg", "int);"]);
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")
        .unwrap();
    let [Block::Paragraph { children, .. }] = section.blocks.as_slice() else {
        panic!("one complete function paragraph owns the hard row")
    };
    assert_eq!(mant_ir::inline_plain_text(children), "call(arg\nint);");
}

#[test]
fn prefix_and_bare_flag_keep_the_same_line_fa_successor_tight() {
    // Fixed CVS termp_pf_post tests raw n->next; termp_fl_pre tests the
    // logical successor and NODE_LINE. Neither relation ends at Fa routing.
    for (body, id, expected) in [
        (".Pf PRE Fa arg", "minimal-pf-fa", "call(PREarg);"),
        (".Fl Fa arg", "minimal-fl-fa", "call(-arg);"),
    ] {
        let source = minimal_source(body);
        let case = cases().into_iter().find(|case| case.id == id).unwrap();
        assert_eq!(source, case.source);
        assert_eq!(body_rows(&roundtrip(&source), "SYNOPSIS"), [expected]);
    }
}

// Canonical Markdown represents one native definition as a bullet item. Read
// its actual item owner while preserving the item's internal block boundaries.
fn unwrap_exported_items(blocks: &mut Vec<Block>) {
    let mut output = Vec::new();
    for block in std::mem::take(blocks) {
        if let Block::List {
            kind: mant_ir::ListKind::Bullet,
            mut items,
            ..
        } = block
        {
            assert_eq!(items.len(), 1);
            let mut item = items.remove(0);
            unwrap_exported_items(&mut item.blocks);
            output.extend(item.blocks);
        } else {
            output.push(block);
        }
    }
    *blocks = output;
}

#[test]
fn actual_markdown_readback_preserves_adjacency_rows() {
    verify_all(|case| {
        let content = roundtrip(&case.source);
        let expected = portable_rows(case, &content);
        let markdown = mant_codec::encode::render_markdown(&content);
        let mut read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        if matches!(case.context.as_str(), "head" | "tagbody") {
            let section = read
                .document
                .as_mut()
                .unwrap()
                .sections
                .iter_mut()
                .find(|section| section.heading.plain_text() == case.section)
                .unwrap();
            unwrap_exported_items(&mut section.blocks);
        }
        assert_eq!(
            body_rows(&read, &case.section),
            expected,
            "{}: actual Markdown\n{markdown}",
            case.id
        );
    });
}

#[derive(Default)]
struct Links {
    occurrences: Vec<(String, String)>,
    anchors: Vec<(String, u32)>,
}
impl<'ir> Visit<'ir> for Links {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Link {
            children,
            target: LinkTarget::External { uri },
            ..
        } = inline
        {
            self.occurrences
                .push((mant_ir::inline_plain_text(children), uri.clone()));
        }
        if let Inline::Anchor {
            id,
            owner_source: Some(source),
            ..
        } = inline
        {
            self.anchors.push((id.to_string(), source.line));
        }
        visit::walk_inline(self, inline);
    }
}

#[test]
fn function_argument_fragments_keep_link_and_target_owners() {
    let case = cases()
        .into_iter()
        .find(|case| case.id == "pair-linked-owner")
        .unwrap();
    let content = roundtrip(&case.source);
    let mut links = Links::default();
    links.visit_document(content.document.as_ref().unwrap());
    assert_eq!(
        links.occurrences,
        [("LINK".into(), "https://example.test".into())]
    );
    assert_eq!(
        links
            .anchors
            .iter()
            .filter(|(id, _)| id == "argument-target")
            .collect::<Vec<_>>(),
        [&("argument-target".to_owned(), 11)]
    );
}
