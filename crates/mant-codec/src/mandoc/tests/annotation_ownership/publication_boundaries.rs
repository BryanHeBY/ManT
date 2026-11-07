use super::{Block, Links, Visit, parse_manual_bytes};

const MDOC: &str =
    ".Dd November 7, 2026\n.Dt DEMO 1\n.Os\n.Sh NAME\n.Nm demo\n.Nd test\n.Sh DESCRIPTION\n";
const MAN: &str = ".TH DEMO 1 \"November 7, 2026\"\n.SH NAME\ndemo \\- test\n.SH DESCRIPTION\n";

fn parse_description(header: &str, body: &str) -> mant_ir::Section {
    let source = format!("{header}{body}");
    let document =
        parse_manual_bytes(std::path::Path::new("publication.1"), source.as_bytes()).unwrap();
    document
        .sections
        .into_iter()
        .find(|section| mant_ir::inline_plain_text(&section.heading.content) == "DESCRIPTION")
        .unwrap()
}

fn link_label(section: &mant_ir::Section) -> String {
    let mut links = Links::default();
    links.visit_section(section);
    assert_eq!(links.0.len(), 1, "{links:?}", links = links.0);
    mant_ir::inline_plain_text(&links.0[0].1)
}

#[test]
fn first_label_member_excludes_generated_motion_after_zero_output() {
    // term.c::term_word distinguishes NBRZW, ASCII_BREAK and ESCAPE_HORIZ.
    // mdoc_html.c::mdoc_lk_pre selects the descriptor as the link label.
    // Exact inputs ran pristine before the label assertions. ManT retains
    // accepted authored whitespace and the normal reading motion cell.
    for (payload, expected) in [
        (r#""\&\h'2n'LABEL""#, "LABEL"),
        (r#""\&" "\h'2n'LABEL""#, "LABEL"),
        (r#""\:" "\h'2n'LABEL""#, "LABEL"),
        (r#""\fB" "\h'2n'LABEL""#, "LABEL"),
        (r#""\h'2n'\&" "\h'2n'LABEL""#, "LABEL"),
        (r#"" \h'2n'LABEL""#, "  LABEL"),
        (r#""\zX" "\h'2n'LABEL""#, "X LABEL"),
        (r#""\zX\h'2n'LABEL""#, "X LABEL"),
        (r#""\&\h'2n'LABEL\h'2n'X""#, "LABEL X"),
    ] {
        let body = format!(".No PRE\n.Lk https://one.test {payload}\nAFTER\n");
        assert_eq!(
            link_label(&parse_description(MDOC, &body)),
            expected,
            "{payload}"
        );
    }
    for (begin, target, end) in [
        ("UR", "https://one.test", "UE"),
        ("MT", "x@example.test", "ME"),
    ] {
        for payload in [r"\&\h'2n'LABEL", r"\:\h'2n'LABEL", r"\fB\h'2n'LABEL"] {
            let body = format!("PRE\n.{begin} {target}\n{payload}\n.{end}\nAFTER\n");
            assert_eq!(
                link_label(&parse_description(MAN, &body)),
                "LABEL",
                "{body}"
            );
        }
    }
}

#[test]
fn leading_linked_literal_spacing_keeps_request_and_first_row_sources() {
    // roff_term.c::roff_term_pre_sp emits independent term_vspace rows.
    // Pristine prints one/two blank rows before LASTMARK in these exact
    // sources. With REALLINE first, the same gap is internal to the literal.
    for (request, rows) in [(".sp", 1), (".sp 2", 2)] {
        let body = format!(".nf\n.UR https://one.test\n{request}\nLASTMARK\n.UE\n.fi\nTAILMARK\n");
        let section = parse_description(MAN, &body);
        assert_eq!(link_label(&section), "LASTMARK");
        assert!(
            matches!(
                &section.blocks[..],
                [
                    Block::VerticalSpace { lines, source: Some(gap), .. },
                    Block::Preformatted { source: Some(literal), .. },
                    Block::Paragraph { .. },
                ] if *lines == rows && gap.line == 7 && literal.line == 8
            ),
            "{:?}",
            section.blocks
        );

        let body = format!(
            ".nf\n.UR https://one.test\nREALLINE\n{request}\nLASTMARK\n.UE\n.fi\nTAILMARK\n"
        );
        let section = parse_description(MAN, &body);
        let [
            Block::Preformatted {
                children,
                source: Some(literal),
                ..
            },
            Block::Paragraph { .. },
        ] = &section.blocks[..]
        else {
            panic!("{:?}", section.blocks);
        };
        assert_eq!(literal.line, 7);
        assert!(mant_ir::inline_plain_text(children).starts_with(&format!(
            "REALLINE{}LASTMARK",
            "\n".repeat(usize::from(rows) + 1)
        )));
    }
}

#[test]
fn literal_mode_pre_breaks_a_live_label_without_ending_its_owner() {
    // man_term.c::pre_literal calls term_newln even if filled mode is
    // unchanged, including after \c. Pristine prints two body rows for
    // each exact source, and HTML keeps the initial label in one anchor.
    for first in ["FIRSTMARK", r"FIRSTMARK\c"] {
        for request in ["EE", "fi"] {
            let body =
                format!(".UR https://one.test\n{first}\n.{request}\nLASTMARK\n.UE\nTAILMARK\n");
            assert_eq!(
                link_label(&parse_description(MAN, &body)),
                "FIRSTMARK\nLASTMARK",
                "{body}"
            );
        }
    }
}

#[test]
fn unchanged_nofill_control_retains_the_executed_initial_invisible_row() {
    // man_term.c::print_man_node closes the occupied NBRZW source row
    // at NODE_NOFILL|NODE_LINE before these handlers run. Pristine output
    // has one empty row before LASTMARK, no second row from the request.
    // The public roff reading policy omits precisely that initial row.
    for request in ["EX", "nf", "br"] {
        let body = format!(".nf\n.UR https://one.test\n\\&\n.{request}\nLASTMARK\n.UE\nTAILMARK\n");
        let source = format!("{MAN}{body}");
        let native = crate::mandoc::parse_native_manual(
            std::path::Path::new("publication.1"),
            source.as_bytes(),
        )
        .unwrap();
        let section = &native.sections[1];
        let Block::Preformatted { children, .. } = &section.blocks[0] else {
            panic!("{:?}", section.blocks);
        };
        assert_eq!(
            mant_ir::inline_plain_text(children),
            "\nLASTMARK <https://one.test>\nTAILMARK",
            "{request}"
        );
        let public = parse_description(MAN, &body);
        let Block::Preformatted { children, .. } = &public.blocks[0] else {
            panic!("{:?}", public.blocks);
        };
        assert_eq!(
            mant_ir::inline_plain_text(children),
            "LASTMARK <https://one.test>\nTAILMARK",
            "{request}"
        );
    }
}

#[test]
fn completed_empty_text_before_a_link_is_counted_once() {
    // man_term.c::print_man_node sends the raw empty TEXT through
    // term_vspace(). Neither pre_UR nor pre_br adds a row on entry.
    // Exact inputs ran pristine in ASCII and UTF-8 before these assertions.
    for (begin, target, end) in [
        ("UR", "https://one.test", "UE"),
        ("MT", "x@example.test", "ME"),
    ] {
        for (control, request_rows) in [
            (".sp", 1),
            (".sp 2", 2),
            (".sp 0", 0),
            (".sp\n.sp 2", 3),
            (".br", 0),
        ] {
            let body =
                format!(".nf\n\n.{begin} {target}\n{control}\nLASTMARK\n.{end}\n.fi\nTAILMARK\n");
            let source = format!("{MAN}{body}");
            let native = crate::mandoc::parse_native_manual(
                std::path::Path::new("publication.1"),
                source.as_bytes(),
            )
            .unwrap();
            let Block::Preformatted {
                children, source, ..
            } = &native.sections[1].blocks[0]
            else {
                panic!("{:?}", native.sections[1].blocks);
            };
            assert_eq!(source.unwrap().line, 6, "{body}");
            assert_eq!(
                mant_ir::inline_plain_text(children),
                format!("{}LASTMARK <{target}>", "\n".repeat(request_rows + 1)),
                "{body}"
            );
            let public = parse_description(MAN, &body);
            assert_eq!(link_label(&public), "LASTMARK");
            let Block::Preformatted { children, .. } = &public.blocks[0] else {
                panic!("{:?}", public.blocks);
            };
            let reading_rows = if request_rows > 0 {
                request_rows + 1
            } else {
                0
            };
            assert_eq!(
                mant_ir::inline_plain_text(children),
                format!("{}LASTMARK <{target}>", "\n".repeat(reading_rows)),
                "{body}"
            );
        }
    }
}
