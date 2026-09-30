use super::*;

// These exact sources (including every dynamic Bx operand below) were rerun
// with pristine CVS ASCII, UTF-8, HTML and lint before changing expectations.
// mdoc_term.c::termp_lk_pre() visits all description operands, then the
// colon and URI in the same stream; post_bx() retains the authored spelling.
// Native assertions preserve its visible word and hard-line sequence. Portable
// enhancement assertions explicitly select the Markdown encoder.

#[test]
fn zero_advance_crosses_empty_enclosures_and_atomic_mdoc_output() {
    let cases = [
        (
            "empty-enclosure",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\n.Dq\n.No B\n".as_slice(),
            "AX“” B",
        ),
        (
            "include",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.In A\\zX\n".as_slice(),
            "<A>",
        ),
        (
            "bsd",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.Bx A\\zX\n".as_slice(),
            "ABSD",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-{label}.1")),
            source,
        )
        .expect("parse zero-advance atomic output");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.sections[0].blocks
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn bsd_reference_executes_font_operands_before_native_and_portable_text() {
    let document = parse_manual_bytes(
        std::path::Path::new("bsd-reference-font-state.1"),
        b".Dd September 12, 2026\n.Dt BSD-FONT 1\n.Os\n.Sh DESCRIPTION\n.Bx \\fB\n.Li \\fPZ\n.Bx \\fB-devel\n.Li \\fPZ\n",
    )
    .expect("parse Bx formatter-state fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections[0].blocks);
    };

    // Pristine CVS mdoc_validate.c::post_bx() appends BSD to the authored
    // argument after its font controls. Native readers retain that spelling;
    // the separately selected portable Markdown spelling keeps the expansion.
    assert_eq!(inline_text(children), "BSD Z -develBSD Z");
    assert_eq!(strong_native_text(children), "BSDZ-develBSDZ");
    let portable = crate::encode::render_inline_fragment(
        children,
        crate::encode::MarkdownFragmentOptions::default(),
    );
    assert!(
        portable.contains("BSD (currently under development)"),
        "{portable}"
    );
    assert_eq!(portable.matches("**Z**").count(), 2, "{portable}");
}

#[test]
fn bsd_native_and_portable_text_share_the_executed_word_boundaries() {
    for (label, operand, expected) in [
        ("lifecycle-word-end-break", r"-alpha\p", "-alphaBSD\nAFTER"),
        ("control-only-word-end-break", r"\p", "BSD\nAFTER"),
        (
            "lifecycle-source-continuation",
            r"-beta\c",
            "-betaBSD AFTER",
        ),
        (
            "noncanonical-lifecycle-zero-advance",
            r"-devel\zX",
            "-develBSD AFTER",
        ),
        ("control-only-zero-advance", r"\z", "SD AFTER"),
        ("completed-zero-advance", r"\zX", "BSD AFTER"),
        ("canceled-zero-advance", r"\z\c", "BSD AFTER"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt BSD-STATE 1\n.Os\n.Sh DESCRIPTION\n.Bx {operand}\n.No AFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("bsd-reference-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse Bx hidden execution fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}");
    }

    let document = parse_manual_bytes(
        std::path::Path::new("bsd-reference-hidden-font-break.1"),
        b".Dd September 12, 2026\n.Dt BSD-STATE 1\n.Os\n.Sh DESCRIPTION\n.Bx \\fB-devel\\p\n.Li \\fPZ\n",
    )
    .expect("parse styled Bx hidden execution fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "-develBSD\nZ");
    assert!(
        strong_native_text(children).contains("-develBSDZ"),
        "hidden font execution must style replacement and follower: {children:?}"
    );
}

#[test]
fn bsd_reference_replacement_preserves_boundaries_and_exact_arity() {
    let document = parse_manual_bytes(
        std::path::Path::new("bsd-reference-replacement-boundaries.1"),
        b".Dd September 12, 2026\n.Dt BSD-STATE 1\n.Os\n.Sh DESCRIPTION\n.No A\n.Bx \\fB\n.Li \\fPZ\n.No A\n.Bx \\p\n.No Z\n.Bx -alpha \"\"\n.Bx 4.3 Tahoe\n",
    )
    .expect("parse Bx replacement boundary fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections);
    };

    assert_eq!(
        inline_text(children),
        "A BSD Z A BSD\nZ -alphaBSD- 4.3BSD-Tahoe"
    );
    assert!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::Strong { .. }))
            .count()
            >= 2,
        "control-only Bx operands must retain font execution: {children:?}"
    );
}

#[test]
fn mail_and_link_labels_share_the_zero_advance_stream() {
    let cases = [
        (
            "mail-address",
            b".Dd September 12, 2026\n.Dt ZERO-LINK 1\n.Os\n.Sh DESCRIPTION\n.No a Mt b@example.org\\zX Ns c\n".as_slice(),
            "a b@example.orgc",
        ),
        (
            "link-label",
            b".Dd September 12, 2026\n.Dt ZERO-LINK 1\n.Os\n.Sh DESCRIPTION\n.No a Lk https://example.org b\\zX Ns c\n".as_slice(),
            "a b: https://example.orgc",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-{label}.1")),
            source,
        )
        .expect("parse link formatter-state fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph");
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert!(
            children
                .iter()
                .any(|inline| matches!(inline, Inline::Link { .. })),
            "{label}: preserving formatter state must not discard link identity"
        );
        let target = children.iter().find_map(|inline| match inline {
            Inline::Link { target, .. } => Some(target),
            _ => None,
        });
        match label {
            "mail-address" => assert!(
                matches!(target, Some(mant_ir::LinkTarget::Email { address }) if address == "b@example.org"),
                "{label}: wrong email target: {target:?}"
            ),
            "link-label" => assert!(
                matches!(target, Some(mant_ir::LinkTarget::External { uri }) if uri == "https://example.org"),
                "{label}: wrong external target: {target:?}"
            ),
            _ => unreachable!("unrecognized zero-advance case"),
        }
    }
}

#[test]
fn semantic_links_execute_uri_words_and_distinguish_empty_descriptions() {
    let cases = [
        (
            "link-zero-width-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org A\\zX\n.No B\n".as_slice(),
            "A: https://example.org B",
        ),
        (
            "empty-link-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \"\"\n.No B\n".as_slice(),
            ": https://example.org B",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-{label}.1")),
            source,
        )
        .expect("parse semantic link formatter-state fixture");
        let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
            panic!(
                "{label}: expected one description paragraph: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        // CVS termp_lk_pre() uses operand presence for the colon, while
        // mdoc_lk_pre() anchors the description, even if it has no glyphs.
        let has_visible_anchor = children.iter().any(|inline| {
            matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. } if uri == "https://example.org")
        });
        assert_eq!(has_visible_anchor, label == "link-zero-width-label");
    }

    let link_controls = parse_manual_bytes(
        std::path::Path::new("semantic-link-hidden-uri-font.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk \\fBhttps://example.org label\n.Li \\fPZ\n",
    )
    .expect("parse hidden link URI controls");
    let [Block::Paragraph { children, .. }] = link_controls.sections[1].blocks.as_slice() else {
        panic!(
            "expected one description paragraph: {:#?}",
            link_controls.sections
        );
    };
    assert!(
        matches!(children.last(), Some(Inline::Strong { children }) if inline_text(children) == "Z"),
        "controls hidden with the URI must still affect following siblings: {children:?}"
    );

    let mail_controls = parse_manual_bytes(
        std::path::Path::new("semantic-mail-hidden-font.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Mt \\fB a@example.org\n.Li \\fPZ\n",
    )
    .expect("parse hidden mail operand controls");
    let [Block::Paragraph { children, .. }] = mail_controls.sections[1].blocks.as_slice() else {
        panic!(
            "expected one description paragraph: {:#?}",
            mail_controls.sections
        );
    };
    assert!(
        children.iter().any(|inline| {
            matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Email { address }, children, .. }
                if address == "a@example.org"
                    && matches!(children.as_slice(), [Inline::Strong { children }] if inline_text(children) == "a@example.org"))
        }),
        "a control-only Mt operand must set the address font: {children:?}"
    );
}

