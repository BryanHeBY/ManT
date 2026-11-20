//! Source payload and realized structure must agree independently.

use super::super::{
    ListTopologyKind, compare_structure, ir_profile,
    ordinal_labels::{OrdinalDisposition, cases::payloads, description::description},
};
use super::parsed_structure;

#[test]
fn literal_displays_count_only_proven_glyphs_without_losing_blank_body_rows() {
    for example in payloads::displays() {
        let case = &example.case;
        let (expected, topology, document) = parsed_structure(&case.source);
        assert_eq!(
            expected.literal_displays, example.literal_count,
            "{}",
            case.name
        );
        let (observed, actual) = ir_profile(&document);
        assert_eq!(
            observed.preformatted_blocks, example.literal_count,
            "{}",
            case.name
        );
        let violations = compare_structure(&expected, &observed, &topology, &actual);
        assert!(violations.is_empty(), "{}: {violations:?}", case.name);
        example.assert_body(&document);
    }
}

#[test]
fn ordinal_head_controls_do_not_publish_arguments_as_words() {
    for case in payloads::heads().chain(payloads::margin_heads()) {
        let (expected, topology, document) = parsed_structure(&case.source);
        let (observed, actual) = ir_profile(&document);
        let [source] = topology.lists.as_slice() else {
            panic!("{}: expected one source list", case.name);
        };
        assert_eq!(source.source_line, case.source_line, "{}", case.name);
        assert_eq!(
            source.kind,
            if case.disposition == OrdinalDisposition::Recovered {
                ListTopologyKind::Generic
            } else {
                ListTopologyKind::Definition
            },
            "{}",
            case.name,
        );
        assert_eq!(source.items, 2, "{}", case.name);
        assert!(!source.ordinal_uncertain, "{}", case.name);
        let violations = compare_structure(&expected, &observed, &topology, &actual);
        assert!(violations.is_empty(), "{}: {violations:?}", case.name);
    }
}

#[test]
fn display_fields_and_nested_containers_require_only_real_literal_owners() {
    for example in payloads::display_fields() {
        let case = &example.case;
        let (expected, topology, document) = parsed_structure(&case.source);
        assert_eq!(
            expected.literal_displays, example.literal_count,
            "{}",
            case.name
        );
        assert!(expected.literal_display_reviews.is_empty(), "{}", case.name);
        example.assert_literal_count(&document);
        if example.ordinal {
            let expected_kind = if case.disposition == OrdinalDisposition::Recovered {
                ListTopologyKind::Generic
            } else {
                ListTopologyKind::Definition
            };
            assert!(
                topology.lists.iter().any(|list| {
                    list.source_line == case.source_line && list.kind == expected_kind
                }),
                "{}",
                case.name
            );
        }
        let (observed, actual) = ir_profile(&document);
        let violations = compare_structure(&expected, &observed, &topology, &actual);
        assert!(violations.is_empty(), "{}: {violations:?}", case.name);
    }
}

#[test]
fn partially_rewound_source_is_reviewed_only_at_its_display_owner() {
    // With the fixed term.c input slots, both devices retain PRF. The
    // source proof covers whole rewinds and intentionally leaves this
    // partial overwrite as a coordinate-bound source review.
    let field = r".D1 PROOF\h'|2n'\p\p";
    for (carrier, line) in [
        (String::from(field), 10),
        (format!(".Bd -literal -compact\n{field}\n.Ed"), 11),
    ] {
        let source = format!(
            concat!(
                ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n",
                "STARTPROBE\n.br\n{}\n.br\nSTOPPROBE\n.Sh ENDPROBE\nDone.\n"
            ),
            carrier,
        );
        let (expected, _, _) = parsed_structure(&source);
        assert_eq!(expected.literal_displays, 0, "{source}");
        let [review] = expected.literal_display_reviews.as_slice() else {
            panic!("{source}");
        };
        assert_eq!(
            (review.line, review.column, review.occurrence, review.cell),
            (line, 2, 1, None),
            "{source}"
        );
    }
}

