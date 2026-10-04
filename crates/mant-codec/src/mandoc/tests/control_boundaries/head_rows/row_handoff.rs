use super::*;

#[test]
fn definition_head_handoff_uses_native_close_and_executed_continuation() {
    // Each exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. The entry
    // rule in mdoc_term.c::print_mdoc_node() runs NODE_LINE before dispatch;
    // mdoc_macro.c::blk_exp_close() marks explicit closes NODE_BROKEN. Bq is
    // an ordinary block and cannot stand in for that close event.
    for (label, source, term, body_blank_rows) in [
        (
            "ordinary-bq",
            ".nf\n.Bl -inset\n.It Bq X\n.br\n.No BODY\n.El\n",
            "[X]",
            0,
        ),
        (
            "closed-xo",
            ".nf\n.Bl -inset\n.It Xo X\n.Xc\n.br\n.No BODY\n.El\n",
            "X",
            1,
        ),
        (
            "continued-xo",
            ".nf\n.Bl -inset\n.It Xo\n.No X\\c\n.Xc\n.br\n.No BODY\n.El\n",
            "X",
            0,
        ),
        (
            "closed-bo",
            ".nf\n.Bl -inset\n.It Bo X\n.Bc\n.br\n.No BODY\n.El\n",
            "[X]",
            1,
        ),
        (
            "fill-changes-in-head",
            ".Bl -inset\n.It Xo X\n.nf\n.No Y\n.Xc\n.br\n.No BODY\n.El\n",
            "X\nY",
            1,
        ),
    ] {
        let item = definition_item_from_source(source);
        assert_eq!(inline_text(&item.terms[0]), term, "{label}: {item:#?}");
        let rows: u16 = item
            .description
            .iter()
            .filter_map(|block| match block {
                Block::VerticalSpace { lines, .. } => Some(*lines),
                _ => None,
            })
            .sum();
        assert_eq!(rows, body_blank_rows, "{label}: {item:#?}");
    }
}

#[test]
fn literal_definition_body_shares_only_a_continued_head_row() {
    // Exact positive and negative inputs passed fixed CVS -Tascii/-Tutf8/
    // -Tlint. mdoc_term.c enters no-fill BODY at NODE_LINE; TERMP_NONEWLINE
    // from a final \c suppresses that row break and survives the IR drain.
    for (head, joins) in [(".No X\\c", true), (".No X", false)] {
        let item = definition_item_from_source(&format!(
            ".nf\n.Bl -hang -width 4n\n.It Xo\n{head}\n.Xc\n.No BODY\n.El\n"
        ));
        assert!(matches!(
            item.description.first(),
            Some(Block::Preformatted { .. })
        ));
        assert_eq!(item.inline_term(), joins, "{head}: {item:#?}");
        assert_eq!(
            item.inline_description().is_some(),
            joins,
            "{head}: {item:#?}"
        );
    }
}

#[test]
fn inset_head_drain_keeps_native_word_separator_state() {
    // Exact variants checked with fixed CVS -Tutf8 (ascii pins the old
    // visible-X expectation). In mdoc_term.c::termp_it_pre(), the inset
    // BODY's generated `\\ ` cell executes encode1(U+00A0), which consumes
    // the pending HEAD glyph's BACKBEFORE retreat and overstrikes it
    // (term.c:901-908: `X^H<NBSP>`): the HEAD term loses its only glyph and
    // the body owns the row. NOSPACE for an empty BODY word still precedes
    // the visible word.
    for (middle, expected_body) in [
        (".No \"\"\n", "  BODY"),
        (".No \\&\n", "  BODY"),
        (".No \\fB\n", "  BODY"),
        ("", " BODY"),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd review probe\n.Sh DESCRIPTION\n.Bl -inset\n.It \\zX\n{middle}.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("inset-word-state.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("missing definition item: {document:#?}");
        };
        assert!(
            items[0]
                .terms
                .iter()
                .all(|term| inline_text(term).is_empty()),
            "{middle}: {items:#?}"
        );
        let Block::Paragraph { children, .. } = &items[0].description[0] else {
            panic!("missing definition body: {items:#?}");
        };
        assert_eq!(inline_text(children), expected_body, "{middle}: {items:#?}");
    }
}

#[test]
fn unrendered_definition_head_does_not_consume_body_row() {
    // Exact inset and diagnostic forms checked with fixed CVS -Tascii/-Tlint.
    // mdoc_term.c::termp_it_pre() executes the BODY separator through
    // term_word(); roff_term_pre_br() then closes its occupied row. A \& HEAD
    // has no rendered term to own that completed blank line.
    for style in ["inset", "diag"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -{style}\n.It \\&\n.br\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("invisible-definition-head.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{style}: {document:#?}");
        };
        assert!(items[0].terms.is_empty(), "{style}: {items:#?}");
        assert!(
            matches!(&items[0].description[0], Block::Paragraph { children, .. } if inline_text(children).ends_with("\nBODY")),
            "{style}: {items:#?}"
        );
    }
}

