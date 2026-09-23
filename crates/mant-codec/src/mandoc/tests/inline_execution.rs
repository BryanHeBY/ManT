use super::*;

#[test]
fn inline_execution_keeps_word_joins_glyph_ownership_and_literal_breaks_distinct() {
    // CVS `term_word()` carries `TERMP_BACKAFTER`, `TERMP_NOSPACE`, and an
    // emitted `\\p` break as independent state.  These cases deliberately
    // cross semantic wrappers because they are where a flattened AST tail is
    // most tempting (and wrong) to use as a replacement for execution order.
    for (label, source, expected) in [
        (
            "enclosure-closes-a-source-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\\c\n.Ec\n.No AFTER\n".as_slice(),
            "[A AFTER",
        ),
        (
            "include-closes-a-source-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.In stdio.h\\c\n.No AFTER\n".as_slice(),
            "<stdio.h> AFTER",
        ),
        (
            "no-space-cancels-a-bare-zero-advance",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\z\\c\n.No AFTER\n".as_slice(),
            "BEFORE AFTER",
        ),
        (
            "hidden-empty-mail-operand-cannot-own-a-prior-glyph",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt a@example.org\\zX \"\"\n.No AFTER\n".as_slice(),
            "a@example.orgX AFTER",
        ),
        (
            "literal-explicit-break-closes-the-source-row-once",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\\p\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\nAFTER",
        ),
        (
            "hidden-uri-explicit-break-closes-the-source-row-once",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Lk https://example.org\\p label\n.No AFTER\n.Ed\n".as_slice(),
            "label\nAFTER",
        ),
        (
            "literal-enclosure-retains-formatter-and-authored-blanks",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Eo [\n.No BEFORE\\c\n.Ec\n AFTER\n.Ed\n".as_slice(),
            "[\nBEFORE  AFTER",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-execution-{label}.1")),
            source,
        )
        .expect("parse inline execution boundary fixture");
        let blocks = &document.flow().expect("Flow fixture").sections[0].blocks;
        let [block] = blocks.as_slice() else {
            panic!("{label}: expected one flow block: {blocks:#?}");
        };
        let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = block else {
            panic!("{label}: expected flow content: {blocks:#?}");
        };
assert_eq!(inline_text(document.content(), children), expected, "{label}: {children:?}");
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn inline_execution_tracks_zero_advance_word_and_physical_line_states_independently() {
    let cases = [
        (
            "completed-zero-advance-survives-no-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\zX\\c\n.No AFTER\n".as_slice(),
            "BEFOREAFTER",
        ),
        (
            "later-no-space-remains-effective",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\z\\c\\c\n.No AFTER\n".as_slice(),
            "BEFOREAFTER",
        ),
        (
            "armed-and-pending-zero-advance-stages-coexist",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\zX\\z\\c\n.No AFTER\n".as_slice(),
            "BEFOREXAFTER",
        ),
        (
            "word-end-break-waits-for-the-complete-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\pTAIL\n.No AFTER\n".as_slice(),
            "BEFORETAIL\nAFTER",
        ),
        (
            "word-end-break-crosses-tight-ns-boundary",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\pB Ns C\n.No D\n".as_slice(),
            "ABC\nD",
        ),
        (
            "word-end-break-realizes-inside-a-later-tight-text-node",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p Ns No \"B C\"\n.No D\n".as_slice(),
            "AB\nC D",
        ),
        (
            "word-end-break-crosses-a-generated-tight-prefix-before-an-inner-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p Ns Fl \"B C\"\n.No D\n".as_slice(),
            "A-B\nC D",
        ),
        (
            "disabled-spacing-defers-word-end-break-to-the-next-inner-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.No A\\p No \"B C\"\n.Sm on\n.No D\n".as_slice(),
            "AB\nC D",
        ),
        (
            "word-end-break-stops-at-intra-operand-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB C\"\n.No D\n".as_slice(),
            "AB\nC D",
        ),
        (
            "word-end-break-and-trailing-continuation-remain-orthogonal",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\p B\\c\"\n.No C\n".as_slice(),
            "A\nBC",
        ),
        (
            "intra-word-break-does-not-cancel-a-later-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB C\\c\"\n.No D\n".as_slice(),
            "AB\nCD",
        ),
        (
            "word-end-break-crosses-unpaddable-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB\\ C\"\n.No D\n".as_slice(),
            "AB C\nD",
        ),
        (
            "word-end-break-crosses-nonbreaking-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB\\~C\"\n.No D\n".as_slice(),
            "AB C\nD",
        ),
        (
            "word-end-break-crosses_fixed-width_space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB\\0C\"\n.No D\n".as_slice(),
            "AB C\nD",
        ),
        (
            "projected-glyph-ends-only-the-consumed-break-whitespace-run",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\(em C\"\n".as_slice(),
            "AB\n— C",
        ),
        (
            "nonbreaking-glyph-preserves-the-following-ordinary-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\~ C\"\n".as_slice(),
            "AB\n  C",
        ),
        (
            "fixed-width-glyph-preserves-the-following-ordinary-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\0 C\"\n".as_slice(),
            "AB\n  C",
        ),
        (
            "overstrike-glyph-preserves-the-following-ordinary-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\o'XY' C\"\n".as_slice(),
            "AB\nY C",
        ),
        (
            "device-glyph-preserves-the-following-ordinary-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\*[.T] C\"\n".as_slice(),
            "AB\nutf8 C",
        ),
        (
            "font-state-does-not-end-consumed-break-whitespace",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\fB C\"\n".as_slice(),
            "AB\nC",
        ),
        (
            "repeated-word-end-break-is-idempotent",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\\pB\n.No C\n".as_slice(),
            "AB\nC",
        ),
        (
            "word-end-break-and-zero-advance-remain-orthogonal",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\\zX Ns B\n.No C\n".as_slice(),
            "AB\nC",
        ),
        (
            "zero-advance-glyph-defers-a-trailing-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\\p\n.No D\n.No C\n".as_slice(),
            "AXD\nC",
        ),
        (
            "word-end-break-before-zero-advance-survives-the-settling-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\\zX\n.No B\n.No C\n".as_slice(),
            "AXB\nC",
        ),
        (
            "empty-word-settles-zero-advance-before-realizing-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\\p\n.No \"\"\n.No B\n.No C\n".as_slice(),
            "AX\nB C",
        ),
        (
            "empty-word-realizes-word-end-break-with-one-following-boundary",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\n.No \"\"\n.No B\n.No C\n".as_slice(),
            "A\n B C",
        ),
        (
            "zero-advance-word-blank-defers-the-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\zX\\p B\"\n.No D\n".as_slice(),
            "AXB\nD",
        ),
        (
            "zero-advance-overwrite-keeps-the-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\\pB\n.No D\n".as_slice(),
            "AB\nD",
        ),
        (
            "enclosure-releases-spacing-not-physical-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No BEFORE\\c\n.Ec\n AFTER\n".as_slice(),
            "[BEFORE  AFTER",
        ),
        (
            "styled-word-retains-formatter-and-authored-blanks",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No BEFORE\\c\n.Ec\n.No \" AFTER\"\n".as_slice(),
            "[BEFORE  AFTER",
        ),
        (
            "explicit-empty-es-close-is-a-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Es \"\" \"\"\n.En BEFORE\\c\n.No AFTER\n".as_slice(),
            "BEFORE AFTER",
        ),
        (
            "missing-es-close-releases-word-spacing",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Es \"\"\n.En BEFORE\\c\n.No AFTER\n".as_slice(),
            "BEFORE AFTER",
        ),
        (
            "en-without-es-releases-word-spacing",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.En BEFORE\\c\n.No AFTER\n".as_slice(),
            "BEFORE AFTER",
        ),
        (
            "generated-close-consumes-physical-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Op BEFORE\\c\n AFTER\n".as_slice(),
            "[BEFORE]\n AFTER",
        ),
        (
            "man-font-scope-carries-word-end-break",
            b".TH PROBE 1\n.SH DESCRIPTION\n.B A\\p\nB\n".as_slice(),
            "A\nB",
        ),
        (
            "alternating-man-font-scope-carries-word-end-break",
            b".TH PROBE 1\n.SH DESCRIPTION\n.BR A\\p B\nC\n".as_slice(),
            "AB\nC",
        ),
        (
            "alternating-man-font-scope-realizes-word-end-break-inside-an-operand",
            b".TH PROBE 1\n.SH DESCRIPTION\n.BR A\\p \"B C\"\nD\n".as_slice(),
            "AB\nC D",
        ),
        (
            "man-op-keeps-operands-before-word-end-break",
            b".TH PROBE 1\n.SH DESCRIPTION\n.OP A\\p B\nC\n".as_slice(),
            "[A B]\nC",
        ),
        (
            "man-op-no-space-suppresses-the-kept-operand-boundary",
            b".TH PROBE 1\n.SH DESCRIPTION\n.OP \"A\\c\" C\nD\n".as_slice(),
            "[AC] D",
        ),
        (
            "man-op-no-space-after-word-end-break-keeps-the-second-operand",
            b".TH PROBE 1\n.SH DESCRIPTION\n.OP \"A\\p B\\c\" C\nD\n".as_slice(),
            "[A\nBC] D",
        ),
        (
            "man-op-empty-kept-operand-consumes-no-space-without-padding",
            b".TH PROBE 1\n.SH DESCRIPTION\n.OP \"A\\p B\\c\" \"\"\nD\n".as_slice(),
            "[A\nB] D",
        ),
        (
            "include-scope-carries-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.In stdio.h\\p\n.No AFTER\n".as_slice(),
            "<stdio.h>\nAFTER",
        ),
        (
            "mdoc-keep-group-defers-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p No B\n.Ek\n.No C\n".as_slice(),
            "A B\nC",
        ),
        (
            "mdoc-keep-group-respects-source-line-end",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p\n.No B\n.Ek\n.No C\n".as_slice(),
            "A\nB C",
        ),
        (
            "macro-expanded-continuation-retains-authored-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.de X\n.Eo [\n.No BEFORE\\c\n.Ec\n.No \" AFTER\"\n..\n.Sh DESCRIPTION\n.X\n".as_slice(),
            "[BEFORE  AFTER",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-state-{label}.1")),
            source,
        )
        .expect("parse inline execution-state fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.flow().expect("Flow fixture").sections
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
fn private_and_keep_scopes_preserve_incoming_word_end_execution_state() {
    for (label, source, expected) in [
        (
            "tight-include",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p Ns In stdio.h\n.No AFTER\n".as_slice(),
            "A<stdio.h>\nAFTER",
        ),
        (
            "include-realizes-incoming-break-at-its-interior-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p Ns In \"B C\"\n.No AFTER\n".as_slice(),
            "A<B\nC> AFTER",
        ),
        (
            "keep-prephase-does-not-swallow-an-incoming-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\p\n.Bk -words\n.No A No B\n.Ek\n.No C\n".as_slice(),
            "BEFORE\nA B C",
        ),
        (
            "macro-expanded-keep-observes-each-executed-line",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.de XX\n.No A\\p\n.No B\n..\n.Sh DESCRIPTION\n.Bk -words\n.XX\n.Ek\n.No C\n".as_slice(),
            "A\nB C",
        ),
        (
            "alternating-man-scope-receives-an-incoming-word-end-break",
            b".TH PROBE 1\n.SH DESCRIPTION\nA\\p\n.BR \"B C\" D\nAFTER\n".as_slice(),
            "A\nB CD AFTER",
        ),
        (
            "kept-include",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p In stdio.h\n.Ek\n.No AFTER\n".as_slice(),
            "A <stdio.h>\nAFTER",
        ),
        (
            "same-line-keep-without-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A No B\n.Ek\n.No C\n".as_slice(),
            "A B C",
        ),
        (
            "cross-line-keep-without-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\n.No B\n.Ek\n.No C\n".as_slice(),
            "A B C",
        ),
        (
            "keep-boundary-settles-a-zero-advance-glyph",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\zX\n.No B\n.Ek\n.No C\n".as_slice(),
            "AXB C",
        ),
        (
            "empty-keep-word-settles-zero-advance-before-the-next-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\zX\n.No \"\"\n.No B\n.Ek\n.No C\n".as_slice(),
            "AX B C",
        ),
        (
            "empty-keep-word-realizes-word-end-break-with-one-following-boundary",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p\n.No \"\"\n.No B\n.Ek\n.No C\n".as_slice(),
            "A\n B C",
        ),
        (
            "keep-boundary-defers-word-end-break-after-settling-zero-advance",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\zX\\p\n.No B\n.Ek\n.No C\n".as_slice(),
            "AXB\nC",
        ),
        (
            "inner-keep-close-clears-the-global-outer-state",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\n.Bk -words\n.No B\n.Ek\n.No E\\p No \"F G\"\n.Ek\n".as_slice(),
            "A B E\nF G",
        ),
        (
            "target-does-not-realize-a-pending-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\n.Tg word-break-mark\n.No B C\n".as_slice(),
            // `.Tg` emits no formatter word, so the pending break survives
            // until the next `.No` begins its first word.  This is the fixed
            // CVS `term_word()` order, not an internal-space heuristic.
            "A\nB C",
        ),
        (
            "target-does-not-cover-a-pending-zero-advance-glyph",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\n.Tg zero-mark\n.No B\n".as_slice(),
            "AXB",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-private-state-{label}.1")),
            source,
        )
        .expect("parse private inline execution fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.flow().expect("Flow fixture").sections);
        };
        assert_eq!(inline_text(document.content(), children), expected, "{label}: {children:?}");
    }
}

#[test]
fn keep_words_survives_a_paragraph_output_flush() {
    let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p\n.Pp\n.No B\n.Ek\n.No C\n";
    let document = parse_manual_bytes(
        std::path::Path::new("inline-private-state-keep-paragraph.1"),
        source,
    )
    .expect("parse keep paragraph fixture");

    let [
        Block::Paragraph {
            children: first, ..
        },
        Block::VerticalSpace { lines: 1, .. },
        Block::Paragraph {
            children: second, ..
        },
    ] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "expected two paragraphs separated by one row: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(inline_text(document.content(), first), "A");
    assert_eq!(inline_text(document.content(), second), "B C");
}

#[test]
fn keep_words_tracks_repeated_macro_execution_and_transparent_targets() {
    for (label, macro_body, expected) in [
        ("repeated-lines", ".No A\\p\n.No B", "A\nB A\nB C"),
        (
            "target-between-lines",
            ".No A\\p\n.Tg inside-macro\n.No B",
            "A\nB A\nB C",
        ),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.de XX\n{macro_body}\n..\n.Sh DESCRIPTION\n.Bk -words\n.XX\n.XX\n.Ek\n.No C\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-keep-macro-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse repeated keep macro fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        if label == "target-between-lines" {
            assert!(
                children
                    .iter()
                    .filter(
                        |inline| matches!(inline, Inline::Anchor { id, .. } if id == "inside-macro")
                    )
                    .count()
                    >= 1,
                "{children:?}"
            );
        }
    }
}

#[test]
fn transparent_target_preserves_continuation_and_its_exact_anchor() {
    let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No BEFORE\\c\n.Ec\n.Tg mark\n AFTER\n";
    let document = parse_manual_bytes(
        std::path::Path::new("inline-state-transparent-target.1"),
        source,
    )
    .expect("parse transparent target fixture");
    let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "expected one paragraph: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(inline_text(document.content(), children), "[BEFORE  AFTER");
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Anchor { id, .. } if id == "mark")),
        "{children:?}"
    );
}

#[test]
fn overstrike_projects_one_terminal_cell_through_the_shared_zero_advance_state() {
    for (label, source, expected) in [
        ("ordinary", r"A\o'BC'D", "ACD"),
        ("trailing-blank", r"A\o'BC 'D", "ACD"),
        ("repeated-trailing-blanks", r"A\o'BC  'D", "ACD"),
        ("trailing-tab", "A\\o'BC\t'D", "ACD"),
        ("all-blanks", r"A\o'   'D", "AD"),
        ("zero-advance", r"A\z\o'BC'D", "AD"),
        ("zero-advance-trailing-blank-at-end", r"A\z\o'BC '", "AC"),
        ("empty", r"A\o''D", "AD"),
        ("nested-escape-spelling", r"A\o'BC\fI'D", "AID"),
        ("same-delimiter-numbered-escape", r"A\o'BC\N'8''D", "A'D"),
        ("same-delimiter-motion-escape", r"A\o'BC\h'1n''D", "A'D"),
        ("same-delimiter-character-escape", r"A\o'BC\C'x''D", "A'D"),
    ] {
        let manual =
            format!(".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {source}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-overstrike-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse overstrike projection fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.flow().expect("Flow fixture").sections
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
fn styled_overstrike_uses_the_complete_nested_escape_extent() {
    for (label, source, expected) in [
        ("quoted-number", r"A\o'BC\N'8''D", "A'D"),
        ("standard-nested-argument", r"A\o'1\f\N'39'2'B", "A2B"),
    ] {
        let manual =
            format!(".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Sy {source}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-overstrike-nested-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse styled nested overstrike fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert!(
            children.iter().any(
                |inline| matches!(inline, Inline::Strong { children } if inline_text(document.content(), children) == expected)
            ),
            "{label}: styled projection escaped its semantic scope: {children:?}"
        );
    }
}

#[test]
fn deeply_nested_escape_arguments_remain_bounded_at_the_native_boundary() {
    let mut source =
        String::from(".Dd September 12, 2026\n.Dt ESCAPE-DEPTH 1\n.Os\n.Sh DESCRIPTION\n.No ");
    source.push_str(&"\\o'".repeat(512));
    source.push('X');
    source.push_str(&"'".repeat(512));
    source.push_str("\n.No AFTER\n");

    let document = parse_manual_bytes(
        std::path::Path::new("deep-native-escape.1"),
        source.as_bytes(),
    )
    .expect("native escape depth exhaustion must remain a finite lowering");
    let text = visible_document_text(&document);
    assert!(text.contains("AFTER"), "document tail was lost: {text:?}");
    assert!(
        text.len() < source.len(),
        "rejected nesting must not be expanded into unbounded output"
    );
}

#[test]
fn numbered_and_named_nonbreaking_glyphs_defer_word_end_breaks() {
    for (label, spelling, expected_glyph) in [
        ("numbered-tab", r"\N'9'", "\t"),
        ("numbered-nbsp", r"\N'160'", "\u{a0}"),
        ("unicode-nbsp", "\u{a0}", "\u{a0}"),
        ("named-unicode-nbsp", r"\[u00A0]", "\u{a0}"),
        ("roff-nbsp", r"\~", " "),
        ("roff-digit-width-space", r"\0", " "),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\pB{spelling}C\n.No D\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-numbered-glyph-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse numbered glyph break fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            format!("AB{expected_glyph}C\nD"),
            "{label}: {children:?}"
        );
    }

    for (number, expected) in [("0", "�"), ("10", "�"), ("27", "�"), ("255", "ÿ")] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\pB\\N'{number}'C\n.No D\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-numbered-control-{number}.1")),
            manual.as_bytes(),
        )
        .expect("parse numbered terminal glyph fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{number}: expected one paragraph: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            format!("AB{expected}C\nD")
        );
    }
}

#[test]
fn literal_flow_uses_the_same_numbered_and_overstrike_glyph_projection() {
    for (label, source, expected) in [
        ("numbered-nbsp", r"A\pB\N'160'C", "AB\u{a0}C"),
        ("overstrike-trailing-blank", r"A\o'BC 'D", "ACD"),
        ("zero-advance-before-word-break", r"A\zX\p", "AX"),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No {source}\n.No D\n.Ed\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-literal-glyph-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse literal formatter glyph fixture");
        let [Block::Preformatted { children, .. }] =
            document.flow().expect("Flow fixture").sections[0]
                .blocks
                .as_slice()
        else {
            panic!(
                "{label}: expected one preformatted block: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        let expected = format!("{expected}\nD");
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
    }
}

#[test]
fn man_no_fill_settles_an_unrealized_word_end_break_at_the_physical_line() {
    for (source, expected_first_row) in [
        (r"PLAIN:A\pB", "PLAIN:AB"),
        (r"PLAIN:A\pB\N'160'C", "PLAIN:AB\u{a0}C"),
        (r"PLAIN:A\pB\N'9'C", "PLAIN:AB\tC"),
        (r"PLAIN:A\zX\p", "PLAIN:AX"),
    ] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\n{source}\nNEXT\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new("inline-man-no-fill-word-end-break.1"),
            manual.as_bytes(),
        )
        .expect("parse man no-fill word-end break fixture");
        let [Block::Preformatted { children, .. }] =
            document.flow().expect("Flow fixture").sections[0]
                .blocks
                .as_slice()
        else {
            panic!(
                "expected one preformatted block: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children)
                .matches('\n')
                .count(),
            1,
            "{source}: {children:?}"
        );
        assert_eq!(
            inline_text(document.content(), children),
            format!("{expected_first_row}\nNEXT"),
            "{source}: {children:?}"
        );
    }
}

#[test]
fn zero_advance_state_crosses_only_continued_no_fill_rows() {
    for (label, manual, expected) in [
        (
            "man",
            ".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\nB\n.fi\n",
            "AB",
        ),
        (
            "mdoc",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No A\\zX\\c\n.No B\n.Ed\n",
            "AB",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-{label}-continued-zero-advance.1")),
            manual.as_bytes(),
        )
        .expect("parse continued no-fill zero-advance fixture");
        let [Block::Preformatted { children, .. }] =
            document.flow().expect("Flow fixture").sections[0]
                .blocks
                .as_slice()
        else {
            panic!(
                "expected one preformatted block: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-continued-word-end-break.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\p\\c\nB\n.fi\n",
    )
    .expect("parse continued no-fill word-end-break fixture");
    let [Block::Preformatted { children, .. }] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "expected one preformatted block: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(
        inline_text(document.content(), children),
        "AB",
        "{children:?}"
    );
}

#[test]
fn no_fill_mode_boundaries_preserve_only_unoccupied_zero_advance_state() {
    for (label, no_fill, expected, rejected) in [
        ("filled-to-no-fill-bare", "B C", "BC", "B C"),
        ("no-fill-to-filled-bare", "\\z\n.fi\nB C", "BC", "B C"),
        ("visible-cell", "A\\z\n.fi\nB C", "AB C", "ABC"),
        ("row-marker-cell", "\\&\\z\n.fi\nB C", "B C", "BC"),
        ("word-break-cell", "\\p\\z\n.fi\nB C", "B C", "BC"),
        ("completed-and-armed", "\\zX\\z\n.fi\nB C", "XB C", "XBC"),
    ] {
        let source = if label == "filled-to-no-fill-bare" {
            format!(".TH PROBE 1\n.SH DESCRIPTION\n\\z\n.nf\n{no_fill}\n.fi\n")
        } else {
            format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\n{no_fill}\n")
        };
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-no-fill-boundary-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse no-fill execution boundary fixture");
        let visible = projected_document_text(&document);
        assert!(
            visible.contains(expected),
            "{label}: expected {expected:?} in {visible:?}; {:#?}",
            document.flow().expect("Flow fixture").sections
        );
        assert!(!visible.contains(rejected), "{label}: {visible:?}");
    }
}

#[test]
fn trailing_no_space_preserves_a_pending_word_end_break() {
    for (label, wrapper_open, wrapper_close, expected_block) in [
        ("filled", "", "", "paragraph"),
        ("literal", ".Bd -literal\n", ".Ed\n", "preformatted"),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{wrapper_open}.No BEFORE\\p\\c\n.No AFTER LAST\n{wrapper_close}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-word-end-no-space-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse trailing no-space word-end fixture");
        let children = match document.flow().expect("Flow fixture").sections[0]
            .blocks
            .as_slice()
        {
            [Block::Paragraph { children, .. }] if expected_block == "paragraph" => children,
            [Block::Preformatted { children, .. }] if expected_block == "preformatted" => children,
            blocks => panic!("{label}: unexpected blocks: {blocks:#?}"),
        };
        assert_eq!(
            inline_text(document.content(), children),
            "BEFOREAFTER\nLAST",
            "{label}"
        );
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak { .. }))
                .count(),
            1,
            "{label}: {children:?}"
        );
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-word-end-no-space-keep.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No BEFORE\\p\\c\n.No AFTER LAST\n.Ek\n",
    )
    .expect("parse kept trailing no-space word-end fixture");
    let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "unexpected keep blocks: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(
        inline_text(document.content(), children),
        "BEFOREAFTER LAST"
    );
    assert!(
        !children
            .iter()
            .any(|node| matches!(node, Inline::LineBreak { .. }))
    );

    for (label, source, expected) in [
        (
            "alternating-man-font",
            b".TH PROBE 1\n.SH DESCRIPTION\n.BR \"BEFORE\\p\\c\" \"AFTER LAST\"\n".as_slice(),
            "BEFOREAFTER\nLAST",
        ),
        (
            "private-include",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\p\\c Ns In \"AFTER LAST\"\n".as_slice(),
            "BEFORE<AFTER\nLAST>",
        ),
        (
            "enclosure-close",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No BEFORE\\p\\c\n.Ec\n.No AFTER LAST\n".as_slice(),
            "[BEFORE\nAFTER LAST",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-word-end-no-space-{label}.1")),
            source,
        )
        .expect("parse scoped trailing no-space word-end fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.flow().expect("Flow fixture").sections);
        };
        assert_eq!(inline_text(document.content(), children), expected, "{label}: {children:?}");
    }
}
