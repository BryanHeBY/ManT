//! Source actions before a section heading, independent of visible BODY output.

const MDOC: &str =
    ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd audit\n.Sh DESCRIPTION\n";
const MAN: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n";

fn sections(items: &[mant_ir::Section]) -> impl Iterator<Item = &mant_ir::Section> {
    items
        .iter()
        .flat_map(|item| std::iter::once(item).chain(item.children.iter()))
}

#[test]
fn section_block_pre_uses_source_predecessor_and_live_spacing_debt() {
    // Each full source was run through the fixed CVS UTF-8/tree/lint
    // profiles. man_term.c::pre_SH/pre_SS filter the peer's BODY children;
    // mdoc_term.c::termp_sh_pre reads raw children and termp_ss_pre only
    // checks for a predecessor. term_vspace spends skipvsp before endline.
    for (header, body, gap, before) in [
        (
            MDOC,
            "BEFOREMARK\n.sp -1\n.Sh NEXT\nAFTERMARK\n",
            0,
            "BEFOREMARK",
        ),
        (
            MDOC,
            "BEFOREMARK\n.sp -2\n.Ss NEXT\nAFTERMARK\n",
            0,
            "BEFOREMARK",
        ),
        (MDOC, ".Ss PREVIOUS\n.Ss NEXT\nAFTERMARK\n", 1, "PREVIOUS"),
        (MDOC, ".Sh PREVIOUS\n.Sh NEXT\nAFTERMARK\n", 0, "PREVIOUS"),
        (
            MDOC,
            ".Sh PREVIOUS\n.Sm off\n.Sh NEXT\nAFTERMARK\n",
            1,
            "PREVIOUS",
        ),
        (MDOC, ".Sm off\n.Ss NEXT\nAFTERMARK\n", 0, "DESCRIPTION"),
        (
            MAN,
            ".SH PREVIOUS\n.ft R\n.SH NEXT\nAFTERMARK\n",
            0,
            "PREVIOUS",
        ),
        (
            MAN,
            ".SS PREVIOUS\n.ft R\n.SS NEXT\nAFTERMARK\n",
            0,
            "PREVIOUS",
        ),
    ] {
        let source = format!("{header}{body}");
        let content = mant_loader::load_roff_bytes(source.as_bytes()).expect("parse roff");
        let document = content.document.as_ref().expect("document");
        let next = sections(&document.sections)
            .find(|section| section.id.as_str() == "next")
            .expect("NEXT owner");
        assert_eq!(next.spacing_before_lines, gap, "{source}");
        let text = mant_render::render_query_man(&content);
        let lines: Vec<_> = text.split('\n').collect();
        let heading = lines
            .iter()
            .position(|line| line.trim_start_matches(' ') == "NEXT")
            .expect("printed NEXT");
        let gap = usize::from(gap);
        assert_eq!(lines[heading - gap - 1].trim_start_matches(' '), before);
        assert!(
            lines[heading - gap..heading]
                .iter()
                .all(|line| line.is_empty())
        );
        assert_eq!(lines[heading + 1].trim_start_matches(' '), "AFTERMARK");
    }
}

#[test]
fn native_heading_handoff_preserves_exact_marker_rows_and_body_owner() {
    // Each complete case ran fixed CVS ASCII/UTF-8/HTML/tree/lint first.
    // With PD=0, pre_SS does not flush: term_word leaves the old NBRZW and
    // break markers in the input until post_SH(HEAD) executes term_newln.
    // A real no-fill NODE_LINE does close the earlier of two source rows.
    let header = MAN.replace(".SH DESCRIPTION", ".SH FIRST");
    for mode in ["", ".nf\n"] {
        for (input, split_source, suppress_heading, mut blanks) in [
            ("\\p\n", false, true, 1),
            ("\\p\\&\n\\p\\&\n", true, false, 2),
            ("\\p\\& \\p\\&\n", false, false, 2),
            ("\\&\n\\&\n", true, false, 0),
            ("\\& \\&\n", false, false, 0),
        ] {
            let source = format!("{header}.PD 0\n{mode}{input}.SS NEXT\nAFTERMARK\n");
            let content = mant_loader::load_roff_bytes(source.as_bytes()).expect("parse roff");
            let document = content.document.as_ref().expect("document");
            let first = &document.sections[1];
            let next = &first.children[0];
            let old_literal_row = !mode.is_empty() && split_source;
            assert_eq!(first.blocks.len(), usize::from(old_literal_row), "{source}");
            if old_literal_row {
                assert!(
                    matches!(first.blocks.as_slice(), [mant_ir::Block::Preformatted { children, source: Some(span), .. }]
                    if mant_ir::inline_plain_text(children).is_empty() && span.line == 7)
                );
                blanks += usize::from(input.starts_with("\\&"));
            }
            assert_eq!(
                next.spacing_before_lines,
                u16::from(suppress_heading),
                "{source}"
            );
            assert_eq!(
                next.heading.plain_text().trim(),
                if suppress_heading { "" } else { "NEXT" },
                "{source}"
            );
            assert!(
                matches!(next.blocks.as_slice(), [mant_ir::Block::Paragraph { children, source: Some(span), .. }]
                if mant_ir::inline_plain_text(children) == "AFTERMARK" && span.line as usize == source.lines().count()),
                "{source}: {next:#?}"
            );
            let rendered = mant_render::render_query_man(&content);
            let suffix: Vec<_> = rendered
                .lines()
                .skip_while(|line| *line != "FIRST")
                .skip(1)
                .map(|line| line.trim_start_matches(' '))
                .collect();
            let mut expected = vec![""; blanks];
            if !suppress_heading {
                expected.push("NEXT");
            }
            expected.push("AFTERMARK");
            assert_eq!(suffix, expected, "{source}");
        }
    }
}