#[test]
fn no_fill_body_does_not_replay_detached_head_cell() {
    // Exact forms checked with fixed CVS -Tutf8. A completed \zX belongs to
    // the HEAD row, and the inset BODY's generated `\ ` cell overstrikes it
    // (encode1 consumes BACKBEFORE, term.c:901-908): the HEAD term loses X.
    // An empty or font-only BODY word adds no new cell, while \& adds a
    // genuine invisible BODY cell and its own row.
    for (body_word, body_breaks) in [(".No \"\"", 0), (".No \\fB", 0), (".No \\&", 1)] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n.Bl -inset\n.It \\zX\n{body_word}\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("no-fill-head-cell-owner.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{body_word}: {document:#?}");
        };
        assert!(items[0].terms.is_empty(), "{body_word}: {items:#?}");
        let children = items[0]
            .description
            .iter()
            .find_map(|block| match block {
                Block::Preformatted { children, .. } => Some(children),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{body_word}: {items:#?}"));
        assert_eq!(
            children
                .iter()
                .filter(|inline| matches!(inline, Inline::LineBreak { .. }))
                .count(),
            body_breaks,
            "{body_word}: {children:#?}"
        );
        assert!(
            inline_text(children).ends_with("BODY"),
            "{body_word}: {children:#?}"
        );
    }
}

#[test]
fn whitespace_definition_terms_own_their_rendered_head_row() {
    // Exact inset/diag and whitespace spellings checked with fixed CVS
    // -Tascii/-Tlint. term.c::term_field() can print a row occupied only by
    // spaces; mdoc_term.c::termp_it_pre() then closes that existing HEAD row.
    // The invisible \& counterexample is covered by the preceding test.
    for style in ["inset", "diag"] {
        for head in ["\" \"", "\\~", "\\0"] {
            let source = format!(
                ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -{style}\n.It {head}\n.br\n.No BODY\n.El\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("whitespace-head-owner.1"),
                source.as_bytes(),
            )
            .unwrap();
            let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
                panic!("{style}, {head}: {document:#?}");
            };
            assert!(mant_ir::has_printable_character(&items[0].terms[0]));
            assert!(
                matches!(&items[0].description[0], Block::Paragraph { children, .. } if inline_text(children) == "BODY"),
                "{style}, {head}: {items:#?}"
            );
        }
    }
}

#[test]
fn head_body_relation_classifies_shared_rows() {
    // Expectations classify rows the fixed CVS reference printed for the
    // oracle matrix families (`/tmp/fix/oramatrix` c/f/m rows):
    // - plain hang: `X BODY` shares the row behind one separator;
    // - tag: `X` closes its row and BODY starts the next;
    // - `.fi` tail: `body linetail text` has no separator cell
    //   (roff_term.c:75-78 TERMP_NOSPACE);
    // - filled cleared field: `afterwardstail text` starts at the
    //   description column (term.c:250-253 with 205-207).
    for (body, expected) in [
        (
            ".Bl -hang -width 4n\n.It X\n.No BODY\n.El\n",
            (
                HeadBodyRelation::from(true),
                mant_ir::DefinitionBodyAlignment::Indented,
            ),
        ),
        (
            ".Bl -tag -width 4n\n.It X\n.No BODY\n.El\n",
            // A short tag fits the field: the reference keeps `X     BODY`
            // on one row, like hang.
            (
                HeadBodyRelation::from(true),
                mant_ir::DefinitionBodyAlignment::Indented,
            ),
        ),
        (
            ".Bl -tag -width 4n\n.It plain head\n.No BODY\n.El\n",
            // The reference closes the over-long tag row: `plain head`
            // then BODY at the description column.
            (
                HeadBodyRelation::Separate,
                mant_ir::DefinitionBodyAlignment::Indented,
            ),
        ),
        (
            // This exact source was rechecked with pristine UTF-8/ASCII:
            // it prints `body linetail text` at the description origin.
            // pre_br moves the empty HANG field before the no-fill words
            // arrive, and the later HEAD post keeps its device row open
            // (roff_term.c:69-78; term.c:250-253). The shared old-state
            // receipt therefore classifies FlushAtBody, not a zero-origin
            // concatenation inferred only from the visible word seam.
            ".Bl -hang -width 4n\n.It Xo\n.nf\n.No body line\n.fi\n.Xc\n.No tail text\n.El\n",
            (
                HeadBodyRelation::joined(),
                mant_ir::DefinitionBodyAlignment::Indented,
            ),
        ),
        (
            // c family: the control cleared NOBREAK and the field word
            // overran the width, so the body starts at the description
            // column; the reference prints `     aftertail text`.
            ".Bl -hang -width 2n\n.It Xo\n.sp\n.No after\n.Xc\n.No tail text\n.El\n",
            (
                HeadBodyRelation::joined(),
                mant_ir::DefinitionBodyAlignment::Indented,
            ),
        ),
    ] {
        let item = definition_item_from_source(body);
        assert_eq!(
            (item.head_body_relation, item.layout.body_alignment),
            expected,
            "{item:#?}"
        );
    }
}
