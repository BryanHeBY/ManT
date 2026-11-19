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
        assert_eq!(expected.literal_display_reviews, [[line, 2]], "{source}");
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
            expected.literal_display_reviews,
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