#[test]
fn semantic_links_choose_visible_output_after_executing_operands() {
    let cases = [
        (
            "hidden-uri-zero-width",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org/\\zXY label\n.No AFTER\n".as_slice(),
            "label: https://example.org/Y AFTER",
        ),
        (
            "mail-zero-width",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Mt \\zX a@example.org\n.No AFTER\n".as_slice(),
            "Xa@example.org AFTER",
        ),
        (
            "projected-empty-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\zX\n.No AFTER\n".as_slice(),
            ": https://example.org AFTER",
        ),
        (
            "empty-target-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk \\fB label\n.Li \\fPZ\n".as_slice(),
            "label:  Z",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-execution-{label}.1")),
            source,
        )
        .expect("parse semantic link execution fixture");
        let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
            panic!(
                "{label}: expected one description paragraph: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert!(
            !children.iter().any(|inline| {
                matches!(inline, Inline::Link { children, .. } if children.is_empty())
            }),
            "{label}: visible source must not leave an empty link: {children:?}"
        );
        match label {
            "mail-zero-width" => assert!(
                children.iter().any(|inline| {
                    matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Email { address }, .. }
                        if address == "a@example.org")
                }),
                "{label}: the recovered address must retain its email target: {children:?}"
            ),
            "hidden-uri-zero-width" => assert!(
                children.iter().any(|inline| {
                    matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. }
                        if uri == "https://example.org/Y" || uri == "https://example.org")
                }),
                "{label}: the visible link must retain its external target: {children:?}"
            ),
            "projected-empty-label" => assert!(
                !children.iter().any(|inline| matches!(inline, Inline::Link { .. })),
                "a pending glyph overprinted by the colon cannot create a visible anchor: {children:?}"
            ),
            "empty-target-label" => {
                assert!(
                    !children.iter().any(|inline| matches!(inline, Inline::Link { .. })),
                    "{label}: an empty target must degrade to ordinary text: {children:?}"
                );
                assert!(
                    matches!(children.last(), Some(Inline::Strong { children }) if inline_text(children) == "Z"),
                    "{label}: hidden target controls must still affect later siblings: {children:?}"
                );
            }
            _ => unreachable!("unrecognized semantic link case"),
        }
    }
}

