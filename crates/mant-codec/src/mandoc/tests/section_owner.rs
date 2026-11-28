//! A new heading, definition HEAD, or declaration cannot replay the old row.

use super::{
    Block, Inline, inline_text, parse_manual_bytes, projected_document_text, visible_document_text,
};

#[path = "section_owner/padding.rs"]
mod padding;
#[path = "section_owner/tail.rs"]
mod tail;

const MAN: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
const MDOC: &str =
    ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd audit\n.Sh FIRST\n";

fn lower(source: &str) -> mant_ir::Document {
    parse_manual_bytes(std::path::Path::new("section-owner.1"), source.as_bytes()).unwrap()
}

#[test]
fn heading_retires_only_the_row_consumed_by_its_native_pre_or_post() {
    // Exact cases ran all five fixed CVS profiles first. With PD=0,
    // man_term.c::pre_SS has no BLOCK newline: post_SH(HEAD) consumes the
    // old \& with NEXT. The explicit .sp completes two old-owner rows;
    // it cannot move either of them below NEXT. Sh/Ss execute their own
    // mdoc pre/post and may leave the new BODY in literal mode.
    // CVS also prints XNEXT on one device line for the delayed glyph
    // case, but its tree has \zX in FIRST's BODY and NEXT in SS's HEAD.
    for (header, input, expected_title, previous_rows, literal_body) in [
        (MAN, "\\&\n.SS NEXT\n", "NEXT", 0, false),
        (MAN, ".nf\n\\&\n.SS NEXT\n", "NEXT", 0, false),
        (MAN, ".nf\n\\&\n.SH NEXT\n", "NEXT", 1, false),
        (MDOC, ".nf\n\\&\n.Ss NEXT\n", "NEXT", 1, true),
        (MDOC, ".nf\n\\&\n.Sh NEXT\n", "NEXT", 1, false),
        (MAN, ".nf\n\\z\n.SS NEXT\n", "EXT", 0, false),
        (MAN, ".nf\n\\zX\n.SS NEXT\n", "NEXT", 1, false),
        (MAN, ".nf\nALPHA\\c\n.SS NEXT\n", "NEXT", 1, false),
        (MAN, ".nf\n\\&\n.sp 1\n.SS NEXT\n", "NEXT", 2, false),
    ] {
        let source = format!("{header}{input}AFTERMARK\n");
        let document = lower(&source);
        let previous = &document.sections[1];
        let next = previous
            .children
            .first()
            .unwrap_or_else(|| &document.sections[2]);
        assert_eq!(
            inline_text(&next.heading.content).trim_start(),
            expected_title,
            "{source}"
        );
        assert_eq!(
            previous.blocks.len(),
            previous_rows,
            "{source}: {previous:#?}"
        );
        if input.contains("ALPHA") {
            assert!(
                matches!(previous.blocks.as_slice(), [Block::Preformatted { children, .. }]
                if inline_text(children) == "ALPHA")
            );
        }
        if input.contains("\\zX") {
            assert!(
                matches!(previous.blocks.as_slice(), [Block::Preformatted { children, .. }]
                if inline_text(children) == "X")
            );
        }
        if input.contains(".sp") {
            assert!(matches!(
                previous.blocks.as_slice(),
                [
                    Block::Preformatted { .. },
                    Block::VerticalSpace { lines: 1, .. }
                ]
            ));
        }
        let [block] = next.blocks.as_slice() else {
            panic!("{source}: unexpected output owner: {next:#?}");
        };
        let (children, span) = match block {
            Block::Paragraph {
                children, source, ..
            } if !literal_body => (children, source),
            Block::Preformatted {
                children, source, ..
            } if literal_body => (children, source),
            _ => panic!("{source}: wrong output owner: {block:#?}"),
        };
        assert_eq!(inline_text(children), "AFTERMARK", "{source}");
        assert_eq!(
            span.expect("source BODY").line as usize,
            source.lines().count(),
            "{source}"
        );
    }
}

