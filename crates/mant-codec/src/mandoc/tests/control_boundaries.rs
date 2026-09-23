use super::*;

#[test]
fn formatter_request_boundaries_execute_inside_mdoc_scopes() {
    for (label, request, expected, breaks) in [
        ("filled-margin", ".mc |", "AX B", 0),
        ("filled-temporary-indent", ".ti 4n", "AX\nB", 1),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\\c\n{request}\n.No B\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-control-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse filled control boundary fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak { .. }))
                .count(),
            breaks,
            "{label}: {children:?}"
        );
    }

    for (label, request, expected, breaks) in [
        ("margin", ".mc |", "AX B", 0),
        ("temporary-indent", ".ti 4n", "AX\nB", 1),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No A\\zX\\c\n{request}\n.No B\n.Ed\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-display-control-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse display control boundary fixture");
        let [Block::Preformatted { children, .. }] =
            document.flow().expect("Flow fixture").sections[0]
                .blocks
                .as_slice()
        else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak { .. }))
                .count(),
            breaks,
            "{label}: {children:?}"
        );
    }
}

#[test]
fn formatter_request_boundaries_execute_inside_nested_mdoc_scopes() {
    for (label, request, expected) in [
        ("margin", ".mc |", "[AX B"),
        ("temporary-indent", ".ti 4n", "[AX\nB"),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\\zX\\c\n{request}\n.No B\n.Ec\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-container-{label}-control.1")),
            manual.as_bytes(),
        )
        .expect("parse nested control boundary fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: unexpected nested-control blocks: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
    }

    for (label, request, expected, breaks) in [
        ("kept-margin", ".mc |", "AX B", 0),
        ("kept-temporary-indent", ".ti 4n", "AX\nB", 1),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\zX\\c\n{request}\n.No B\n.Ek\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-control-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse kept control boundary fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak { .. }))
                .count(),
            breaks,
            "{label}: {children:?}"
        );
    }
}

#[test]
fn formatter_requests_use_the_current_cell_not_prior_document_output() {
    for (label, request) in [("break", ".br"), ("indent", ".ti 4n")] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\p\n{request}\n.No B C\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("formatter-leading-break-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse leading buffered word-end break");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            "\nB C",
            "{label}: {children:?}"
        );
    }

    for (label, first, expected) in [
        ("pending", r"\zX", "X B"),
        ("continued-pending", r"\zX\c", "X B"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {first}\n.mc |\n.No B\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("formatter-zero-cell-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse zero-advance formatter cell");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
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
        std::path::Path::new("formatter-transparent-target.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\n.ti 4n\n.Tg mark\n.ti 4n\n.No B\n",
    )
    .expect("parse formatter boundaries around a transparent target");
    let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "unexpected target-boundary blocks: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(
        inline_text(document.content(), children),
        "A\nB",
        "{children:?}"
    );
    assert_eq!(
        children
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak { .. }))
            .count(),
        1,
        "{children:?}"
    );
    assert!(children.iter().any(|node| matches!(
        node,
        Inline::Anchor { id, .. } if id == "mark"
    )));
}