#[test]
fn control_only_link_labels_keep_their_structural_font_scope() {
    let document = parse_manual_bytes(
        std::path::Path::new("semantic-link-control-only-label.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\fB\n.Li \\fPZ\n",
    )
    .expect("parse control-only Lk label");
    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!(
            "expected one description paragraph: {:#?}",
            document.sections
        );
    };
    assert_eq!(
        inline_text(children),
        ": https://example.org Z",
        "{children:?}"
    );
    assert!(
        matches!(children.last(), Some(Inline::Text { value }) if value == "Z"),
        "the Lk scope must not turn the URI or following Z bold: {children:?}"
    );
}

#[test]
fn semantic_links_execute_hidden_word_boundaries_and_native_delimiters() {
    let cases = [
        (
            "label-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org label\\c\n.No AFTER\n".as_slice(),
            "label: https://example.org AFTER",
        ),
        (
            "uri-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org\\c label\n.No AFTER\n".as_slice(),
            "label: https://example.orgAFTER",
        ),
        (
            "escaped-punctuation-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\fB.\n.Li \\fPZ\n".as_slice(),
            ".: https://example.org Z",
        ),
        (
            "bare-closing-punctuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org .\n.Li Z\n".as_slice(),
            "https://example.org. Z",
        ),
        (
            "zero-width-punctuation-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\&.\n.Li Z\n".as_slice(),
            ".: https://example.org Z",
        ),
        (
            "bare-hidden-zero-advance",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org/\\z label\n.No AFTER\n".as_slice(),
            "label: https://example.org/ FTER",
        ),
        (
            "whitespace-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \" \"\n.No AFTER\n".as_slice(),
            " : https://example.org AFTER",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-boundary-{label}.1")),
            source,
        )
        .expect("parse semantic link boundary fixture");
        let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
            panic!(
                "{label}: expected one description paragraph: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert!(
            !children.iter().any(|inline| {
                matches!(inline, Inline::Link { children, .. } if children.is_empty())
            }),
            "{label}: semantic link output must not be empty: {children:?}"
        );
        if label == "escaped-punctuation-label" {
            assert!(
                matches!(children.last(), Some(Inline::Text { value }) if value == "Z"),
                "{label}: an authored label font scope must not leak: {children:?}"
            );
        }
    }
}

