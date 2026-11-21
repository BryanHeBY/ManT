//! Native source occurrence and field witnesses must also detect mutations.

use super::{Block, Inline, first_term, parsed};
use crate::{compare_structure, ir_profile};

#[test]
fn same_source_line_and_man_paragraph_blocks_keep_exact_equation_owners() {
    // roff_eqndelim() replays TEXT/EQ/TEXT at the macro call's coordinates.
    // The style prehandlers print the first paragraph; man pre_PP executes
    // the second paragraph inside its own BODY.
    for source in [
        include_str!("paragraph_witnesses/mdoc-simple.1"),
        include_str!("paragraph_witnesses/mdoc-control.1"),
        include_str!("paragraph_witnesses/mdoc-punct.1"),
        include_str!("paragraph_witnesses/mdoc-span.1"),
        include_str!("paragraph_witnesses/man-macro.1"),
        include_str!("paragraph_witnesses/man-move.1"),
    ] {
        let (expected, native, mut document) = parsed(source);
        let (observed, topology) = ir_profile(&document);
        let issues = compare_structure(&expected, &observed, &native, &topology);
        assert!(issues.is_empty(), "{source}: {issues:?}");
        let (bodies, equation_owner) = document.sections.iter_mut().find_map(|section| {
            let index = section.blocks.iter().position(|block| {
                matches!(block, Block::Paragraph { children, .. } if children.iter().any(|inline| matches!(inline, Inline::Equation { .. })))
            })?;
            Some((&mut section.blocks, index))
        }).unwrap_or_else(|| panic!("{source}"));
        let Block::Paragraph { children, .. } = &mut bodies[equation_owner] else {
            unreachable!();
        };
        let index = children
            .iter()
            .position(|inline| matches!(inline, Inline::Equation { .. }))
            .unwrap();
        let equation = children.remove(index);
        let Block::Paragraph { children, .. } = bodies[..equation_owner]
            .iter_mut()
            .rev()
            .find(|block| matches!(block, Block::Paragraph { .. }))
            .unwrap()
        else {
            unreachable!();
        };
        children.push(equation);
        let (observed, topology) = ir_profile(&document);
        let issues = compare_structure(&expected, &observed, &native, &topology);
        assert!(
            issues.iter().any(|issue| issue.contains("in paragraph")),
            "{source}: {issues:?}"
        );
    }
}

#[test]
fn executed_label_groups_accept_the_original_and_detect_one_missing_atom() {
    // Pinned mdoc termp_fo_pre() writes '(' and keeps the next word tight.
    // termp_quote_pre() adds nested openings. The literal HANG branches
    // can join across the br field transition without closing the row.
    for (source, token) in [
        (include_str!("paragraph_witnesses/hang-side.1"), "B"),
        (include_str!("paragraph_witnesses/FOF.1"), "ALPHA"),
        (include_str!("paragraph_witnesses/longfo34.1"), "ALPHA"),
        (include_str!("paragraph_witnesses/longhang34.1"), "G"),
    ] {
        let (_, native, mut document) = parsed(source);
        let mut issues = Vec::new();
        native.head_payloads.compare(&document, &mut issues);
        assert!(issues.is_empty(), "{source}: {issues:?}");
        let term = first_term(&mut document);
        fn remove_atom(term: &mut [Inline], token: &str) -> bool {
            term.iter_mut().any(|node| match node {
                Inline::Text { value } | Inline::Code { value } if value.contains(token) => {
                    *value = value.replacen(token, "", 1);
                    true
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => remove_atom(children, token),
                _ => false,
            })
        }
        assert!(remove_atom(term, token), "{source}");
        let mut issues = Vec::new();
        native.head_payloads.compare(&document, &mut issues);
        assert!(!issues.is_empty(), "{source}: removed {token}");
    }
}

#[test]
fn empty_source_text_does_not_consume_a_literal_head_continuation() {
    let source = include_str!("paragraph_witnesses/continuation.1");
    // mdoc_term.c::print_mdoc_node() calls term_newln() for empty TEXT
    // under NONEWLINE. term_word() consumes that register at the next word.
    let (_, native, mut document) = parsed(source);
    let mut issues = Vec::new();
    native.head_payloads.compare(&document, &mut issues);
    assert!(issues.is_empty(), "{issues:?}");
    let term = first_term(&mut document);
    let at = term
        .iter()
        .position(|node| matches!(node, Inline::Text { value } if value.contains('B')))
        .unwrap();
    term.insert(at, Inline::line_break());
    let mut issues = Vec::new();
    native.head_payloads.compare(&document, &mut issues);
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("unexpected hard line")),
        "{issues:?}"
    );
}

#[test]
fn a_tag_control_ends_the_original_field_before_later_literal_rows() {
    for source in [
        include_str!("paragraph_witnesses/tag-br.1"),
        include_str!("paragraph_witnesses/tag-sp-0.1"),
        include_str!("paragraph_witnesses/tag-sp-1.1"),
    ] {
        let (_, native, mut document) = parsed(source);
        let mut issues = Vec::new();
        native.head_payloads.compare(&document, &mut issues);
        assert!(issues.is_empty(), "{source}: {issues:?}");
        let term = first_term(&mut document);
        let last = term
            .iter()
            .rposition(|node| matches!(node, Inline::Text { value } if value == "D"))
            .unwrap();
        let separator = term[..last]
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak { .. }))
            .unwrap();
        term.remove(separator);
        native.head_payloads.compare(&document, &mut issues);
        assert!(!issues.is_empty(), "{source}");
    }
}

#[test]
fn an_unproved_same_line_predecessor_keeps_a_local_review() {
    let source = include_str!("paragraph_witnesses/unproved.1");
    let (expected, native, document) = parsed(source);
    let (observed, topology) = ir_profile(&document);
    let issues = compare_structure(&expected, &observed, &native, &topology);
    assert!(issues.is_empty(), "{issues:?}");
    let [equation] = native.equations.as_slice() else {
        panic!("expected a single inline equation");
    };
    assert!(equation.unproved_paragraph);
    assert!(equation.paragraph.is_none());
}