#[test]
fn default_slot_coverage_is_withheld_for_an_inherited_native_font() {
    // In fixed CVS a Roman PROOF has five cells, all replaced here. Bold
    // and emphasis encode additional cells, so the native devices retain
    // ROOF in those scopes even though the authored strings match.
    use std::iter::once;
    let field = r".D1 PROOF\h'|0n'\p\p\p\p\p";
    let carriers = once(("", "", None)).chain([
        (".ft B\n", "\n.ft R", Some([11, 2])),
        (".Bf -emphasis\n", "\n.Ef", Some([11, 2])),
    ]);
    for (before, after, review) in carriers {
        let source = format!(
            concat!(
                ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n",
                "STARTPROBE\n.br\n{}{}{}\n.br\nSTOPPROBE\n.Sh ENDPROBE\nDone.\n"
            ),
            before, field, after,
        );
        let (expected, _, _) = parsed_structure(&source);
        assert_eq!(expected.literal_displays, 0, "{source}");
        assert_eq!(
            expected
                .literal_display_reviews
                .iter()
                .map(|r| [r.line, r.column])
                .collect::<Vec<_>>(),
            review.into_iter().collect::<Vec<_>>(),
            "{source}"
        );
    }
}

fn display_probe(source: &str) -> String {
    format!(
        concat!(
            ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n",
            "BEFORE\n.br\n{}\n.br\nAFTER\n.Sh END PROBE\nDone.\n"
        ),
        source,
    )
}

#[test]
fn generated_words_are_checked_after_their_native_field_closes() {
    for mode in ["Dl", "literal"] {
        for (motion, join) in [(r"\h'|0n'", "Ns "), (r"\h'-12n'", "Ns "), (r"\h'|0n'", "")] {
            for count in 0..=3 {
                // termp_fl_pre() pushes bold. The fixed devices retain
                // - BS -, BS -, -, and no glyph for the four prefixes.
                let line = format!("Fl {join}No \"{motion}{}\"", r"\p".repeat(count));
                let source = display_probe(&if mode == "Dl" {
                    format!(".Dl {line}")
                } else {
                    format!(".Bd -literal -compact\n.{line}\n.Ed")
                });
                let (expected, topology, document) = parsed_structure(&source);
                let (observed, actual) = ir_profile(&document);
                assert_eq!(
                    expected.literal_displays,
                    usize::from(count < 3),
                    "{source}"
                );
                assert!(expected.literal_display_reviews.is_empty(), "{source}");
                let violations = compare_structure(&expected, &observed, &topology, &actual);
                assert!(violations.is_empty(), "{source}: {violations:?}");
            }
        }
        let examples = [
            (r"Xr \& \&", false),
            (r#"Fn x Ns No "\h'|0n'\p\p\p\p\p""#, true),
            (r#"In x Ns No "\h'|0n'\p\p\p\p\p""#, true),
            (r#"Lk u t Ns No "\h'|0n'\p\p\p\p\p""#, true),
        ];
        for (line, needs_local_review) in examples {
            let source = display_probe(&if mode == "Dl" {
                format!(".Dl {line}")
            } else {
                format!(".Bd -literal -compact\n.{line}\n.Ed")
            });
            let (expected, _, _) = parsed_structure(&source);
            // termp_xr_pre() emits parentheses for two glyphless arguments.
            // The other operands' full native field geometry isn't proved.
            assert_eq!(
                expected.literal_displays,
                usize::from(!needs_local_review),
                "{source}"
            );
            assert_eq!(
                expected.literal_display_reviews.len(),
                usize::from(needs_local_review),
                "{source}"
            );
        }
    }
}

#[test]
fn a_later_font_event_cannot_change_a_closed_display_obligation() {
    // The Roman cells are erased before the next section enters. The
    // following font selection belongs to that section's own native scope.
    for field in [
        r".D1 PROOF\h'|0n'\p\p\p\p\p".to_string(),
        r".Dl PROOF\h'|0n'\p\p\p\p\p".to_string(),
        format!(".Bd -literal -compact\n{}\n.Ed", r"PROOF\h'|0n'\p\p\p\p\p"),
        format!(".Bd -unfilled -compact\n{}\n.Ed", r"PROOF\h'|0n'\p\p\p\p\p"),
    ] {
        for suffix in [
            ".ft R\nplain",
            ".ft B\nstrong",
            ".Bf -emphasis\nitalic\n.Ef",
            r"then \fIitalic\fR text",
        ] {
            let base = display_probe(&field);
            let source = base.replace("Done.\n", &format!("{suffix}\n"));
            let (expected, _, _) = parsed_structure(&source);
            assert_eq!(expected.literal_displays, 0, "{source}");
            assert!(expected.literal_display_reviews.is_empty(), "{source}");
        }
    }
}

#[test]
fn literal_obligations_cannot_use_another_display_or_repeat_one_occurrence() {
    use mant_ir::Block;

    for (body, victim) in [
        (".Dl ALPHA\n.Bd -literal -compact\nALPHA\nBETA\n.Ed", 0),
        (".Bd -literal -compact\nALPHA\nBETA\n.Ed\n.Dl ALPHA", 1),
        (".de EXAMPLE\n.D1 ALPHA\n.Dl ALPHA\n..\n.EXAMPLE", 0),
        (".de EXAMPLE\n.D1 ALPHA\n.Dl ALPHA\n..\n.EXAMPLE", 1),
        (".Bd -literal -compact\nALPHA\n.Dl NESTED\nOMEGA\n.Ed", 1),
    ] {
        let source = display_probe(body);
        let (expected, topology, mut document) = parsed_structure(&source);
        let baseline = |document: &mant_ir::Document| {
            let (observed, actual) = ir_profile(document);
            compare_structure(&expected, &observed, &topology, &actual)
        };
        assert_eq!(expected.literal_displays, 2, "{source}");
        let violations = baseline(&document);
        assert!(violations.is_empty(), "{source}: {violations:?}");
        let blocks = &mut document.sections[1].blocks;
        let index = blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| matches!(block, Block::Preformatted { .. }))
            .nth(victim)
            .unwrap()
            .0;
        blocks.remove(index);
        let violations = baseline(&document);
        assert_eq!(
            violations
                .iter()
                .filter(|line| line.starts_with("literal-displays:"))
                .count(),
            1,
            "{source}: {violations:?}"
        );
    }
}

#[test]
fn literal_origin_must_stay_in_its_own_item_body() {
    use mant_ir::Block;

    // The fixed tree's macro expansion gives both native items and display
    // payloads identical coordinates. The fixed devices retain both rows.
    for body in [
        ".Bl -tag -width width\n.It FIRST\n.Dl ALPHA\n.It SECOND\n.Dl ALPHA\n.El",
        ".de EXAMPLE\n.It FIRST\n.Dl ALPHA\n.It SECOND\n.Dl ALPHA\n..\n.Bl -tag -width width\n.EXAMPLE\n.El",
    ] {
        let source = display_probe(body);
        let (expected, topology, mut document) = parsed_structure(&source);
        let check = |document: &mant_ir::Document| {
            let (observed, actual) = ir_profile(document);
            compare_structure(&expected, &observed, &topology, &actual)
        };
        assert!(check(&document).is_empty());
        let Some(Block::DefinitionList { items, .. }) = document.sections[1]
            .blocks
            .iter_mut()
            .find(|block| matches!(block, Block::DefinitionList { .. }))
        else {
            panic!("native It must own the literal block");
        };
        let relocated = items[0].description.remove(0);
        items[1].description.push(relocated);
        let violations = check(&document);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].starts_with("literal-displays:"));
    }
}