#[test]
fn semantic_link_continuations_preserve_literal_rows() {
    for (label, source, expected) in [
        (
            "label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bd -literal\n.Lk https://example.org label\\c\n.No AFTER\n.Ed\n".as_slice(),
            "label: https://example.org\nAFTER",
        ),
        (
            "uri",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bd -literal\n.Lk https://example.org\\c label\n.No AFTER\n.Ed\n".as_slice(),
            "label: https://example.orgAFTER",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-literal-continuation-{label}.1")),
            source,
        )
        .expect("parse literal semantic link continuation");
        let [Block::Preformatted { children, .. }] = document.sections[1].blocks.as_slice() else {
            panic!("{label}: expected one literal display: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn semantic_link_compaction_preserves_layout_and_final_execution_boundaries() {
    for (label, source, expected) in [
        (
            "empty-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \"\"\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\n: https://example.org\nAFTER",
        ),
        (
            "control-only-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \\fB\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\n: https://example.org\nAFTER",
        ),
        (
            "zero-width-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \\zX\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\n: https://example.org\nAFTER",
        ),
        (
            "hidden-target-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org\\p label\n.No AFTER\n".as_slice(),
            "label: https://example.org\nAFTER",
        ),
        (
            "trailing-punctuation-consumes-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org\\c label .\n.No AFTER\n".as_slice(),
            "label: https://example.org. AFTER",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-layout-{label}.1")),
            source,
        )
        .expect("parse semantic link layout fixture");
        let blocks = &document.sections[0].blocks;
        let [block] = blocks.as_slice() else {
            panic!("{label}: expected one flow block: {blocks:#?}");
        };
        let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = block else {
            panic!("{label}: expected one flow block: {blocks:#?}");
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn empty_operands_are_words_before_generated_semantic_punctuation() {
    for (label, source, expected) in [
        (
            "optional-argument",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Op A\\zX \"\"\n.No AFTER\n".as_slice(),
            "[AX] AFTER",
        ),
        (
            "link-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org \\zX \"\"\n.No AFTER\n".as_slice(),
            "X: https://example.org AFTER",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-empty-word-{label}.1")),
            source,
        )
        .expect("parse semantic empty word fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        if label == "link-label" {
            assert!(
                matches!(children.first(), Some(Inline::Link { children, .. }) if inline_text(children) == "X"),
                "{label}: the resolved label must remain a visible link: {children:?}"
            );
        }
    }
}

#[test]
fn generated_bsd_word_retains_break_markers_across_continuation_and_portable_display() {
    // Exact inputs verified with pristine CVS before adding the assertions.
    // term_word() buffers ESCAPE_BREAK even when ESCAPE_NOSPACE follows it;
    // post_bx() inserts Ns then the generated BSD word. The next ordinary
    // word blank realizes that buffered marker (term.c:294-305, 656-667).
    for (operand, native, portable) in [
        (r"\p\c", "BSD", "BSD"),
        (r"\p", "BSD", "BSD"),
        (r"\p\c\zX", "BSD", "BSD"),
        (r"\p\&", "BSD", "BSD"),
        (r"-alpha\p\c", "-alphaBSD", "BSD (currently in alpha test)"),
        (r"-beta\p\c", "-betaBSD", "BSD (currently in beta test)"),
        (
            r"-devel\p\c",
            "-develBSD",
            "BSD (currently under development)",
        ),
    ] {
        for literal in [false, true] {
            let source = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{}.Bx {operand}\n.No AFTER LAST\n{}",
                if literal { ".Bd -literal\n" } else { "" },
                if literal { ".Ed\n" } else { "" },
            );
            let document = parse_manual_bytes(
                std::path::Path::new("bsd-generated-marker.1"),
                source.as_bytes(),
            )
            .expect("parse generated BSD marker");
            let [block] = document.sections[0].blocks.as_slice() else {
                panic!("{operand} literal={literal}: {document:#?}");
            };
            let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = block
            else {
                panic!("{operand} literal={literal}: {block:#?}");
            };
            assert_eq!(
                inline_text(children),
                format!("{native}\nAFTER LAST"),
                "{operand} literal={literal}: {children:?}"
            );
            let markdown = crate::encode::render_inline_fragment(
                children,
                crate::encode::MarkdownFragmentOptions::default(),
            );
            assert!(markdown.contains(portable), "{operand}: {markdown}");
            assert_eq!(markdown.matches('\n').count(), 1, "{operand}: {markdown}");
        }
    }
}

#[test]
fn mail_identity_decoding_cannot_erase_native_control_only_operands() {
    // Exact inputs verified with pristine CVS before adding the assertions.
    // Mt uses termp_under_pre() and visits each authored child. encode1()
    // writes X before BACKBEFORE consumes the following word separator
    // (term.c:901-908); pure href encoding must not erase the native glyph.
    // G-IND: pristine starts the automatic separator cell one column past
    // the section's five-cell origin for these empty leading operands. Filled
    // reading keeps its word/hard-row effects but omits device-only first-row
    // padding; these exact IR strings deliberately contain no leading blank.
    for (operand, expected) in [
        (r"\zX", "Xa@example.org AFTER"),
        (r"\z", "@example.org AFTER"),
        (r"\z\c", "a@example.org AFTER"),
        (r"\fB", "a@example.org AFTER"),
        (r"\&", "a@example.org AFTER"),
        (r#""""#, "a@example.org AFTER"),
        (r"\zX\p", "Xa@example.org\nAFTER"),
        (r"\zX\c", "a@example.org AFTER"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt {operand} a@example.org\n.No AFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("mail-native-controls.1"),
            source.as_bytes(),
        )
        .expect("parse mail native controls");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{operand}: {document:#?}");
        };
        assert_eq!(inline_text(children), expected, "{operand}: {children:?}");
        let address = children
            .iter()
            .find_map(|inline| match inline {
                Inline::Link {
                    target: mant_ir::LinkTarget::Email { address },
                    children,
                    ..
                } => Some((address, inline_text(children))),
                _ => None,
            })
            .expect("the address operand retains its email identity");
        assert_eq!(address.0, "a@example.org", "{operand}");
        assert_eq!(
            address.1,
            if operand == r"\z" {
                "@example.org"
            } else {
                "a@example.org"
            },
            "{operand}"
        );
    }
}

#[test]
fn recovered_inline_man_links_share_the_document_head_identity_decoder() {
    fn block<'a>(node: &'a libmandoc_rs::Node, name: &str) -> Option<&'a libmandoc_rs::Node> {
        if node.kind == libmandoc_rs::NodeKind::Block && node.macro_name.as_deref() == Some(name) {
            return Some(node);
        }
        node.children.iter().find_map(|node| block(node, name))
    }
    fn identities(nodes: &[Inline], output: &mut Vec<String>) {
        for node in nodes {
            match node {
                Inline::Link {
                    target, children, ..
                } => {
                    match target {
                        mant_ir::LinkTarget::External { uri } => output.push(uri.clone()),
                        mant_ir::LinkTarget::Email { address } => output.push(address.clone()),
                        _ => {}
                    }
                    identities(children, output);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::PortableDisplay { children, .. } => identities(children, output),
                _ => {}
            }
        }
    }
    // These exact ordinary man sources ran pristine first. The recovery
    // entry accepts the same owned UR/MT shape inside its closed language;
    // its href follows man_html.c::man_UR_pre/print_encode(norecurse=1),
    // independently of the later terminal post_UR target-word execution.
    for (start, end, prefix) in [
        ("UR", "UE", "https://example.org/"),
        ("MT", "ME", "user@example.org"),
    ] {
        for (spelling, suffix) in [
            ("", ""),
            (r"\zX", ""),
            (r"\z\fBX\fP", ""),
            (r"\&X", "X"),
            (r"\*[.T]", "html"),
            (r"\o'BC'", "C"),
            (r"\o'BC '", " "),
            (r"\z\&X", "X"),
        ] {
            let source = format!(
                ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.{start} \"{prefix}{spelling}\"\nLINKLABEL\n.{end}\nafter\n"
            );
            let parsed = libmandoc_rs::Parser::default()
                .parse_bytes("man-identity.1", source.as_bytes())
                .unwrap();
            let node = block(&parsed.document.root, start).unwrap();
            let mut builder = crate::mandoc::inline::InlineBuilder::new();
            crate::mandoc::inline::append_man_link(&mut builder, node, None, false);
            let output = builder.finish();
            let mut targets = Vec::new();
            identities(&output, &mut targets);
            assert_eq!(
                targets,
                [format!("{prefix}{suffix}")],
                "{source}\n{output:#?}"
            );
        }
    }
}

#[test]
fn recovered_inline_man_link_annotation_cannot_skip_native_post_words() {
    fn block<'a>(node: &'a libmandoc_rs::Node, name: &str) -> Option<&'a libmandoc_rs::Node> {
        if node.kind == libmandoc_rs::NodeKind::Block && node.macro_name.as_deref() == Some(name) {
            return Some(node);
        }
        node.children.iter().find_map(|node| block(node, name))
    }
    fn labels(nodes: &[Inline], output: &mut Vec<String>) {
        for node in nodes {
            match node {
                Inline::Link { children, .. } => {
                    output.push(inline_text(children));
                    labels(children, output);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::PortableDisplay { children, .. } => labels(children, output),
                _ => {}
            }
        }
    }
    // All 64 exact ordinary UR/MT sources ran pristine before this test.
    // post_UR always emits <, the original HEAD and >, while man_UR_pre
    // chooses a HEAD label only for a BODY without any syntax children.
    // A trailing executed row boundary stays outside the clickable label;
    // its exact row still appears in the full native-output assertion.
    // An empty href cannot skip p/c/z consumption or erase safe native text.
    // Unknown specials follow ManT's documented recovery spelling; their
    // HTML identity is still empty, just like the pristine href.
    // G-IND: only the device-only leading padding for a first \\& BODY word
    // is omitted; the exact strings keep interior spaces and hard rows.
    for (start, end) in [("UR", "UE"), ("MT", "ME")] {
        for (operand, target_word, has_target) in [
            (r"\zX", "", false),
            (r"\&", "", false),
            (r"\[nosuch]", r"\[nosuch]", false),
            ("https://example.org", "https://example.org", true),
        ] {
            let suffix = format!("<{target_word}>");
            for (body, expected, expected_label) in [
                ("", format!("{suffix} after"), target_word.to_owned()),
                (
                    "LINKLABEL\n",
                    format!("LINKLABEL {suffix} after"),
                    "LINKLABEL".to_owned(),
                ),
                (
                    "LINKLABEL\\p\n",
                    format!("LINKLABEL\n{suffix} after"),
                    "LINKLABEL".to_owned(),
                ),
                (
                    "LINKLABEL\\p\\c\n",
                    format!("LINKLABEL{suffix}\nafter"),
                    "LINKLABEL".to_owned(),
                ),
                (
                    "LINKLABEL\\z\n",
                    format!("LINKLABEL {target_word}> after"),
                    "LINKLABEL".to_owned(),
                ),
                ("\\zX\n", format!("X{suffix} after"), "X".to_owned()),
                ("\\&\n", format!("{suffix} after"), String::new()),
                (".ft B\n", format!("{suffix} after"), String::new()),
            ] {
                let source = format!(
                    ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.{start} \"{operand}\"\n{body}.{end}\nafter\n"
                );
                let parsed = libmandoc_rs::Parser::default()
                    .parse_bytes("man-post-identity.1", source.as_bytes())
                    .unwrap();
                let node = block(&parsed.document.root, start).unwrap();
                let mut builder = crate::mandoc::inline::InlineBuilder::new();
                crate::mandoc::inline::append_man_link(&mut builder, node, None, false);
                builder.append_text("after");
                let output = builder.finish();
                assert_eq!(inline_text(&output), expected, "{source}\n{output:#?}");
                let mut actual_labels = Vec::new();
                labels(&output, &mut actual_labels);
                let expected_labels = if has_target {
                    vec![expected_label]
                } else {
                    Vec::new()
                };
                assert_eq!(actual_labels, expected_labels, "{source}\n{output:#?}");
            }
        }
    }
}

#[test]
fn descriptive_link_labels_do_not_own_preceding_native_row_boundaries() {
    fn labels(nodes: &[Inline], output: &mut Vec<String>) {
        for node in nodes {
            match node {
                Inline::Link { children, .. } => output.push(inline_text(children)),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::PortableDisplay { children, .. } => labels(children, output),
                _ => {}
            }
        }
    }
    // Every exact source ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // termp_lk_pre visits description words after the caller's term_word
    // marker has closed its row. mdoc_lk_pre's anchor contains only the
    // description; private word anchors cannot make that previous row part
    // of the typed label. A cached z glyph remains owned by its prior word.
    for (prefix, head) in [
        (r"X\p", "X\n"),
        (r"X\p\p", "X\n"),
        (r"X\p\c", "X"),
        (r"X\zZ", "XZ"),
    ] {
        for (operand, label, styled) in [
            ("Y", "Y", "*Y*"),
            (r"\fBY", "Y", "**Y**"),
            (r"\fIY", "Y", "*Y*"),
            ("é名", "é名", "*é名*"),
            (r"\fBé名", "é名", "**é名**"),
            (r"\fIé名", "é名", "*é名*"),
        ] {
            if label != "Y" && !matches!(prefix, r"X\p" | r"X\zZ") {
                continue;
            }
            let source = format!(
                ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.No {prefix}\n.ta 2n\n.Lk https://example.org {operand}\n.No AFTER\n.Sh NEXT\n.No END\n"
            );
            let document =
                parse_manual_bytes(std::path::Path::new("lk-boundary.1"), source.as_bytes())
                    .unwrap();
            let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
                panic!("expected a paragraph: {source}\n{document:#?}");
            };
            let native = if prefix == r"X\p\c" {
                format!("{head}{label}:\nhttps://example.org AFTER")
            } else {
                format!("{head}{label}: https://example.org AFTER")
            };
            assert_eq!(inline_text(children), native, "{source}");
            let mut actual = Vec::new();
            labels(children, &mut actual);
            assert_eq!(actual, [label], "{source}");
            let markdown = crate::encode::render_inline_fragment(
                children,
                crate::encode::MarkdownFragmentOptions::default(),
            );
            assert!(
                markdown.contains(&format!("[{styled}](https://example.org)")),
                "{source}\n{markdown}"
            );
        }
    }
}

// Font assertions inspect the native children, rather than freezing a private
// wrapper shape used by portable Markdown export.
fn strong_native_text(children: &[Inline]) -> String {
    children
        .iter()
        .map(|inline| match inline {
            Inline::Strong { children } => inline_text(children),
            Inline::Emphasis { children }
            | Inline::PortableDisplay { children, .. }
            | Inline::Link { children, .. } => strong_native_text(children),
            _ => String::new(),
        })
        .collect()
}
