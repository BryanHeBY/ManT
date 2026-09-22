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
    assert_eq!(inline_text(document.content(), children), "AB");
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
    assert_eq!(inline_text(document.content(), children), "AXB [AXB] [A]");
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
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        if label == "link-label" {
            assert!(
                matches!(children.as_slice(), [Inline::Text { content: prefix }, Inline::Text { content: glyph }, Inline::Link { .. }]
                    if document.content().resolve_text(*prefix) == Some("A")
                        && document.content().resolve_text(*glyph) == Some("X")),
                "the pending glyph must precede the atomically lowered link: {children:?}"
            );
        }
    }
}
