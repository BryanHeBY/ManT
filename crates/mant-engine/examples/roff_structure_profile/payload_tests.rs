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
    for case in payloads::heads() {
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

fn display_probe(source: &str) -> String {
    format!(
        ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n+         BEFORE\n.br\n{source}\n.br\nAFTER\n.Sh END PROBE\nDone.\n"
    )
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
        let observation = description(display(&report.document.root).unwrap(), false);
        assert!(!observation.absent(), "{source}");
        assert!(!observation.has_text(), "{source}");
        assert_eq!(observation.has_row(), row, "{source}");
    }
}
