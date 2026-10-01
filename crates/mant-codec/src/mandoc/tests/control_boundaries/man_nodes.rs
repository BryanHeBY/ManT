use super::*;

// The selected UTF-8 formatter preserves CVS's executed glyphs: mdoc Ao/Ac
// select the Unicode angle glyphs, while man post_UR always calls term_word
// with literal ASCII "<"/">" (man_term.c:896-908). These assertions were
// rechecked against pristine ASCII/UTF-8/tree/lint before synchronizing them.

#[test]
fn man_paragraph_node_exit_updates_the_previous_font_register() {
    // Each exact UR/MT x PP/P/LP input was checked with fixed CVS -Tascii
    // and -Tlint. man_term.c::print_man_node() applies term_fontrepl() at
    // BLOCK, HEAD, and BODY entry/exit even though pre_PP has no post handler.
    for (open, close, target) in [
        ("UR", "UE", "https://example.org"),
        ("MT", "ME", "user@example.org"),
    ] {
        for paragraph in ["PP", "P", "LP"] {
            let source = format!(
                ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.{open} \\fP{target}\n.{paragraph}\n\\fBlabel\n.{close}\n\\fPafter\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("paragraph-font-register.1"),
                source.as_bytes(),
            )
            .unwrap();
            let strong = strong_document_text(&document);
            assert!(
                strong.contains("label"),
                "{open}/{paragraph}: {document:#?}"
            );
            assert!(
                !strong.contains(target),
                "{open}/{paragraph}: {document:#?}"
            );
            assert!(!strong.contains('⟩'), "{open}/{paragraph}: {document:#?}");
            assert!(
                !strong.contains("after"),
                "{open}/{paragraph}: {document:#?}"
            );
        }
    }
}

#[test]
fn automatic_man_paragraph_space_consumes_negative_sp_debt() {
    // Every exact PP/P/LP/SY/HP/IP/TP input was checked with fixed CVS
    // -Tascii/-Tlint. man_term.c::print_bvspace() calls term_vspace(), whose
    // skipvsp rule consumes the preceding .sp -1 before producing any gap.
    for (macro_name, tail) in [
        ("PP", ".PP\nAFTER"),
        ("P", ".P\nAFTER"),
        ("LP", ".LP\nAFTER"),
        ("SY", ".SY call\narg\n.YS"),
        ("HP", ".HP\nAFTER"),
        ("IP", ".IP tag 4\nAFTER"),
        ("TP", ".TP\ntag\nAFTER"),
    ] {
        let source =
            format!(".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\n.sp -1\n{tail}\n");
        let document = parse_manual_bytes(
            std::path::Path::new("negative-sp-automatic-gap.1"),
            source.as_bytes(),
        )
        .unwrap();
        let gap = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph {
                    children, layout, ..
                }
                | Block::Preformatted {
                    children, layout, ..
                } if inline_text(children).contains("AFTER")
                    || inline_text(children).contains("call") =>
                {
                    Some(layout.spacing_before_lines)
                }
                Block::DefinitionList { layout, .. } if matches!(macro_name, "IP" | "TP") => {
                    Some(layout.spacing_before_lines)
                }
                _ => None,
            });
        assert_eq!(gap, Some(0), "{macro_name}: {document:#?}");
    }
}