#[test]
fn filled_man_link_starts_in_the_new_body_after_a_literal_predecessor() {
    // man_term.c::print_man_node observes the UR/MT BLOCK fill mode before
    // pre_UR. The fixed UTF-8 label follows NEXT on its first BODY row.
    for (open, destination, close) in [
        ("UR", "https://example.test/", "UE"),
        ("MT", "test@example.test", "ME"),
    ] {
        let source =
            format!("{MAN}.nf\n\\&\n.SS NEXT\n.{open} {destination}\nAFTERMARK\n.{close}\n");
        let document = lower(&source);
        let [
            Block::Paragraph {
                children,
                source: span,
                ..
            },
        ] = document.sections[1].children[0].blocks.as_slice()
        else {
            panic!("{source}: {document:#?}");
        };
        assert_eq!(inline_text(children), format!("AFTERMARK <{destination}>"));
        assert!(
            matches!(children.first(), Some(Inline::Link { children, .. })
            if inline_text(children) == "AFTERMARK")
        );
        assert_eq!(span.expect("source label").line, 10);
    }
}

#[test]
fn native_head_session_sources_retain_their_term_and_successor_owners() {
    // Fixed CVS ASCII/UTF-8/HTML/tree/lint witnesses: detached man IP/TP/TQ
    // and mdoc tag/hang, SY field flush, non-flushing inset/diag, plus the
    // unchanged column and Nm post. The AST shape here is checked via the
    // actual output owner; a test-only session cannot prove source reach.
    let definition_sources = [
        (MAN, ".nf\n\\&\\c\n.TP 12\nTERM\nAFTERMARK\n.fi\n"),
        (MAN, ".nf\n\\&\\c\n.IP TERM 12\nAFTERMARK\n.fi\n"),
        (
            MAN,
            ".nf\n\\&\\c\n.TP 12\nFIRSTTERM\n.TQ 12\nTERM\nAFTERMARK\n.fi\n",
        ),
        (
            MDOC,
            ".nf\n.Bl -tag -width 12n\n.It TERM\nAFTERMARK\n.El\n.fi\n",
        ),
        (
            MDOC,
            ".nf\n.Bl -hang -width 12n\n.It TERM\nAFTERMARK\n.El\n.fi\n",
        ),
        (MDOC, ".nf\n.Bl -inset\n.It TERM\nAFTERMARK\n.El\n.fi\n"),
        (MDOC, ".nf\n.Bl -diag\n.It TERM\nAFTERMARK\n.El\n.fi\n"),
    ];
    for (header, source) in definition_sources {
        let document = lower(&format!("{header}{source}"));
        let Some(Block::DefinitionList { items, .. }) = document.sections[1].blocks.last() else {
            panic!("{source}: {document:#?}");
        };
        let [item] = items.as_slice() else {
            panic!("{source}: {items:#?}");
        };
        let terms: Vec<_> = item
            .terms
            .iter()
            .map(|term| inline_text(&term.content))
            .collect();
        assert_eq!(
            terms,
            if source.contains(".TQ") {
                vec!["FIRSTTERM", "TERM"]
            } else {
                vec!["TERM"]
            },
            "{source}"
        );
        assert!(
            matches!(item.description.as_slice(), [Block::Preformatted { children, .. }]
            if inline_text(children) == "AFTERMARK"),
            "{source}: {item:#?}"
        );
    }
    for (header, source, expected) in [
        (MAN, "\\&\\c\n.SY TERM\nAFTERMARK\n.YS\n", "paragraph"),
        (
            MAN,
            ".nf\n\\&\\c\n.SY TERM\nAFTERMARK\n.YS\n.fi\n",
            "literal",
        ),
        (
            MDOC,
            ".Bl -column 12n\n.It TERM Ta AFTERMARK\n.El\n",
            "table",
        ),
    ] {
        let document = lower(&format!("{header}{source}"));
        let blocks = &document.sections[1].blocks;
        assert!(
            matches!(
                (blocks.last(), expected),
                (Some(Block::Paragraph { .. }), "paragraph")
                    | (Some(Block::Preformatted { .. }), "literal")
                    | (Some(Block::Table { .. }), "table")
            ),
            "{source}: {blocks:#?}"
        );
        let text = visible_document_text(&document);
        for word in ["TERM", "AFTERMARK"] {
            assert_eq!(
                text.split_whitespace()
                    .filter(|token| *token == word)
                    .count(),
                1,
                "{source}: {text}"
            );
        }
    }
    let synopsis = lower(&format!(
        "{}.Nm TERM\n.Ar AFTERMARK\n",
        MDOC.replace(".Sh FIRST", ".Sh SYNOPSIS")
    ));
    assert!(
        matches!(synopsis.sections[1].blocks.as_slice(), [Block::Paragraph { children, .. }]
        if inline_text(children) == "TERM AFTERMARK")
    );
}