#[test]
fn vertical_space_uses_the_cvs_fallback_and_preserves_unflushed_execution() {
    for (label, opening, closing) in [("top", "", ""), ("display", ".Bd -literal\n", ".Ed\n")] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{opening}.No A\n.sp bogus\n.No B\n{closing}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("vertical-space-invalid-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse invalid vertical-space operand");
        let blocks = &document.flow().expect("Flow fixture").sections[0].blocks;
        assert_eq!(blocks.len(), 3, "{label}: {blocks:#?}");
        assert!(matches!(
            blocks[0],
            Block::Paragraph { ref children, .. }
                | Block::Preformatted { ref children, .. }
                if inline_text(document.content(), children) == "A"
        ));
        assert!(matches!(blocks[1], Block::VerticalSpace { lines: 1, .. }));
        assert!(matches!(
            blocks[2],
            Block::Paragraph { ref children, .. }
                | Block::Preformatted { ref children, .. }
                if inline_text(document.content(), children) == "B"
        ));
        assert!(!visible_document_text(&document).contains("bogus"));
    }

    let nested = parse_manual_bytes(
        std::path::Path::new("vertical-space-invalid-nested.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\n.sp bogus\n.No B\n.Ec\n",
    )
    .expect("parse nested invalid vertical-space operand");
    let [Block::Paragraph { children, .. }] = nested.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "unexpected nested spacing blocks: {:#?}",
            nested.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(
        inline_text(nested.content(), children),
        "[A\n\nB",
        "{children:?}"
    );

    let armed = parse_manual_bytes(
        std::path::Path::new("vertical-space-armed-zero.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\z\n.sp 1\n.No B C\n",
    )
    .expect("parse vertical space after an armed zero-advance escape");
    let [
        Block::VerticalSpace { lines: 1, .. },
        Block::Paragraph { children, .. },
    ] = armed.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "unexpected armed spacing blocks: {:#?}",
            armed.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(inline_text(armed.content(), children), "BC", "{children:?}");

    for (label, escape, spacing) in [
        ("pending", r"\p", ".sp 1"),
        ("pending-continued", r"\p\c", ".sp 1"),
        ("pending-invalid", r"\p", ".sp bogus"),
        ("pending-continued-invalid", r"\p\c", ".sp bogus"),
        ("armed-then-pending", r"\z\p", ".sp 1"),
        ("armed-pending-continued", r"\z\p\c", ".sp bogus"),
        ("armed-continued-pending", r"\z\c\p", ".sp 1"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {escape}\n{spacing}\n.No B C\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("vertical-space-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse vertical space after a leading word-end break");
        let [
            Block::VerticalSpace { lines: 1, .. },
            Block::VerticalSpace { lines: 1, .. },
            Block::Paragraph { children, .. },
        ] = document.flow().expect("Flow fixture").sections[0]
            .blocks
            .as_slice()
        else {
            panic!(
                "unexpected leading-break spacing blocks: {:#?}",
                document.flow().expect("Flow fixture").sections
            );
        };
        assert_eq!(
            inline_text(document.content(), children),
            "B C",
            "{label}: {children:?}"
        );
    }
}

#[test]
fn pending_inline_execution_is_settled_before_structural_owners() {
    for (label, escape, expects_gap) in [("word-end", r"\p", true), ("zero-advance", r"\z", false)]
    {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {escape}\n.Bl -bullet\n.It\nITEM\n.El\n.No AFTER LAST\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("execution-before-list-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse pending execution before a structural list");
        assert_eq!(
            visible_document_text(&document)
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["DESCRIPTION", "ITEM", "AFTER", "LAST"],
            "{label}: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
        let blocks = &document.flow().expect("Flow fixture").sections[0].blocks;
        let list_index = blocks
            .iter()
            .position(|block| matches!(block, Block::List { .. }))
            .expect("retain the structural list");
        let after_index = blocks
            .iter()
            .position(|block| {
                matches!(block, Block::Paragraph { children, .. } if inline_text(document.content(), children) == "AFTER LAST")
            })
            .expect("retain the paragraph after the list");
        assert_eq!(after_index, list_index + 1, "{label}: {blocks:#?}");
        assert_eq!(
            blocks[..list_index]
                .iter()
                .filter(|block| matches!(block, Block::VerticalSpace { lines: 1, .. }))
                .count(),
            usize::from(expects_gap),
            "{label}: {blocks:#?}"
        );
    }
}

#[test]
fn bare_zero_advance_crosses_only_structures_without_generated_words() {
    for (label, body, expected) in [
        ("plain-list", ".Bl -item -compact\n.It\n.No B C\n.El", "BC"),
        (
            "literal-display",
            ".Bd -literal -compact\n.No B C\n.Ed",
            "BC",
        ),
        ("one-line-display", ".D1 B C", "BC"),
        ("literal-one-line-display", ".Dl B C", "BC"),
        ("bibliography", ".Rs\n.%A B C\n.Re", "BC."),
        (
            "bullet-generated-word",
            ".Bl -bullet -compact\n.It\n.No B C\n.El",
            "B C",
        ),
        (
            "ordered-generated-word",
            ".Bl -enum -compact\n.It\n.No B C\n.El",
            "B C",
        ),
    ] {
        let source =
            format!(".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\z\n{body}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-structure-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse zero-advance structural boundary");
        let visible = projected_document_text(&document);
        assert!(
            visible.contains(expected),
            "{label}: expected {expected:?} in {visible:?}; {:#?}",
            document.flow().expect("Flow fixture").sections
        );
        if expected == "BC" {
            assert!(!visible.contains("B C"), "{label}: {visible:?}");
        }
    }

    // tbl_term.c explicitly clears both backtracking flags before each cell;
    // tables are not transparent structural owners for a bare `\z`.
    let table = parse_manual_bytes(
        std::path::Path::new("zero-advance-structure-table.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\z\n.TS\nl.\nB C\n.TE\n",
    )
    .expect("parse zero-advance table boundary");
    assert!(visible_document_text(&table).contains("B C"));

    let completed_and_armed = parse_manual_bytes(
        std::path::Path::new("zero-advance-structure-active-cell.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\zX\\z\n.Bd -literal -compact\n.No B C\n.Ed\n",
    )
    .expect("parse active zero-advance cell before a display");
    let visible = projected_document_text(&completed_and_armed);
    assert!(visible.contains("XB C"), "{visible:?}");
    assert!(!visible.contains("XBC"), "{visible:?}");

    let man = parse_manual_bytes(
        std::path::Path::new("zero-advance-man-relative-scope.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n\\z\n.RS 4\nB C\n.RE\n",
    )
    .expect("parse man zero-advance structural boundary");
    let visible = projected_document_text(&man);
    assert!(visible.contains("BC"), "{visible:?}");
    assert!(!visible.contains("B C"), "{visible:?}");
}

#[test]
fn bsd_two_operand_forms_execute_generated_words_and_joiners() {
    for (label, opening, closing, operands, follower, expected) in [
        ("plain", "", "", "4.4 Tahoe", "", "4.4BSD-Tahoe"),
        (
            "word-end",
            "",
            "",
            r"\p\c Tahoe",
            ".No AFTER LAST\n",
            "BSD-Tahoe\nAFTER LAST",
        ),
        (
            "continued-literal",
            ".Bd -literal\n",
            ".Ed\n",
            r"\c Tahoe",
            ".No AFTER\n",
            "BSD-Tahoe\nAFTER",
        ),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{opening}.Bx {operands}\n{follower}{closing}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("bsd-two-operands-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse two-operand Bx fixture");
        let children = match document.flow().expect("Flow fixture").sections[0]
            .blocks
            .as_slice()
        {
            [Block::Paragraph { children, .. }] if opening.is_empty() => children,
            [Block::Preformatted { children, .. }] => children,
            blocks => panic!("{label}: unexpected blocks: {blocks:#?}"),
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
    }

    for (label, opening, closing, operands, follower, expected) in [
        (
            "kept",
            ".Bk -words\n",
            ".Ek\n",
            r"\p\c Tahoe",
            ".No AFTER LAST\n",
            "BSD-Tahoe\nAFTER LAST",
        ),
        (
            "private-enclosure",
            ".Eo [\n",
            ".Ec\n",
            r"\c Tahoe",
            ".No FINAL\n",
            "[BSD-Tahoe FINAL",
        ),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{opening}.Bx {operands}\n{closing}{follower}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("bsd-two-operands-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse scoped two-operand Bx fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
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
fn filled_margin_flush_settles_the_current_cell_without_a_hard_break() {
    // In filled mode CVS roff_term_pre_mc() flushes with TERMP_NOBREAK.
    // A trailing `\p` is settled with that cell instead of becoming a hard
    // line; literal mode above has already closed its physical source row.
    for (label, keep_open, keep_close, first, expected) in [
        ("filled-word-break", "", "", r".No A\p", "A B C"),
        (
            "filled-zero-width-word-break",
            "",
            "",
            r".No A\zX\p",
            "AX B C",
        ),
        (
            "filled-canceled-arm-word-break",
            "",
            "",
            r".No A\z\p\c",
            "A B C",
        ),
        (
            "kept-word-break",
            ".Bk -words\n",
            ".Ek\n",
            r".No A\p",
            "A B C",
        ),
        ("generated-word-break", "", "", r".Bx \p", "BSD B C"),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{keep_open}{first}\n.mc |\n.No B C\n{keep_close}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-margin-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse filled no-break margin fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
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
fn margin_flush_obeys_current_cell_and_continuation_state() {
    for (label, header, opening, first, request, closing, expected) in [
        (
            "mdoc-column",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal",
            ".No A",
            ".mc |",
            ".Ed",
            "A\nB",
        ),
        (
            "mdoc-zero-column",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal",
            ".No \\zX",
            ".mc |",
            ".Ed",
            "X\nB",
        ),
        (
            "man-column",
            ".TH PROBE 1\n.SH DESCRIPTION",
            ".nf",
            "A",
            ".mc |",
            ".fi",
            "A\nB",
        ),
        (
            "man-zero-column",
            ".TH PROBE 1\n.SH DESCRIPTION",
            ".nf",
            r"\zX",
            ".mc |",
            ".fi",
            "X\nB",
        ),
    ] {
        let manual = format!("{header}\n{opening}\n{first}\n{request}\nB\n{closing}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-control-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse conditional margin flush fixture");
        let [Block::Preformatted { children, .. }] =
            document.flow().expect("Flow fixture").sections[0]
                .blocks
                .as_slice()
        else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
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
fn margin_flush_preserves_nested_and_continued_execution() {
    let document = parse_manual_bytes(
        std::path::Path::new("inline-nested-vertical-space.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\n.sp 2\n.No B\n.Ec\n",
    )
    .expect("parse nested vertical-space fixture");
    let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "unexpected nested vertical-space blocks: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(
        inline_text(document.content(), children),
        "[A\n\n\nB",
        "{children:?}"
    );

    let document = parse_manual_bytes(
        std::path::Path::new("inline-repeated-margin-flush.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No A\\c\n.mc |\n.Tg between-margins\n.mc !\n.No B\n.Ed\n",
    )
    .expect("parse repeated margin flush fixture");
    let [Block::Preformatted { children, .. }] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "unexpected repeated-margin blocks: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(
        inline_text(document.content(), children),
        "A B",
        "{children:?}"
    );
    assert!(children.iter().any(|node| matches!(
        node,
        Inline::Anchor { id, .. } if id == "between-margins"
    )));

    for (label, first, expected) in [
        // With `\c`, CVS retains the occupied zero-width field until `.mc`;
        // its NOBREAK flush commits one relative separator before the next
        // source word even though the field has no visible glyph.
        ("word-break", r"\p\c", " B C"),
        ("zero-width-word-break", r"\&\p\c", " B C"),
        ("uncontinued-word-break", r"\p", "\nB C"),
        ("armed-word-break", r"\z\p", "\nB C"),
        ("canceled-arm-word-break", r"\z\p\c", "\nB C"),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No {first}\n.mc |\n.No B C\n.Ed\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-margin-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse pending formatter-cell margin fixture");
        let [Block::Preformatted { children, .. }] =
            document.flow().expect("Flow fixture").sections[0]
                .blocks
                .as_slice()
        else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
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
fn bsd_replacement_executes_as_a_generated_formatter_word() {
    for (label, wrapper_open, wrapper_close, operand, expected, breaks) in [
        ("filled", "", "", r"\c", "BSD AFTER", 0),
        ("literal", ".Bd -literal\n", ".Ed\n", r"\c", "BSD\nAFTER", 1),
        ("word-end", "", "", r"\p\c", "BSD\nAFTER LAST", 1),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{wrapper_open}.Bx {operand}\n.No AFTER{}\n{wrapper_close}",
            if label == "word-end" { " LAST" } else { "" }
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-bx-generated-word-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse generated BSD word fixture");
        let children = match document.flow().expect("Flow fixture").sections[0]
            .blocks
            .as_slice()
        {
            [Block::Paragraph { children, .. }] if label != "literal" => children,
            [Block::Preformatted { children, .. }] if label == "literal" => children,
            blocks => panic!("{label}: unexpected blocks: {blocks:#?}"),
        };
        assert_eq!(
            inline_text(document.content(), children),
            expected,
            "{label}: {children:?}"
        );
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak { .. }))
                .count(),
            breaks,
            "{label}: {children:?}"
        );
    }

    // CVS post_bx() inserts Ns + BSD after the first authored operand.  An
    // explicit empty formatter word consumes an incoming `\c` before that
    // generated tight word; ordinary input still keeps its inter-word space.
    for (operand_label, operand) in [("empty", r#""""#), ("zero-width", r"\&"), ("font", r"\fB")] {
        for (boundary_label, preceding, expected) in [
            ("ordinary", "A", "A BSD B"),
            ("continued", r"A\c", "ABSD B"),
        ] {
            let manual = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {preceding}\n.Bx {operand}\n.No \\fPB\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new(&format!("inline-bx-{operand_label}-{boundary_label}.1")),
                manual.as_bytes(),
            )
            .expect("parse empty BSD operand boundary fixture");
            let [Block::Paragraph { children, .. }] =
                document.flow().expect("Flow fixture").sections[0]
                    .blocks
                    .as_slice()
            else {
                panic!(
                    "{operand_label}/{boundary_label}: {:#?}",
                    document.flow().expect("Flow fixture").sections
                );
            };
            assert_eq!(
                inline_text(document.content(), children),
                expected,
                "{operand_label}/{boundary_label}: {children:?}"
            );
        }
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-bx-generated-word-keep.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.Bx \\p\\c\n.No AFTER LAST\n.Ek\n",
    )
    .expect("parse kept generated BSD word fixture");
    let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "unexpected kept Bx blocks: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(
        inline_text(document.content(), children),
        "BSD\nAFTER LAST",
        "{children:?}"
    );
    assert_eq!(
        children
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak { .. }))
            .count(),
        1,
        "{children:?}"
    );
}

#[test]
#[allow(clippy::too_many_lines)] // Reference-derived no-fill matrix stays together.
fn no_fill_execution_state_crosses_transparent_requests_only() {
    for (label, request) in [("font", ".ft B"), ("paragraph-distance", ".PD 1")] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n{request}\nB\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-no-fill-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse transparent no-fill request fixture");
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
            "AB",
            "{label}: {children:?}"
        );
        if label == "font" {
            assert!(children.iter().any(|inline| matches!(
                inline,
                Inline::Strong { children } if inline_text(document.content(), children) == "B"
            )));
        }
    }

    for (label, request) in [
        ("indent", ".in 4"),
        ("temporary-indent", ".ti 4"),
        ("break", ".br"),
        ("space", ".sp"),
        ("fill", ".fi\n.nf"),
        ("repeated-no-fill", ".nf"),
    ] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n{request}\nB\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-no-fill-{label}-boundary.1")),
            manual.as_bytes(),
        )
        .expect("parse no-fill boundary fixture");
        let text = document.flow().expect("Flow fixture").sections[0]
            .blocks
            .iter()
            .map(|block| match block {
                Block::Preformatted { children, .. } | Block::Paragraph { children, .. } => {
                    inline_text(document.content(), children)
                }
                Block::VerticalSpace { .. } => "\n".to_owned(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("AX"),
            "{label}: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
        assert!(
            text.contains('B'),
            "{label}: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
        assert!(
            !text.contains("AB"),
            "{label}: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-no-fill-margin-character.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n.mc |\nB\n.fi\n",
    )
    .expect("parse no-break margin-character fixture");
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
        "AX B",
        "{children:?}"
    );

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-no-fill-margin-pending-cell.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\n\\zX\\c\n.mc |\nB\n.fi\n",
    )
    .expect("parse no-break margin pending-cell fixture");
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
        "X B",
        "{children:?}"
    );

    for (label, first, expected) in [
        ("plain", "A", "A B"),
        ("continued-zero-advance", r"A\zX\c", "AX B"),
    ] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n{first}\n.mc |\nB\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-filled-margin-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse filled no-break margin-character fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!(
                "expected one paragraph: {:#?}",
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
fn invisible_continued_no_fill_cells_reach_temporary_and_no_break_flushes() {
    for (label, header, opening, request, closing, expected) in [
        (
            "mdoc-ti",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal",
            ".ti 4n",
            ".Ed",
            "\nB",
        ),
        (
            "mdoc-mc",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal",
            ".mc |",
            ".Ed",
            " B",
        ),
        (
            "man-ti",
            ".TH PROBE 1\n.SH DESCRIPTION",
            ".nf",
            ".ti 4n",
            ".fi",
            "\nB",
        ),
        (
            "man-mc",
            ".TH PROBE 1\n.SH DESCRIPTION",
            ".nf",
            ".mc |",
            ".fi",
            " B",
        ),
    ] {
        let manual = format!("{header}\n{opening}\n\\&\\c\n{request}\nB\n{closing}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("invisible-no-fill-cell-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse invisible continued no-fill cell");
        let [Block::Preformatted { children, .. }] =
            document.flow().expect("Flow fixture").sections[0]
                .blocks
                .as_slice()
        else {
            panic!(
                "{label}: unexpected blocks: {:#?}",
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
fn no_fill_exit_and_document_end_settle_continued_zero_advance_state() {
    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-zero-advance-before-fi.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n.fi\nB\n",
    )
    .expect("parse no-fill exit fixture");
    let [
        Block::Preformatted {
            children: literal, ..
        },
        Block::Paragraph {
            children: filled, ..
        },
    ] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "expected literal and filled blocks: {:#?}",
            document.flow().expect("Flow fixture").sections
        );
    };
    assert_eq!(
        inline_text(document.content(), literal),
        "AX",
        "{literal:?}"
    );
    assert_eq!(inline_text(document.content(), filled), "B", "{filled:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-zero-advance-at-eof.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c",
    )
    .expect("parse no-fill EOF fixture");
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
        "AX",
        "{children:?}"
    );
}

#[test]
fn semantic_link_identity_executes_zero_advance_controls_without_guessing_display_text() {
    for (label, macro_name, source, expected) in [
        (
            "mail-no-space",
            "Mt",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt user@\\z\\cexample.org\n".as_slice(),
            "user@example.org",
        ),
        (
            "mail-zero-advance-glyph",
            "Mt",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt user@\\zXexample.org\n".as_slice(),
            "user@example.org",
        ),
        (
            "mail-zero-advance-font-control",
            "Mt",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt user@\\z\\fBexample.org\n".as_slice(),
            "user@xample.org",
        ),
        (
            "link-zero-width",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\z\\&suffix label\n".as_slice(),
            "https://example.org/suffix",
        ),
        (
            "link-word-break",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\z\\psuffix label\n".as_slice(),
            "https://example.org/suffix",
        ),
        (
            "link-no-space",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\z\\csuffix label\n".as_slice(),
            "https://example.org/suffix",
        ),
        (
            "link-unknown-glyph",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\[nosuch]suffix label\n".as_slice(),
            "https://example.org/suffix",
        ),
        (
            "mail-out-of-range-numbered-glyph",
            "Mt",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt user@\\N'999'example.org\n".as_slice(),
            "user@example.org",
        ),
        (
            "link-html-device-name",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\*[.T] label\n".as_slice(),
            "https://example.org/html",
        ),
        (
            "link-overstrike-final-glyph",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\o'ab' label\n".as_slice(),
            "https://example.org/b",
        ),
        (
            "link-overstrike-standard-nested-argument",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/A\\o'1\\f\\N'39'2'B label\n".as_slice(),
            "https://example.org/A2B",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("link-identity-{label}.1")),
            source,
        )
        .expect("parse link identity fixture");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.flow().expect("Flow fixture").sections);
        };
        let target = children.iter().find_map(|inline| link_target(&document, inline));
        match macro_name {
            "Mt" => assert!(
                matches!(target, Some(mant_ir::LinkTarget::Email { address }) if address == expected),
                "{label}: wrong typed email target: {target:?}"
            ),
            "Lk" => assert!(
                matches!(target, Some(mant_ir::LinkTarget::External { uri }) if uri == expected),
                "{label}: wrong typed external target: {target:?}"
            ),
            _ => unreachable!(),
        }
    }
}

#[test]
fn zero_advance_treats_every_empty_enclosure_delimiter_as_a_formatter_word() {
    for (macro_name, delimiters) in [
        ("Dq", "“”"),
        ("Op", "[]"),
        ("Pq", "()"),
        ("Brq", "{}"),
        ("Sq", "‘’"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\n.{macro_name}\n.No B\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-empty-{macro_name}.1")),
            source.as_bytes(),
        )
        .expect("parse empty mdoc enclosure");
        let [Block::Paragraph { children, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!("{macro_name}: expected one paragraph");
        };
        assert_eq!(
            inline_text(document.content(), children),
            format!("AX{delimiters} B"),
            "{macro_name}: {children:?}"
        );
    }
}

#[test]
fn visible_glyphs_before_a_definition_break_do_not_detach_the_head() {
    let document = parse_manual_bytes(
        std::path::Path::new("definition-glyph-before-break.1"),
        b".TH DEFINITION 1\n.SH DESCRIPTION\n.TP\n.B x\n\\[u03B1]\n.br\nBODY\n",
    )
    .expect("parse visible glyph before a definition body break");
    let text = visible_document_text(&document);

    // A glyph decoded from a named escape is visible content, not a formatter
    // transition. It therefore remains with x before `.br` starts BODY.
    assert!(text.contains("x α \nBODY"), "{text:?}");
}