#[test]
fn man_paragraph_spacing_uses_source_siblings_through_rs_only() {
    // All nine exact outer/inner combinations were checked with fixed CVS
    // -Tascii/-Tlint. man_term.c::print_bvspace() climbs a first-child RS,
    // then stops at the enclosing PP/P/LP BODY: outer output does not count
    // as an extra predecessor for that inner paragraph.
    for outer in ["PP", "P", "LP"] {
        for inner in ["PP", "P", "LP"] {
            let source = format!(
                ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\n.{outer}\n.RS\n.{inner}\ncontent\n.RE\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("rs-first-paragraph.1"),
                source.as_bytes(),
            )
            .unwrap();
            let spacing = document.sections[0]
                .blocks
                .iter()
                .find_map(|block| match block {
                    Block::Paragraph {
                        children, layout, ..
                    } if inline_text(children) == "content" => Some(layout.spacing_before_lines),
                    _ => None,
                })
                .expect("nested paragraph");
            assert_eq!(spacing, 1, "{outer}/{inner}: {document:#?}");
        }
    }

    // These exact controls were also checked with fixed CVS -Tascii/-Tlint:
    // a sibling before RS adds a gap, nested first-child RS wrappers do not,
    // and a UR BODY stops the source-predecessor climb.
    for (name, body, expected_spacing) in [
        ("rs-after-sibling", ".PP\nmiddle\n.RS\n.PP\ncontent\n.RE", 1),
        ("rs-chain-first", ".PP\n.RS\n.RS\n.PP\ncontent\n.RE\n.RE", 1),
        (
            "rs-link-body-first",
            ".RS\nmiddle\n.UR x\n.PP\ncontent\n.UE\n.RE",
            0,
        ),
        (
            "rs-first-transparent-ft",
            ".PP\n.RS\n.ft B\n.PP\ncontent\n.RE",
            1,
        ),
        (
            "rs-first-transparent-pd",
            ".PP\n.RS\n.PD 2\n.PP\ncontent\n.RE",
            1,
        ),
        (
            "rs-after-real-sibling",
            ".PP\n.RS\nfirst\n.ft B\n.PP\ncontent\n.RE",
            1,
        ),
    ] {
        let source = format!(".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\n{body}\n");
        let document = parse_manual_bytes(
            std::path::Path::new("rs-paragraph-source-sibling.1"),
            source.as_bytes(),
        )
        .unwrap();
        let spacing = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph {
                    children, layout, ..
                } if inline_text(children).contains("content") => Some(layout.spacing_before_lines),
                _ => None,
            })
            .expect("content paragraph");
        assert_eq!(spacing, expected_spacing, "{name}: {document:#?}");
    }
}

#[test]
fn man_structural_paragraphs_share_the_cvs_source_predecessor_rule() {
    // Exact HP/IP/TP variants were checked with fixed CVS -Tascii/-Tlint.
    // man_term.c::pre_HP/pre_IP/pre_TP all call print_bvspace() at BLOCK
    // entry. Only an actual source sibling (possibly reached through RS)
    // adds the current PD distance; output before a PP or UR BODY does not.
    for (scope, prefix, suffix, expected) in [
        ("first-rs", ".PP\n.RS\n", ".RE\n", 1),
        ("link-rs", ".UR x\n.RS\n", ".RE\n.UE\n", 0),
        ("sibling-rs", ".PP\n.RS\nfirst\n", ".RE\n", 1),
    ] {
        for (macro_name, head) in [("HP", ""), ("IP", " tag 4"), ("TP", "\ntag")] {
            let source = format!(
                ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\n{prefix}.{macro_name}{head}\ncontent\n{suffix}"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("man-structural-source-sibling.1"),
                source.as_bytes(),
            )
            .unwrap();
            let blocks = &document.sections[0].blocks;
            let spacing = blocks.iter().find_map(|block| match block {
                Block::Paragraph {
                    children, layout, ..
                } if macro_name == "HP" && inline_text(children).contains("content") => {
                    Some(layout.spacing_before_lines)
                }
                Block::DefinitionList { layout, .. } if macro_name != "HP" => {
                    Some(layout.spacing_before_lines)
                }
                _ => None,
            });
            assert_eq!(
                spacing,
                Some(expected),
                "{scope}/{macro_name}: {document:#?}"
            );
        }
    }
}

