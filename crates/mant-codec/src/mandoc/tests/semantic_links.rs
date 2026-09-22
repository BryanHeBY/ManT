use super::*;

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
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
    }
}

#[test]
fn bsd_reference_executes_suppressed_font_operands_before_generated_text() {
    let document = parse_manual_bytes(
        std::path::Path::new("bsd-reference-font-state.1"),
        b".Dd September 12, 2026\n.Dt BSD-FONT 1\n.Os\n.Sh DESCRIPTION\n.Bx \\fB\n.Li \\fPZ\n.Bx \\fB-devel\n.Li \\fPZ\n",
    )
    .expect("parse Bx formatter-state fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections[0].blocks);
    };

    // CVS mdoc_validate.c appends generated BSD after source argument
    // processing. A control-only operand remains executable even though the
    // semantic lifecycle form replaces its visible spelling.
    assert_eq!(
        children.len(),
        7,
        "suppressed Bx operands must retain their font transitions"
    );
    for (index, expected) in [
        (0, "BSD"),
        (2, "Z"),
        (4, "BSD (currently under development)"),
        (6, "Z"),
    ] {
        assert!(
            matches!(&children[index], Inline::Strong { children } if inline_text(document.content(), children) == expected)
        );
    }
    for index in [1, 3, 5] {
        assert_eq!(leaf_text(&document, &children[index]), Some(" "));
    }
}

