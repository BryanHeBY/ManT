use super::*;

#[test]
fn zero_advance_crosses_alternating_man_macro_arguments() {
    let document = parse_manual_bytes(
        std::path::Path::new("zero-advance-man-font-scope.1"),
        b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\n.BR A\\zX B\n",
    )
    .expect("parse alternating man font scope");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph");
    };

    // `\\zX` writes X without advancing. CVS term.c later writes B at that
    // same position even though the operands are separate `term_word()`
    // calls, so the semantic projection must be AB rather than AXB.
    assert_eq!(inline_text(children), "AB");
}

#[test]
fn zero_advance_projects_implicit_words_and_generated_op_brackets_in_output_order() {
    let document = parse_manual_bytes(
        std::path::Path::new("zero-advance-output-order.1"),
        b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\nA\\zX\nB\n.OP A\\zX B\n.OP A\\zX\n",
    )
    .expect("parse zero-advance formatter boundaries");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections[0].blocks);
    };

    // CVS term.c inserts the filled-word blank before the next glyph, which
    // preserves X without making the blank visible. man_term.c emits `.OP`
    // brackets through term_word(), so B and ] respectively overprint or
    // preserve the pending glyph according to their actual output order.
    assert_eq!(inline_text(children), "AXB [AXB] [A]");
}

#[test]
fn zero_advance_crosses_leading_scopes_generated_prefixes_and_link_labels() {
    let cases = [
        (
            "leading-word",
            b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\n\\zX\nB\n".as_slice(),
            "XB",
        ),
        (
            "man-font-scope",
            b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\nA\\zX\n.B B\n".as_slice(),
            "AXB",
        ),
        (
            "optional-arguments",
            b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\nA\\zX\n.OP B C\n".as_slice(),
            "AX[B C]",
        ),
        (
            "mdoc-prefix",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX Fl b\n".as_slice(),
            "AX-b",
        ),
        (
            "link-label",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX Lk https://example.org B\n".as_slice(),
            "AXB",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-{label}.1")),
            source,
        )
        .expect("parse cross-scope zero-advance fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.sections[0].blocks
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        if label == "link-label" {
            assert!(
                matches!(children.as_slice(), [Inline::Text { value: prefix }, Inline::Text { value: glyph }, Inline::Link { .. }]
                    if prefix == "A" && glyph == "X"),
                "the pending glyph must precede the atomically lowered link: {children:?}"
            );
        }
    }
}

#[test]
fn quote_enclosure_angle_marks_follow_the_sole_mt_child_topology() {
    // mdoc_term.c::termp_quote_pre/post (1600-1603, 1658-1661) print the
    // ASCII pair only when the enclosure's child list is exactly one `.Mt`
    // element (`n->child != NULL && n->child->next == NULL &&
    // n->child->tok == MDOC_Mt`); every other child shape takes the \(la
    // and \(ra catalog glyphs. The decision rides the parsed child
    // topology, never a "@" substring test: `.Mt` swallows the remaining
    // arguments on its line, so extra words stay inside the single `.Mt`
    // child. Expected marks verified with the pristine reference binary
    // (-Tutf8 -Owidth=78).
    let cases = [
        ("plain-word", ".Aq Word", "⟨Word⟩"),
        ("word-with-at", ".Aq a@b", "⟨a@b⟩"),
        ("single-mt", ".Aq Mt test@example.com", "<test@example.com>"),
        ("mt-without-at", ".Aq Mt ab", "<ab>"),
        ("multiple-mt", ".Aq Mt a@b Mt c@d", "⟨a@b c@d⟩"),
        ("mt-then-no", ".Aq Mt a@b No plain", "⟨a@b plain⟩"),
        ("explicit-ao-sole-mt", ".Ao Mt a@b\n.Ac", "<a@b>"),
        ("explicit-ao-mt-then-no", ".Ao Mt a@b No x\n.Ac", "⟨a@b x⟩"),
    ];
    for (label, body, expected) in cases {
        let source = format!(".Dd September 28, 2026\n.Dt QUOTE 1\n.Os\n.Sh DESCRIPTION\n{body}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("quote-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse quote enclosure fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.sections[0].blocks
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}
