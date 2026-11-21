use super::{Block, DISPATCH_HEADER, Inline, parsed};
use crate::{compare_structure, ir_profile};

const MAN_HEADER: &str =
    ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- sample\n.SH OPTIONS\n";
const EQN_DELIMITERS: &str = ".EQ\ndelim $$\n.EN\n";

#[test]
fn lifting_a_definition_paragraph_preserves_text_and_source_but_loses_ownership() {
    // man_term.c::pre_RS/post_RS bracket the nested run, independently of
    // option recognition or the macro starting its first visible paragraph.
    for declaration in [
        ".PP\n.B \\-\\-foo",
        ".sp\n.B \\-q",
        ".TP\n.B \\-f",
        ".IP tag",
    ] {
        for body in ["C $x$", ".B C\nword $x$", ".PP\nC $x$"] {
            let source = format!(
                "{MAN_HEADER}{EQN_DELIMITERS}{declaration}\n.RS 4n\n{body}\n.RE\n.PP\nTAIL\n.SH ENDPROBE\nDone\n"
            );
            let (expected, native, mut document) = parsed(&source);
            let (observed, topology) = ir_profile(&document);
            let issues = compare_structure(&expected, &observed, &native, &topology);
            assert!(issues.is_empty(), "{source}: {issues:?}");
            let blocks = &mut document.sections[1].blocks;
            let index = blocks
                .iter()
                .position(|block| matches!(block, Block::DefinitionList { .. }))
                .unwrap();
            let Block::DefinitionList { items, .. } = &mut blocks[index] else {
                unreachable!();
            };
            let description = &mut items[0].description;
            let at = description
                .iter_mut()
                .position(|block| {
                    equation_children(block).is_some_and(|nodes| {
                        nodes
                            .iter()
                            .any(|node| matches!(node, Inline::Equation { .. }))
                    })
                })
                .unwrap();
            let paragraph = description.remove(at);
            blocks.insert(index + 1, paragraph);
            let (observed, topology) = ir_profile(&document);
            let issues = compare_structure(&expected, &observed, &native, &topology);
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.contains("in definition description")),
                "{source}: {issues:?}"
            );
        }
    }
}

#[test]
fn ordinary_and_nested_rs_do_not_invent_a_parent() {
    for body in [
        ".RS 4n\nC $x$\n.RE",
        ".RS 4n\n.RS 2n\nC $x$\n.RE\n.RE",
        ".TP\n.B \\-q\n.RS 4n\n.RS 2n\nC $x$\n.RE\n.RE",
    ] {
        let source = format!("{MAN_HEADER}{EQN_DELIMITERS}{body}\n.PP\nTAIL\n.SH ENDPROBE\nDone\n");
        let (expected, native, document) = parsed(&source);
        let (observed, topology) = ir_profile(&document);
        let issues = compare_structure(&expected, &observed, &native, &topology);
        assert!(issues.is_empty(), "{source}: {issues:?}");
        assert!(!native.equations[0].unproved_paragraph, "{source}");
    }
}

#[test]
fn relative_indent_evidence_does_not_turn_a_valid_owner_into_a_violation() {
    // man_term.c::pre_RS uses the active lmargin for absent operands.
    // term_ascii.c::ascii_hspan proves a whole-cell explicit distance.
    for (request, unproved, definition) in [
        (".RS", true, true),
        (".RS 0.5i", false, true),
        (".RS 0", false, false),
    ] {
        let source = format!(
            "{MAN_HEADER}{EQN_DELIMITERS}.PP\n.B \\-\\-foo\n{request}\nC $x$\n.RE\n.PP\nTAIL\n.SH ENDPROBE\nDone\n"
        );
        let (expected, native, document) = parsed(&source);
        let (observed, topology) = ir_profile(&document);
        let issues = compare_structure(&expected, &observed, &native, &topology);
        assert!(issues.is_empty(), "{source}: {issues:?}");
        assert_eq!(native.equations[0].unproved_paragraph, unproved, "{source}");
        assert_eq!(
            document.sections[1]
                .blocks
                .iter()
                .any(|block| matches!(block, Block::DefinitionList { .. })),
            definition,
            "{source}"
        );
    }
}

#[test]
fn formatting_requests_do_not_replace_the_preceding_declaration() {
    // The man PD and roff br/sp/ft/ta prehandlers do not emit a prose
    // owner between the declaration and its following RS scope.
    for control in [".sp", ".br", ".PD 0", ".ft R", ".ta 2n"] {
        let source = format!(
            "{MAN_HEADER}{EQN_DELIMITERS}.PP\n.B \\-\\-foo\n{control}\n.RS 4n\nC $x$\n.RE\n.PP\nTAIL\n.SH ENDPROBE\nDone\n"
        );
        let (expected, native, document) = parsed(&source);
        let (observed, topology) = ir_profile(&document);
        let issues = compare_structure(&expected, &observed, &native, &topology);
        assert!(issues.is_empty(), "{source}: {issues:?}");
        assert!(
            matches!(
                native.equations[0].owner,
                Some(crate::EquationOwner::Paragraph(crate::FlowOccurrence {
                    definition: Some(_),
                    ..
                }))
            ),
            "{source}"
        );
    }
}