#[test]
fn an_inferred_definition_preserves_its_original_literal_ancestry() {
    use mant_ir::Block;

    // Fixed CVS executes this display directly in the authored It body.
    // The semantic index can then deduce a more specific owner for "-p1".
    let source = display_probe(concat!(
        ".Bl -tag -width width\n.It Fl p Ar strip-count\n.Fl p Ns Ar 1\ngives\n",
        ".Pp\n.D1 Pa u/howard/src/blurfl/blurfl.c\n.Pp\nwithout the leading slash.\n.El",
    ));
    let (expected, topology, document) = parsed_structure(&source);
    let Some(Block::DefinitionList { items, .. }) = document.sections[1]
        .blocks
        .iter()
        .find(|block| matches!(block, Block::DefinitionList { .. }))
    else {
        panic!("expected authored It");
    };
    assert!(items[0].description.iter().any(|block| matches!(
        block,
        Block::DefinitionList { items, .. }
            if items[0].description.iter().any(|block| matches!(block, Block::Preformatted { .. }))
    )));
    let (observed, actual) = ir_profile(&document);
    assert_eq!(expected.literal_displays, 1);
    let violations = compare_structure(&expected, &observed, &topology, &actual);
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn display_field_uses_its_inherited_spacing_mode_and_tbl_resets_it() {
    use crate::ordinal_labels::SourceContext;
    use libmandoc_rs::{Node, NodeKind, Parser};

    fn observed(node: &Node, context: &SourceContext, displays: &mut Vec<bool>) {
        if node.kind == NodeKind::Block
            && matches!(node.macro_token.as_deref(), Some("D1" | "Dl" | "Bd"))
        {
            displays.push(description(node, false, context).has_text());
        }
        for child in &node.children {
            observed(child, context, displays);
        }
    }
    let witnesses = |source: &str| {
        let report = Parser::default()
            .parse_bytes("audit.1", source.as_bytes())
            .unwrap();
        let root = &report.document.root;
        let context = SourceContext::collect(root);
        let mut displays = Vec::new();
        observed(root, &context, &mut displays);
        displays
    };

    // Two independent owners see the mode at their own source position.
    // termp_sm_pre() persists the mode, and term_word() adds its separator
    // only when the mode allows it. That separator decides the leading pass.
    for mode in ["D1", "Dl", "literal", "unfilled"] {
        let word = |label: &str| {
            if matches!(mode, "D1" | "Dl") {
                format!(r".{mode} No \p No {label}")
            } else {
                format!(".Bd -{mode} -compact\n.No \\p No {label}\n.Ed")
            }
        };
        for (first, second) in [("off", "on"), ("on", "off")] {
            let source = display_probe(&format!(
                ".Sm {first}\n{}\n.Sm {second}\n{}",
                word("PROOFFIRST"),
                word("PROOFSECOND"),
            ));
            assert_eq!(
                witnesses(&source),
                [first == "off", second == "off"],
                "{source}"
            );
        }
    }
    for (table, visible) in [("", true), (".TS\nl.\nCELL\n.TE\n", false)] {
        let source = display_probe(&format!(
            ".Bd -literal -compact\n.Sm off\n{table}.No \\p No PROOF\n.Ed"
        ));
        assert_eq!(witnesses(&source), [visible], "{source}");
    }
}

#[test]
fn native_display_text_proof_tracks_overwrites_and_separate_equation_owners() {
    for display in [".D1", ".Dl", ".Bd -literal -compact"] {
        for (word, preformatted) in [
            (r"PROOF\h'0n'", 1),
            (r"\h'0n'PROOF", 1),
            (r"\o'AB'", 1),
            (r"\o' '", 0),
            (r"PROOF\h'|0n'\p\p\p\p\p", 0),
            (r"PROOF\h'-5n'\p\p\p\p\p", 0),
            (r"\p PROOF", 0),
            (r"\p\:PROOF", 0),
            (r"\p\& PROOF", 1),
            (r"\pPROOF", 1),
        ] {
            // term.c::term_word() and term_fill(): only positioning that
            // may revisit earlier slots can invalidate the prefix here.
            let body = if display.starts_with(".Bd") {
                format!("{display}\n{word}\n.Ed")
            } else {
                format!("{display} {word}")
            };
            let source = display_probe(&body);
            let (expected, _, document) = parsed_structure(&source);
            let (observed, _) = ir_profile(&document);
            assert_eq!(expected.literal_displays, preformatted, "{source}");
            assert_eq!(observed.preformatted_blocks, preformatted, "{source}");
        }
    }
    for (word, preformatted) in [(".EQ\nx\n.EN", 0), (".EQ\nx\n.EN\nTEXT", 1)] {
        // The same glyphs are visible in fixed CVS; the source EQ owns the
        // math independently from the literal text around it.
        let source = display_probe(&format!(".Bd -unfilled -compact\n{word}\n.Ed"));
        let (expected, topology, document) = parsed_structure(&source);
        let (observed, actual) = ir_profile(&document);
        assert_eq!(expected.display_equations, 1, "{source}");
        assert_eq!(expected.literal_displays, preformatted, "{source}");
        assert_eq!(observed.preformatted_blocks, preformatted, "{source}");
        let violations = compare_structure(&expected, &observed, &topology, &actual);
        assert!(violations.is_empty(), "{source}: {violations:?}");
    }
}

#[test]
fn display_space_requests_distinguish_physical_rows_from_control_boundaries() {
    fn display(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
        if node.kind == libmandoc_rs::NodeKind::Block && node.macro_token.as_deref() == Some("Bd") {
            Some(node)
        } else {
            node.children.iter().find_map(display)
        }
    }

    for (requests, row) in [
        (".sp 0", false),
        (".sp -1", false),
        (".sp 1", true),
        (".sp -1\n.sp 1", false),
        (".sp -1\n.sp 2", true),
        (".sp -1\n\\fB\n.sp 1", true),
    ] {
        // roff_term_pre_sp() adds debt for a negative count. term_vspace()
        // consumes it, and the real word event for \fB resets it.
        let source = display_probe(&format!(".Bd -unfilled -compact\n{requests}\n.Ed"));
        let report = libmandoc_rs::Parser::default()
            .parse_bytes("audit.1", source.as_bytes())
            .unwrap();
        let context = crate::ordinal_labels::SourceContext::collect(&report.document.root);
        let observation = description(display(&report.document.root).unwrap(), false, &context);
        assert!(!observation.absent(), "{source}");
        assert!(!observation.has_text(), "{source}");
        assert_eq!(observation.has_row(), row, "{source}");
    }
}
