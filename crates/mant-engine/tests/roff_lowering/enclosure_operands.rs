//! Native enclosure operands share one text executor and actual input receipts.
use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{Block, ResolvedContent};

#[path = "../support/enclosure_operands.rs"]
mod fixture;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

fn find<'a>(node: &'a Node, name: &str, kind: NodeKind) -> Option<&'a Node> {
    if node.kind == kind && node.macro_token.as_deref() == Some(name) {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| find(child, name, kind))
    }
}

fn assert_native_operand_identity(case: &fixture::Case) {
    let parsed = Parser::default()
        .parse_bytes(&case.id, case.source.as_bytes())
        .unwrap();
    let root = &parsed.document.root;
    if case.id.starts_with("operand-punctuation-") {
        assert_delimiter_punctuation(root, case);
        return;
    }
    if case.cohort == "raw-operands" {
        let enclosure = find(root, "En", NodeKind::Block)
            .unwrap()
            .enclosure
            .as_ref()
            .unwrap();
        // These exact raw operands are independently present in every
        // recorded tree profile. Normalization borrows Es's raw spelling;
        // it must not turn zero-width/font/break instructions into text.
        let (label, operand) = [
            ("empty", ""),
            ("invisible", "\\&"),
            ("font", "\\fB"),
            ("zero", "\\z"),
            ("cached", "\\zX"),
            ("break", "A\\p"),
            ("continue", "\\c"),
            ("ordinary-punctuation", "@"),
        ]
        .into_iter()
        .find(|(label, _)| case.id.starts_with(&format!("operand-{label}-")))
        .unwrap();
        let side = case.id.strip_prefix(&format!("operand-{label}-")).unwrap();
        assert_eq!(
            enclosure.opening,
            if side.starts_with("closing-") {
                ""
            } else {
                operand
            }
        );
        assert_eq!(
            enclosure.closing.as_deref(),
            Some(if side.starts_with("opening-") {
                ""
            } else {
                operand
            })
        );
    }
    if case.id.starts_with("risk-empty-") {
        let enclosure = find(root, "Eo", NodeKind::Block).unwrap();
        for (kind, part) in [
            (NodeKind::Head, "head"),
            (NodeKind::Body, "body"),
            (NodeKind::Tail, "tail"),
        ] {
            let children = &find(enclosure, "Eo", kind).unwrap().children;
            assert_eq!(
                !children.is_empty(),
                case.id.starts_with(&format!("risk-empty-{part}-")),
                "{}: original present-empty child pointer",
                case.id
            );
        }
    }
    if case.id.starts_with("risk-immediate-end-") {
        let enclosure = find(root, "Eo", NodeKind::Block).unwrap();
        let body = find(enclosure, "Eo", NodeKind::Body).unwrap();
        assert!(
            body.children.first().unwrap().scope_end.is_some(),
            "{}: termp_eo_pre's actual end-body child branch",
            case.id
        );
    }
}

fn assert_delimiter_punctuation(root: &Node, case: &fixture::Case) {
    // in_line_argn() sends leading DELIM_OPEN to a sibling before opening
    // Es and closes it on later punctuation (mdoc_macro.c:1436-1525).
    // These independently recorded tree profiles therefore contain no '['
    // operand: one empty child remains for opening/closing, none for both.
    let both = case.id.starts_with("operand-punctuation-both-");
    let definition = find(root, "Es", NodeKind::Element).unwrap();
    assert_eq!(definition.children.len(), usize::from(!both));
    let enclosure = &find(root, "En", NodeKind::Block).unwrap().enclosure;
    if both {
        assert_eq!(enclosure, &None);
    } else {
        let enclosure = enclosure.as_ref().unwrap();
        assert_eq!(enclosure.opening, "");
        assert_eq!(enclosure.closing, None);
    }
    assert_eq!(opening_delimiters(root), if both { 2 } else { 1 });
}

fn opening_delimiters(node: &Node) -> usize {
    usize::from(node.flags.delimiter_open && node.text.as_deref() == Some("["))
        + node.children.iter().map(opening_delimiters).sum::<usize>()
}

fn rows(content: &ResolvedContent) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    text.split_once("DESCRIPTION\n")
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

fn unwrap_definition(content: &mut ResolvedContent) {
    let section = content
        .document
        .as_mut()
        .unwrap()
        .sections
        .iter_mut()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    let mut flat = Vec::new();
    for block in std::mem::take(&mut section.blocks) {
        match block {
            Block::List { mut items, .. } => {
                assert_eq!(items.len(), 1);
                flat.append(&mut items[0].blocks);
            }
            block => flat.push(block),
        }
    }
    section.blocks = flat;
}

#[test]
fn native_operands_keep_cursor_cells_hard_rows_and_escape_execution_after_wire() {
    // Exact sources ran pristine ASCII/UTF-8/HTML/tree/lint first. Eo uses
    // termp_eo_pre/post's NBRZW word, not an empty word; En feeds normalized
    // Es bytes directly to term_word (mdoc_term.c:1600-1746). Its empty words
    // update flags without a native operand cell (term.c:573-589).
    verify_cases(
        fixture::cases(),
        |case| &case.id,
        |case| {
            assert_native_operand_identity(case);
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let content = json_string_roundtrip(&original);
            assert_eq!(
                mant_ir::validate_document(content.document.as_ref().unwrap()),
                []
            );
            assert_eq!(
                rows(&content),
                case.rows,
                "{}: original native rows",
                case.id
            );
            let mut read = content;
            for round in 1..=2 {
                let markdown = mant_codec::encode::render_markdown(&read);
                read = mant_loader::load_markdown_text(&markdown, None).unwrap();
                let mut observed = read.clone();
                if matches!(case.context.as_str(), "tag-head" | "tag-body") {
                    unwrap_definition(&mut observed);
                }
                assert_eq!(
                    rows(&observed),
                    fixture::portable_rows(case),
                    "{}: actual Markdown round {round}\n{markdown}",
                    case.id
                );
            }
        },
    );
}