#[test]
fn bsd_reference_replacements_execute_complete_hidden_word_state() {
    for (label, operand, expected) in [
        (
            "lifecycle-word-end-break",
            r"-alpha\p",
            "BSD (currently in alpha test)\nAFTER",
        ),
        ("control-only-word-end-break", r"\p", "BSD\nAFTER"),
        (
            "lifecycle-source-continuation",
            r"-beta\c",
            "BSD (currently in beta test) AFTER",
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
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}"
        );
    }

    let document = parse_manual_bytes(
        std::path::Path::new("bsd-reference-hidden-font-break.1"),
        b".Dd September 12, 2026\n.Dt BSD-STATE 1\n.Os\n.Sh DESCRIPTION\n.Bx \\fB-devel\\p\n.Li \\fPZ\n",
    )
    .expect("parse styled Bx hidden execution fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections);
    };
    assert_eq!(
        inline_text(document.content(), children),
        "BSD (currently under development)\nZ"
    );
    assert!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::Strong { .. }))
            .count()
            >= 2,
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
        inline_text(document.content(), children),
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
            "a bc",
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
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert!(
            children
                .iter()
                .any(|inline| matches!(inline, Inline::Link { .. })),
            "{label}: preserving formatter state must not discard link identity"
        );
        let target = children
            .iter()
            .find_map(|inline| link_target(&document, inline));
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
fn semantic_links_execute_hidden_operands_and_preserve_empty_label_fallbacks() {
    let cases = [
        (
            "link-zero-width-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org A\\zX\n.No B\n".as_slice(),
            "A B",
        ),
        (
            "empty-link-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \"\"\n.No B\n".as_slice(),
            "https://example.org B",
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
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert!(
            children.iter().any(|inline| {
                matches!(link_target(&document, inline), Some(mant_ir::LinkTarget::External { uri }) if uri == "https://example.org")
            }),
            "{label}: visible text must retain the typed external target"
        );
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
        matches!(children.last(), Some(Inline::Strong { children }) if inline_text(link_controls.content(), children) == "Z"),
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
            matches!(inline, Inline::Link { children, .. }
                if matches!(link_target(&mail_controls, inline), Some(mant_ir::LinkTarget::Email { address }) if address == "a@example.org")
                    && matches!(children.as_slice(), [Inline::Strong { children }] if inline_text(mail_controls.content(), children) == "a@example.org"))
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
            "label AFTER",
        ),
        (
            "mail-zero-width",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Mt \\zX a@example.org\n.No AFTER\n".as_slice(),
            "a@example.org AFTER",
        ),
        (
            "projected-empty-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\zX\n.No AFTER\n".as_slice(),
            "https://example.org AFTER",
        ),
        (
            "empty-target-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk \\fB label\n.Li \\fPZ\n".as_slice(),
            "label Z",
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
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert!(
            !children.iter().any(|inline| {
                matches!(inline, Inline::Link { children, .. } if children.is_empty())
            }),
            "{label}: visible source must not leave an empty link: {children:?}"
        );
        match label {
            "mail-zero-width" => assert!(
                children.iter().any(|inline| {
                    matches!(link_target(&document, inline), Some(mant_ir::LinkTarget::Email { address })
                        if address == "a@example.org")
                }),
                "{label}: the recovered address must retain its email target: {children:?}"
            ),
            "hidden-uri-zero-width" | "projected-empty-label" => assert!(
                children.iter().any(|inline| {
                    matches!(link_target(&document, inline), Some(mant_ir::LinkTarget::External { uri })
                        if uri == "https://example.org/Y" || uri == "https://example.org")
                }),
                "{label}: the visible link must retain its external target: {children:?}"
            ),
            "empty-target-label" => {
                assert!(
                    !children.iter().any(|inline| matches!(inline, Inline::Link { .. })),
                    "{label}: an empty target must degrade to ordinary text: {children:?}"
                );
                assert!(
                    matches!(children.last(), Some(Inline::Strong { children }) if inline_text(document.content(), children) == "Z"),
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
        inline_text(document.content(), children),
        "https://example.org Z",
        "{children:?}"
    );
    assert!(
        children
            .last()
            .and_then(|inline| leaf_text(&document, inline))
            == Some("Z"),
        "the Lk scope must not turn the URI or following Z bold: {children:?}"
    );
}

#[test]
fn semantic_links_execute_hidden_word_boundaries_and_native_delimiters() {
    let cases = [
        (
            "label-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org label\\c\n.No AFTER\n".as_slice(),
            "label AFTER",
        ),
        (
            "uri-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org\\c label\n.No AFTER\n".as_slice(),
            "labelAFTER",
        ),
        (
            "escaped-punctuation-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\fB.\n.Li \\fPZ\n".as_slice(),
            ". Z",
        ),
        (
            "bare-closing-punctuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org .\n.Li Z\n".as_slice(),
            "https://example.org. Z",
        ),
        (
            "zero-width-punctuation-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\&.\n.Li Z\n".as_slice(),
            ". Z",
        ),
        (
            "bare-hidden-zero-advance",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org/\\z label\n.No AFTER\n".as_slice(),
            "label FTER",
        ),
        (
            "whitespace-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \" \"\n.No AFTER\n".as_slice(),
            "https://example.org AFTER",
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
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert!(
            !children.iter().any(|inline| {
                matches!(inline, Inline::Link { children, .. } if children.is_empty())
            }),
            "{label}: semantic link output must not be empty: {children:?}"
        );
        if label == "escaped-punctuation-label" {
            assert!(
                children
                    .last()
                    .and_then(|inline| leaf_text(&document, inline))
                    == Some("Z"),
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
            "label\nAFTER",
        ),
        (
            "uri",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bd -literal\n.Lk https://example.org\\c label\n.No AFTER\n.Ed\n".as_slice(),
            "labelAFTER",
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
        assert_eq!(inline_text(document.content(), children), expected, "{label}: {children:?}");
    }
}

#[test]
fn semantic_link_compaction_preserves_layout_and_final_execution_boundaries() {
    for (label, source, expected) in [
        (
            "empty-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \"\"\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\nhttps://example.org\nAFTER",
        ),
        (
            "control-only-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \\fB\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\nhttps://example.org\nAFTER",
        ),
        (
            "zero-width-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \\zX\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\nhttps://example.org\nAFTER",
        ),
        (
            "hidden-target-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org\\p label\n.No AFTER\n".as_slice(),
            "label\nAFTER",
        ),
        (
            "trailing-punctuation-consumes-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org\\c label .\n.No AFTER\n".as_slice(),
            "label. AFTER",
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
        assert_eq!(inline_text(document.content(), children), expected, "{label}: {children:?}");
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
            "X AFTER",
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
        assert_eq!(inline_text(document.content(), children), expected, "{label}: {children:?}");
        if label == "link-label" {
            assert!(
                matches!(children.first(), Some(Inline::Link { children, .. }) if inline_text(document.content(), children) == "X"),
                "{label}: the resolved label must remain a visible link: {children:?}"
            );
        }
    }
}
