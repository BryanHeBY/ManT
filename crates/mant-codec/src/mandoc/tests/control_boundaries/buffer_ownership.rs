use super::*;

const MAN: &str = ".TH TEST 1 \"September 28, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n";
const MDOC: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn section_inlines(document: &mant_ir::Document) -> Vec<Inline> {
    document.sections[1]
        .blocks
        .iter()
        .flat_map(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                children.clone()
            }
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn continued_no_fill_words_share_native_acceptance() {
    // Exact pristine CVS ASCII/UTF-8/HTML/tree/lint probes ran before this
    // assertion. term_fill() scans the complete tcol buffer (term.c:263-367);
    // NONEWLINE keeps the two TEXT nodes in that same buffer (man_term.c:922).
    for (prefix, word, expected) in [
        ("BEFORE\\c", "\\p D", "BEFORE\nD\nAFTER"),
        ("BEFORE \\c", "\\p D", "BEFORE\n\nAFTER"),
        ("BEFORE\\c", "\\pD", "BEFORED\nAFTER"),
        ("BEFORE\\c", "\\p\\& D", "BEFORE\nD\nAFTER"),
    ] {
        let source = format!("{MAN}.nf\n{prefix}\n{word}\nAFTER\n.fi\n.SH NEXT\nEND\n");
        let document = parse_manual_bytes(
            std::path::Path::new("continued-native-buffer.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            inline_text(&section_inlines(&document)),
            expected,
            "{source}\n{document:#?}"
        );
    }
}

#[test]
fn delayed_accepted_glyph_keeps_its_original_native_owner() {
    // The eight exact owner/carrier probes ran on pristine CVS first.
    // encode1() writes P before arming BACKBEFORE (term.c:901-927); its
    // accepted cell cannot become the following operand's rejected suffix.
    for carrier in ["No", "Em", "Sy"] {
        for next in [".No \"\\p  D\"", ".Lk https://new.example \"\\p  D\""] {
            let source = format!("{MDOC}.{carrier} \\zP\n{next}\n.No AFTER\n.Sh NEXT\n.No END\n");
            let document = parse_manual_bytes(
                std::path::Path::new("delayed-native-owner.1"),
                source.as_bytes(),
            )
            .unwrap();
            assert_eq!(
                inline_text(&section_inlines(&document)).trim_end_matches('\n'),
                "P",
                "{source}\n{document:#?}"
            );
            let output = section_inlines(&document);
            assert_eq!(output.iter().any(|node| matches!(node, Inline::Emphasis { children } if inline_text(children) == "P")), carrier == "Em", "{source}: {output:#?}");
            assert_eq!(output.iter().any(|node| matches!(node, Inline::Strong { children } if inline_text(children) == "P")), carrier == "Sy", "{source}: {output:#?}");
            assert!(output.iter().all(|node| !matches!(node, Inline::Link { children, .. } if inline_text(children).contains('P'))), "the delayed preceding glyph cannot acquire the following link: {source}: {output:#?}");
        }
    }
}

#[test]
fn generated_words_reuse_the_already_projected_marker_row() {
    // Exact sources ran pristine CVS ASCII/UTF-8/HTML/tree/lint first.
    // termp_lk_pre(): description -> tight ':' -> URI uses the same buffer.
    // No has no generated colon/URI, and its next pass is rejected instead.
    for (body, expected) in [
        (
            ".Lk https://ex.org \"D\\p \\p\"",
            "D\n:\nhttps://ex.org AFTER",
        ),
        (".No \"D\\p \\p\"", "D"),
    ] {
        let source = format!("{MDOC}{body}\n.No AFTER\n.Sh NEXT\n.No END\n");
        let document = parse_manual_bytes(
            std::path::Path::new("generated-native-pass.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            inline_text(&section_inlines(&document)).trim_end_matches('\n'),
            expected,
            "{source}\n{document:#?}"
        );
    }
}

#[test]
fn no_break_flush_retires_the_live_no_fill_owner() {
    // All six exact sources ran pristine CVS ASCII/UTF-8/HTML/tree/lint
    // before this assertion. roff_term_pre_mc() consumes the same native
    // buffer under NOBREAK (roff_term.c:147-151); a continuing source line
    // does not grant acceptance to a rejected prefix (term.c:143-146).
    for (prefix, word, expected) in [
        ("", "\\p D\\c", " AFTER"),
        (".No BEFORE\\c\n", "\\p D\\c", "BEFORE\nD AFTER"),
        ("", "X\\p Y\\c", "X\nY AFTER"),
        (".No BEFORE\\c\n", "X\\p Y\\c", "BEFOREX\nY AFTER"),
        ("", "\\zX\\p  D\\c", "X\nD AFTER"),
        (".No BEFORE\\c\n", "\\zX\\p  D\\c", "BEFOREX\nD AFTER"),
    ] {
        let source =
            format!("{MDOC}.nf\n{prefix}.No \"{word}\"\n.mc\n.No AFTER\n.fi\n.Sh NEXT\n.No END\n");
        let document = parse_manual_bytes(
            std::path::Path::new("literal-no-break-consumption.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            inline_text(&section_inlines(&document)),
            expected,
            "{source}\n{document:#?}"
        );
    }
}

#[test]
fn empty_owners_do_not_advance_acceptance_past_a_replacement_glyph() {
    // Exact pristine CVS ASCII/UTF-8/tree/lint ran first. BACKBEFORE can
    // retreat across an empty word's trailing blank (term.c:901-908), so
    // its old empty interval is not an accepted-prefix consumption event.
    for (word, expected) in [
        ("Y", "XY AFTER"),
        ("Y\\p D", "XY\nD AFTER"),
        ("Y\\p \\p", "XY"),
    ] {
        let source =
            format!("{MDOC}.No \\zX No \"\" Ns No \"{word}\"\n.No AFTER\n.Sh NEXT\n.No END\n");
        let document = parse_manual_bytes(
            std::path::Path::new("empty-native-owner-retreat.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            inline_text(&section_inlines(&document)).trim_end_matches('\n'),
            expected,
            "{source}\n{document:#?}"
        );
    }
}

#[test]
fn no_fill_scope_entry_preserves_the_continuing_native_unit() {
    // Exact pristine ASCII/UTF-8/HTML/tree/lint probes ran first.
    // Bf pre pushes a font, not a line (mdoc_term.c:1799); NODE_LINE
    // remains conditional on NONEWLINE before term_word clears it.
    let source =
        format!("{MDOC}.nf\nBEFORE\\c\n.Bf -emphasis\n\\p D\n.Ef\nAFTER\n.fi\n.Sh NEXT\n.No END\n");
    let document =
        parse_manual_bytes(std::path::Path::new("no-fill-scope.1"), source.as_bytes()).unwrap();
    let inlines = section_inlines(&document);
    assert_eq!(inline_text(&inlines), "BEFORE\nD\nAFTER", "{document:#?}");
    assert!(inlines.iter().any(
        |inline| matches!(inline, Inline::Emphasis { children } if inline_text(children) == "D")
    ));
}

#[test]
fn previous_literal_rows_cannot_settle_a_new_bare_zero_advance_request() {
    // Exact pristine probes ran before writing this expectation. A bare
    // BACKAFTER writes no current-row cell; earlier completed rows cannot
    // make term_newln() flush it (term.c:475-481,924-927).
    let source = format!("{MDOC}.nf\n.No X\n.No \\z\n.fi\n.No A No B\n.Sh NEXT\n.No END\n");
    let document = parse_manual_bytes(
        std::path::Path::new("bare-zero-after-row.1"),
        source.as_bytes(),
    )
    .unwrap();
    let blocks = &document.sections[1].blocks;
    assert!(
        matches!(&blocks[0], Block::Preformatted { children, .. } if inline_text(children) == "X"),
        "{document:#?}"
    );
    assert!(
        matches!(&blocks[1], Block::Paragraph { children, .. } if inline_text(children) == "AB"),
        "{document:#?}"
    );
}

#[test]
fn invisible_graph_passes_keep_distinct_native_row_events() {
    // All exact sources ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // NBRZW sets graph in term_fill() despite producing no scalar
    // (term.c:340-349). Equal projection offsets are distinct passes.
    for (mode, expected) in [("", "X\n\n\nY After"), (".nf\n", "X\n\n\nY\nAfter")] {
        let end = if mode.is_empty() { "" } else { ".fi\n" };
        let source = format!(
            "{MDOC}{mode}.No \"X\\p \\&\\p \\&\\p Y\"\n.No After\n{end}.Sh NEXT\n.No END\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("zero-graph-rows.1"), source.as_bytes())
                .unwrap();
        assert_eq!(
            inline_text(&section_inlines(&document)),
            expected,
            "{source}\n{document:#?}"
        );
    }
}

#[test]
fn new_display_native_units_start_inside_their_actual_output_owner() {
    // Exact D1/Dl sources ran pristine probes first. Their BLOCK pre and
    // BODY post call term_newln (termp_d1_pre/termp_bl_post), so a new
    // unit cannot inherit the former display's projection offset.
    for display in ["D1", "Dl"] {
        let source = format!(
            "{MDOC}.{display} X\\p\n.{display} \"\\p DROP\"\n.{display} Tail\n.Sh NEXT\n.No END\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("display-native-owner.1"),
            source.as_bytes(),
        )
        .unwrap();
        let rows = document.sections[1]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Preformatted { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(rows, ["X", "", "Tail"], "{source}\n{document:#?}");
    }
}

#[test]
fn heading_post_keeps_an_accepted_prefix_and_a_rejected_empty_row() {
    // Exact pristine probes ran first. Ss HEAD post calls term_newln
    // (mdoc_term.c:1537); a later rejected pass ends its own row
    // (term.c:143-146,250-253) after the accepted X prefix.
    let source = format!("{MDOC}.Ss No \"X\\p \\p DROP\"\n.No BodyWord\n.Sh NEXT\n.No END\n");
    let document = parse_manual_bytes(
        std::path::Path::new("heading-native-row.1"),
        source.as_bytes(),
    )
    .unwrap();
    let section = &document.sections[1].children[0];
    assert_eq!(
        inline_text(&section.heading.content),
        "X\n",
        "{document:#?}"
    );
    assert!(
        matches!(&section.blocks[0], Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")
    );
}

#[test]
fn invisible_native_graphs_do_not_consume_the_pending_glyph_retreat() {
    // Exact pristine probes ran first. NBRZW is buffered without encode1
    // (term.c:610-638), so BACKBEFORE retreats over the later source blank,
    // preserving the earlier word separator as well as its glyph.
    for (next, expected) in [
        (".No \"\\p\\& D\"", "P D\nAFTER"),
        (
            ".Lk https://ex.org \"\\p\\& D\"",
            "P D:\nhttps://ex.org AFTER",
        ),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.No \\zP\n{next}\n.No AFTER\n.Sh ENDTEST\nDONE\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("zero-graph-retreat.1"),
            source.as_bytes(),
        )
        .unwrap();
        let output = document.sections[0]
            .blocks
            .iter()
            .flat_map(|block| match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    children.clone()
                }
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();
        assert_eq!(inline_text(&output), expected, "{source}\n{document:#?}");
    }
}

#[test]
fn generated_words_keep_their_executed_separator_before_a_delayed_glyph() {
    // Exact pristine ASCII/UTF-8/HTML/tree/lint probes preceded these
    // expectations. Lk executes description, tight colon, then target
    // (mdoc_term.c:1881-1916). term_word() writes its separator before
    // encode1 can arm a delayed glyph (term.c:573-589,901-927).
    for (label, expected) in [
        ("\\z", "BEFORE :https://e.test AFTER"),
        ("   ", "BEFORE    : https://e.test AFTER"),
    ] {
        let source =
            format!("{MDOC}BEFORE\n.Lk https://e.test \"{label}\"\nAFTER\n.Sh NEXT\n.No EndWord\n");
        let document = parse_manual_bytes(
            std::path::Path::new("generated-native-separator.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            inline_text(&section_inlines(&document)),
            expected,
            "{source}\n{document:#?}"
        );
    }
}

#[test]
fn a_zero_scalar_marker_pass_consumes_the_previous_rows_breakable_padding() {
    // Exact pristine probes preceded this assertion. NBRZW sets graph but
    // projects no scalar (term.c:340-349); the accepted pass still consumes
    // its preceding breakable blanks (term_flushln():205-207).
    let source = format!("{MDOC}BEFORE\n\\p\\& D\nAFTER\n.Sh NEXT\n.No EndWord\n");
    let document = parse_manual_bytes(
        std::path::Path::new("zero-scalar-native-pass.1"),
        source.as_bytes(),
    )
    .unwrap();
    assert_eq!(
        inline_text(&section_inlines(&document)),
        "BEFORE\nD AFTER",
        "{document:#?}"
    );
}

#[test]
fn field_receipt_splits_a_later_word_after_an_accepted_overstrike_prefix() {
    // The exact historic matrix source ran pristine ASCII/UTF-8/HTML/tree
    // and lint before this assertion. The marker splits a later HANG field
    // pass, not the previous BACKBEFORE glyph's source owner (term.c:276-
    // 367,901-908). The existing fixture deliberately lacks a NAME section.
    let source = concat!(
        ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n",
        ".Bl -hang -width 4n\n.It Head\n.No H\\z X \\p YY Z\n.El\n"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("accepted-overstrike-field.1"),
        source.as_bytes(),
    )
    .unwrap();
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("{document:#?}");
    };
    let body = items[0]
        .description
        .iter()
        .flat_map(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                children.clone()
            }
            _ => Vec::new(),
        })
        .collect::<Vec<_>>();
    assert_eq!(inline_text(&body), "H X YY\nZ", "{document:#?}");
}

#[test]
fn native_retirement_keeps_only_the_accepted_plain_word_tail() {
    // Exact pristine probes preceded this assertion. term_field() defers
    // blank output until another graph (term.c:389-427); term_newln() may
    // accept BEFORE while leaving its ordinary trailing blank unprinted.
    let source = format!("{MDOC}.nf\nBEFORE \\c\n.br\n\\p D\nAFTER\n.fi\n.Sh NEXT\n.No END\n");
    let document = parse_manual_bytes(
        std::path::Path::new("accepted-native-tail.1"),
        source.as_bytes(),
    )
    .unwrap();
    let rows = document.sections[1]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Preformatted { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(rows, "BEFORE\n\nAFTER", "{document:#?}");
}

#[test]
fn separate_rejected_native_units_keep_their_separate_completed_rows() {
    // Exact pristine probes preceded this assertion. The br request closes
    // the first unit before the second Lk writes another buffer; each nbr=0
    // pass ends its own row (roff_term.c:69, term.c:143-146,250-253).
    let source = format!(
        "{MDOC}.Lk https://ex.org \"\\p D\"\n.br\n.Lk https://ex.org \"\\p D\"\n.No AFTER\n.Sh NEXT\n.No END\n"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("rejected-native-units.1"),
        source.as_bytes(),
    )
    .unwrap();
    let rows = document.sections[1]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::VerticalSpace { lines, .. } => Some(*lines),
            _ => None,
        })
        .sum::<u16>();
    assert_eq!(rows, 2, "{document:#?}");
    assert_eq!(document_link_targets(&document).len(), 2);
}

#[test]
fn section_labels_own_authored_blanks_but_not_formatter_prefixes() {
    // Exact pristine ASCII/UTF-8/HTML/tree/lint probes preceded these
    // assertions. Sx only pushes underline in termp_under_pre(); the
    // automatic separator is written by term_word before label encoding
    // (mdoc_term.c:1969, term.c:573-589). mdoc_sx_pre() links the operand.
    for (body, expected_label, expected_row) in [
        (
            "Continue with\n.Sx DETAILS\n",
            "DETAILS",
            "Continue with DETAILS",
        ),
        (
            "See\n.Sx White Space Splitting\n",
            "White Space Splitting",
            "See White Space Splitting",
        ),
        (
            "BEFORE\n.Sx \"  DETAILS\"\n",
            "  DETAILS",
            "BEFORE   DETAILS",
        ),
        (
            "BEFORE\n.Sm off\n.Sx DETAILS\n.Sm on\nAFTER\n",
            "DETAILS",
            "BEFORE DETAILS AFTER",
        ),
        ("\\zX\n.Sx DETAILS\nAFTER\n", "DETAILS", "XDETAILS AFTER"),
        // The retained filled G-IND difference omits this device's one
        // initial automatic separator, not authored or between-word space.
        ("\\z\n.Sx DETAILS\nAFTER\n", "ETAILS", "ETAILS AFTER"),
    ] {
        let heading = if expected_label == "White Space Splitting" {
            "\"White Space Splitting (Field Splitting)\""
        } else {
            "DETAILS"
        };
        let source = format!("{MDOC}{body}.Sh {heading}\nTarget content.\n");
        let document = parse_manual_bytes(
            std::path::Path::new("section-label-native-prefix.1"),
            source.as_bytes(),
        )
        .unwrap();
        let output = section_inlines(&document);
        assert_eq!(
            inline_text(&output),
            expected_row,
            "{source}\n{document:#?}"
        );
        let links = output
            .iter()
            .filter_map(|node| match node {
                Inline::Link {
                    target: mant_ir::LinkTarget::Section { .. },
                    children,
                    ..
                } => Some(inline_text(children)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(links, [expected_label], "{source}\n{document:#?}");
    }
}

#[test]
fn invisible_section_operands_keep_their_native_word_boundary() {
    // The exact pristine source ran first. NBRZW is a real graph cell but
    // has no label scalar (term_fill():340-349); its two ordinary word
    // boundaries must not be suppressed by semantic annotation ownership.
    let source = format!("{MDOC}BEFORE\n.Sx \\&\nAFTER\n.Sh DETAILS\nTarget content.\n");
    let document = parse_manual_bytes(
        std::path::Path::new("invisible-section-label.1"),
        source.as_bytes(),
    )
    .unwrap();
    assert_eq!(
        inline_text(&section_inlines(&document)),
        "BEFORE  AFTER",
        "{document:#?}"
    );
}

#[test]
fn accepted_invisible_rows_belong_to_the_consumed_unit_not_its_last_word() {
    // Exact pristine probes preceded this assertion. NODE_LINE closes the
    // generated inset BODY cell before its first word (mdoc_term.c:314,
    // 764). NBRZW occupies the following row even when the last empty word's
    // trailing automatic blank lies outside the accepted nbr (term.c:340-
    // 349,389-427). That last owner cannot erase the row witness.
    let source = format!(
        "{MDOC}.nf\n.Bl -inset\n.It Xo first\nsecond\n.Xc\n.No \\& No \"\"\n.br\n.No BODY\n.El\n.fi\n"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("invisible-native-unit-row.1"),
        source.as_bytes(),
    )
    .unwrap();
    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("{document:#?}");
    };
    let rows = items[0]
        .description
        .iter()
        .filter_map(|block| match block {
            Block::Preformatted { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(rows, ["", "BODY"], "{document:#?}");
    let completed_rows = items[0]
        .description
        .iter()
        .filter_map(|block| match block {
            Block::VerticalSpace { lines, .. } => Some(*lines),
            _ => None,
        })
        .sum::<u16>();
    // The generated BODY row and the later accepted invisible source row
    // have separate output owners. The engine regression for this exact
    // source also checks their actual rendered row sequence.
    assert_eq!(completed_rows + 1, 2, "{document:#?}");
}

#[test]
fn semantic_and_row_metadata_cannot_hide_a_definition_term_break() {
    // The exact pristine ASCII/UTF-8/HTML/tree/lint source ran first.
    // Pp's print_bvspace() executes native line/space events before the
    // next operand (mdoc_term.c:583-628). Private projection anchors must
    // neither hide that boundary nor change its post-drain term index.
    let source = format!(
        "{MDOC}.Bl -tag -width 10n\n.It Xo\n.Sx FIRST\n.Pp\n.Sx SECOND\n.Xc\n.No BODY\n.El\n.Sh FIRST\n.No A\n.Sh SECOND\n.No B\n"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("semantic-native-term-break.1"),
        source.as_bytes(),
    )
    .unwrap();
    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("{document:#?}");
    };
    assert_eq!(items[0].terms.len(), 2, "{document:#?}");
    for (term, label) in items[0].terms.iter().zip(["FIRST", "SECOND"]) {
        assert!(
            term.iter().any(|node| matches!(node,
                Inline::Link { target: mant_ir::LinkTarget::Section { .. }, children, .. }
                    if inline_text(children) == label)),
            "{document:#?}"
        );
    }
}