#[test]
fn relocated_surviving_head_owns_the_displayed_definition_once() {
    // AX: the two native devices retain the actual Fl, regardless of
    // which overwritten BODY glyph was placed at the initial position.
    for (head, body, spelling) in [
        ("Fl KX", "A", "-KX"),
        ("Fl KX", "C", "-KX"),
        ("Fl VV", "A", "-VV"),
        ("Dv KX", "A", ""),
    ] {
        let source =
            format!("{MDOC}.Bl -inset -compact\n.It {head}\n.No \"\\h'|0n'{body}\"\n.El\n");
        let document = lower(&source);
        assert!(
            !serde_json::to_string(&document)
                .unwrap()
                .contains(":relocated:")
        );
        let Some(Block::DefinitionList { items, .. }) = document.sections[1].blocks.last() else {
            panic!("{source}: '{document:#?}'");
        };
        let [item] = items.as_slice() else {
            panic!("{items:#?}")
        };
        let names = item
            .entry
            .as_ref()
            .map_or(&[][..], |facts| facts.names.as_slice());
        if spelling.is_empty() {
            assert!(!names.iter().any(|name| name == "KX"), "{item:#?}");
            assert!(
                projected_document_text(&document).contains("AX"),
                "{item:#?}"
            );
        } else {
            assert_eq!(inline_text(&item.terms[0].content), spelling, "{item:#?}");
            assert_eq!(names, [spelling], "{item:#?}");
            assert_eq!(
                projected_document_text(&document).matches(spelling).count(),
                1
            );
        }
    }
}

#[test]
fn relocated_head_in_a_native_include_replay_only_binds_a_surviving_original() {
    // Pinned devices: without END they paint U backspace H at column
    // zero and the resulting H belongs to HEAD. With END the original
    // H is overwritten; no exact semantic binding may survive.
    for (suffix, expected_head) in [("", Some("H")), ("END\n", None)] {
        let source = format!(
            "{MDOC}.Bl -diag -compact\n.It H\n.No \"\\zR\"\n.In x\n.No \"\\h'|0n'U\"\n{suffix}.El\n"
        );
        let document = lower(&source);
        let Some(Block::DefinitionList { items, .. }) = document.sections[1].blocks.last() else {
            panic!("{document:#?}")
        };
        let [item] = items.as_slice() else {
            panic!("{items:#?}")
        };
        let found = item
            .entry
            .as_ref()
            .and_then(|entry| entry.names.iter().find(|name| *name == "H"))
            .map(String::as_str);
        assert_eq!(found, expected_head, "{suffix:?}: {item:#?}");
        let projected = projected_document_text(&document);
        assert_eq!(
            projected.matches("R<x>").count(),
            1,
            "{suffix:?}: {item:#?}"
        );
        assert_eq!(
            projected.matches("ENDR<x>").count(),
            usize::from(expected_head.is_none()),
            "{item:#?}"
        );
        if let Some(head) = expected_head {
            assert_eq!(inline_text(&item.terms[0].content), head, "{item:#?}");
        }
    }
}