#[test]
fn inline_equation_moves_between_literal_and_prose_are_detected_both_ways() {
    // roff_eqndelim() emits an inline EQ even in no-fill. The source node
    // flags and executed Bd BODY establish the literal owner independently.
    for (header, start, end, paragraph, section) in [
        (MAN_HEADER, ".nf", ".fi", ".PP", ".SH ENDPROBE"),
        (MAN_HEADER, ".EX", ".EE", ".PP", ".SH ENDPROBE"),
        (DISPATCH_HEADER, ".nf", ".fi", ".Pp", ".Sh ENDPROBE"),
        (
            DISPATCH_HEADER,
            ".Bd -literal",
            ".Ed",
            ".Pp",
            ".Sh ENDPROBE",
        ),
        (
            DISPATCH_HEADER,
            ".Bd -unfilled",
            ".Ed",
            ".Pp",
            ".Sh ENDPROBE",
        ),
    ] {
        for from_prose in [false, true] {
            let (literal, prose) = if from_prose {
                ("text", " $x$")
            } else {
                ("$x$", "")
            };
            let source = format!(
                "{header}{EQN_DELIMITERS}{start}\nC {literal}\n{end}\n{paragraph}\nTAIL{prose}\n{section}\nDone.\n"
            );
            let (expected, native, mut document) = parsed(&source);
            let (observed, topology) = ir_profile(&document);
            let issues = compare_structure(&expected, &observed, &native, &topology);
            assert!(issues.is_empty(), "{source}: {issues:?}");
            assert!(!native.equations[0].unproved_literal_owner, "{source}");
            assert!(!native.equations[0].unproved_paragraph, "{source}");
            let blocks = &mut document.sections[1].blocks;
            let mut equation = None;
            for block in &mut *blocks {
                let is_prose = matches!(block, Block::Paragraph { .. });
                if is_prose == from_prose
                    && let Some(nodes) = equation_children(block)
                    && let Some(index) = nodes
                        .iter()
                        .position(|node| matches!(node, Inline::Equation { .. }))
                {
                    equation = Some(nodes.remove(index));
                }
            }
            let recipient = blocks
                .iter_mut()
                .find(|block| {
                    if from_prose {
                        matches!(block, Block::Preformatted { .. })
                    } else {
                        matches!(block, Block::Paragraph { .. })
                    }
                })
                .unwrap();
            equation_children(recipient)
                .unwrap()
                .push(equation.unwrap());
            let (observed, topology) = ir_profile(&document);
            let issues = compare_structure(&expected, &observed, &native, &topology);
            let owner = if from_prose {
                "in paragraph"
            } else {
                "in literal owner"
            };
            assert!(
                issues.iter().any(|issue| issue.contains(owner)),
                "{source}: {issues:?}"
            );
        }
    }
}

#[test]
fn equal_contexts_do_not_make_distinct_literal_owners_interchangeable() {
    // Two Bd BODY scopes have distinct origins even in the same section.
    let source = format!(
        "{DISPATCH_HEADER}{EQN_DELIMITERS}.Bd -literal\nC $x$\n.Ed\n.Bd -literal\nSECOND\n.Ed\n.Pp\nTAIL\n.Sh ENDPROBE\nDone.\n"
    );
    let (expected, native, mut document) = parsed(&source);
    let (observed, topology) = ir_profile(&document);
    assert!(compare_structure(&expected, &observed, &native, &topology).is_empty());
    assert!(!native.equations[0].unproved_literal_owner);
    let mut literal_blocks = document.sections[1]
        .blocks
        .iter_mut()
        .filter(|block| matches!(block, Block::Preformatted { .. }));
    let source = equation_children(literal_blocks.next().unwrap()).unwrap();
    let index = source
        .iter()
        .position(|node| matches!(node, Inline::Equation { .. }))
        .unwrap();
    equation_children(literal_blocks.next().unwrap())
        .unwrap()
        .push(source.remove(index));
    let (observed, topology) = ir_profile(&document);
    let issues = compare_structure(&expected, &observed, &native, &topology);
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("in literal owner at line")),
        "{issues:?}"
    );
}

#[test]
fn unproved_literal_origin_reports_locally_without_waiving_the_section() {
    let source = format!(
        "{DISPATCH_HEADER}{EQN_DELIMITERS}.Bl -bullet\n.It\n.Bd -literal\nC $x$\n.Ed\n.El\n.Pp\nTAIL\n.Sh ENDPROBE\nDone.\n"
    );
    let (expected, native, mut document) = parsed(&source);
    let (observed, topology) = ir_profile(&document);
    assert!(compare_structure(&expected, &observed, &native, &topology).is_empty());
    let [equation] = native.equations.as_slice() else {
        panic!("expected one independently traced equation");
    };
    assert!(equation.unproved_literal_owner);
    let reported = serde_json::to_value(equation).unwrap();
    assert_eq!(reported["unprovedLiteralOwner"], true);
    assert_eq!(reported["sourceLine"], 14);
    let Block::List { items, .. } = &mut document.sections[1].blocks[0] else {
        panic!("expected a bullet item");
    };
    let literal = items[0]
        .blocks
        .iter_mut()
        .find(|block| matches!(block, Block::Preformatted { .. }))
        .unwrap();
    let content = equation_children(literal).unwrap();
    let at = content
        .iter()
        .position(|node| matches!(node, Inline::Equation { .. }))
        .unwrap();
    let equation = content.remove(at);
    equation_children(&mut document.sections[2].blocks[0])
        .unwrap()
        .push(equation);
    let (observed, topology) = ir_profile(&document);
    let issues = compare_structure(&expected, &observed, &native, &topology);
    assert!(issues.iter().any(|issue| issue.contains("in section")));
}

fn equation_children(block: &mut Block) -> Option<&mut Vec<Inline>> {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => Some(children),
        _ => None,
    }
}