#[test]
fn man_synopsis_spacing_obeys_native_previous_sibling_and_pd() {
    // Each exact source was checked with fixed CVS -Tascii/-Tlint. The
    // BLOCK path of man_term.c::pre_SY() calls print_bvspace() unless the
    // direct previous nontransparent sibling is another SY; YS separates
    // two declarations, and PD changes the requested number of rows.
    for (name, body, label, expected) in [
        ("after-text", "BEFORE\n.SY call\narg\n.YS\n", "call arg", 1),
        (
            "after-pd",
            "BEFORE\n.PD 2\n.SY call\narg\n.YS\n",
            "call arg",
            2,
        ),
        (
            "after-ys",
            ".SY first\narg\n.YS\n.SY second\narg\n.YS\n",
            "second arg",
            1,
        ),
        (
            "direct-sy",
            ".SY first\narg\n.SY second\narg\n.YS\n",
            "second arg",
            0,
        ),
        (
            "no-fill",
            ".nf\nBEFORE\n.SY call\narg\n.YS\n.fi\n",
            "call\narg",
            1,
        ),
    ] {
        let heading = if name == "after-ys" || name == "direct-sy" {
            "SYNOPSIS"
        } else {
            "DESCRIPTION"
        };
        let source = format!(".TH TEST 1 \"2026-09-28\"\n.SH {heading}\n{body}");
        let document = parse_manual_bytes(
            std::path::Path::new("man-synopsis-source-spacing.1"),
            source.as_bytes(),
        )
        .unwrap();
        let spacing = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph {
                    children, layout, ..
                }
                | Block::Preformatted {
                    children, layout, ..
                } if inline_text(children).contains(label) => Some(layout.spacing_before_lines),
                _ => None,
            });
        assert_eq!(spacing, Some(expected), "{name}: {document:#?}");
    }
}

#[test]
fn man_synopsis_body_post_restores_font_for_following_text() {
    // Exact input checked with fixed CVS -Tascii/-Thtml/-Tlint.
    // man_term.c::post_SY(BODY) ends the row and print_man_node() then
    // replaces the active font at BODY and BLOCK exits.
    let source = b".TH TEST 1\n.SH SYNOPSIS\n.SY call\n.ft I\narg\n.YS\nafter\n";
    let document =
        parse_manual_bytes(std::path::Path::new("man-synopsis-post-font.1"), source).unwrap();
    let emphasized = emphasized_document_text(&document);
    assert!(emphasized.contains("arg"), "{document:#?}");
    assert!(!emphasized.contains("after"), "{document:#?}");
}

#[test]
fn man_synopsis_and_hanging_body_posts_settle_their_own_no_fill_rows() {
    // Exact inputs checked with fixed CVS -Tascii and -Tlint.  man_term.c
    // post_SY() ends the synopsis BODY row at .YS; post_HP() ends the hanging
    // BODY row before the following PP spacing request executes.
    let synopsis = b".TH TEST 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.nf\n.SY call\n\\zX\\c\n.YS\nY\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("man-sy-post.1"), synopsis)
        .expect("parse man synopsis post fixture");
    let [
        Block::Preformatted {
            children: synopsis, ..
        },
        Block::Preformatted {
            children: outer, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("unexpected synopsis output: {:#?}", document.sections);
    };
    assert_eq!(inline_text(synopsis), "call\nX", "{synopsis:?}");
    assert_eq!(inline_text(outer), "Y", "{outer:?}");

    let hanging =
        b".TH TEST 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.nf\n.HP 7\n\\zX\\c\n.PP\nY\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("man-hp-post.1"), hanging)
        .expect("parse man hanging post fixture");
    let [
        Block::Preformatted {
            children: inner,
            source: Some(source),
            ..
        },
        Block::Preformatted {
            children: outer, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("unexpected hanging output: {:#?}", document.sections);
    };
    assert_eq!(inline_text(inner), "X", "{inner:?}");
    assert_eq!(source.line, 5, "{source:?}");
    assert_eq!(inline_text(outer), "Y", "{outer:?}");
}
