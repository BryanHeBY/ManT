use super::*;

#[test]
fn inset_mid_word_marker_wipes_body_first_text() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. The inset
    // HEAD field flushes late (at the BODY word's NODE_LINE); its buffer
    // still holds the marker suffix, the generated separator, and the
    // BODY text, and the rejected pass wipes that whole buffer
    // (term.c:144-146 with 235): only the accepted prefix prints.
    let item = review_definition_item(
        ".Bl -inset\n.It Xo\n.No \"alpha \\p beta\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "alpha", "{item:#?}");
    assert!(
        item.description.is_empty(),
        "the shared buffer's body text never prints: {item:#?}"
    );
}

#[test]
fn inset_empty_text_after_marker_closes_its_row() {
    // The exact sources passed fixed CVS -Tascii/-Tutf8/-Tlint. The
    // generated inset separator is a non-breaking space (term.c:347-350),
    // so an armed trailing \p alone keeps the field row open for the
    // joining BODY; a separate empty TEXT runs term_newln() mid-HEAD
    // (NODE_LINE), and a field without NOBREAK or HANG closes there
    // (term.c:250-252). DIAG keeps NOBREAK and its shared row.
    let inset = review_definition_item(
        ".Bl -inset\n.It Xo\n.No alpha\\p\n.No \"\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&inset.terms[0]), "alpha", "{inset:#?}");
    assert!(
        !inset.layout.inline_term(),
        "inset body must start its own row: {inset:#?}"
    );
    let diag = review_definition_item(
        ".Bl -diag\n.It Xo\n.No alpha\\p\n.No \"\"\n.Xc\n.No tail text\n.El\n",
    );
    assert!(
        diag.layout.inline_term(),
        "diag NOBREAK keeps the shared row: {diag:#?}"
    );
    let armed_only =
        review_definition_item(".Bl -inset\n.It Xo\n.No alpha\\p\n.Xc\n.No tail text\n.El\n");
    assert!(
        armed_only.layout.inline_term(),
        r"a trailing \p without the empty TEXT keeps the shared row: {armed_only:#?}"
    );
}

#[test]
fn inset_separate_marker_text_wipes_body_first_text() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. With the
    // marker in its own TEXT, the following word's separator meets the
    // armed field before any new graph: term.c:287-299 with 349-360 make
    // that pass reject, and the buffer wipe (term.c:144-146 with 235)
    // discards the suffix together with the run-in BODY text that still
    // shared the buffer. This latches at the `word()` accounting site,
    // unlike the single-TEXT case above.
    let item = review_definition_item(
        ".Bl -inset\n.It Xo\n.No one\n.No \\p\n.No two\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "one", "{item:#?}");
    assert!(
        item.description.is_empty(),
        "the shared buffer's body text never prints: {item:#?}"
    );
}

#[test]
fn diag_literal_head_marker_wipes_field_suffix() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. Under
    // -diag the It HEAD does not parse extension blocks, so `.It Xo`
    // keeps the literal word "Xo" as its head (mdoc_macro.c:1112-1119)
    // and the extension's BODY words execute inside the still-open
    // NOBREAK field that termp_it_pre() configured before the HEAD
    // printed (mdoc_term.c:827-831) and that only the item's BODY post
    // term_newln() closes (mdoc_term.c:939-945). The marker's rejected
    // pass therefore wipes the unprinted suffix (term.c:144-146 with
    // 235): the accepted prefix prints on the head row and the joining
    // body text never does. CVS output: one row `Xo  alpha`.
    let item = review_definition_item(
        ".Bl -diag\n.It Xo\n.No \"alpha \\p beta\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "Xo", "{item:#?}");
    assert!(
        item.layout.inline_term(),
        "diag NOBREAK keeps the head row: {item:#?}"
    );
    let [Block::Paragraph { children, .. }] = &item.description[..] else {
        panic!("expected one run-in paragraph: {item:#?}");
    };
    assert_eq!(inline_text(children), "  alpha", "{item:#?}");
}

#[test]
fn empty_head_sp_keeps_its_blank_rows() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. An empty
    // HEAD field still executes term_vspace(): term.c:486-498 runs one
    // conditional term_newln() and then one unconditional endline per
    // requested row, so `.sp` contributes a blank row before the head's
    // remaining words. The request also cleared TERMP_NOBREAK
    // (roff_term.c:71-78); the head post's term_flushln() therefore wraps
    // the remaining head words at the field's own vfield (term.c:134-136,
    // reference: `after`/`space` rows at the list offset), while the
    // restored head margins keep tag's final row closed for BODY.
    let item = review_definition_item(
        ".Bl -tag -width 4n\n.It Xo\n.sp\n.No after space\n.Xc\n.No tail text\n.El\n",
    );
    let term = inline_text(&item.terms[0]);
    assert!(
        term.starts_with("\n\n") && term.contains("after\nspace"),
        "the requested blank row must precede the wrapped head words: {item:#?}"
    );
}

#[test]
fn cleared_no_break_field_wraps_hang_head_words_at_the_field_width() {
    // The exact source passed fixed CVS -Tascii. The `.sp` ran
    // term_vspace() then roff_term_pre_br(), which cleared TERMP_NOBREAK
    // (roff_term.c:71-78); the roff node returns before any flag restore
    // (mdoc_term.c:394-396), so the HEAD post flush fills the remaining
    // head words with vtarget=vfield (term.c:134-136): `after` and
    // `space` take separate rows at the list offset, and HANG keeps the
    // last row open for its body (term.c:250-253).
    let item = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.sp\n.No after space\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "\n\nafter\nspace", "{item:#?}");
    assert!(
        item.layout.inline_term(),
        "hang body stays on the last wrapped head row: {item:#?}"
    );
}

#[test]
fn no_fill_head_words_never_wrap_at_the_field_width() {
    // The exact source passed fixed CVS -Tascii. Even after `.nf` cleared
    // TERMP_NOBREAK through the shared roff_term_pre_br() dispatch
    // (roff_term.c:45-58), its NODE_NOFILL subtree prints under
    // TERMP_BRNEVER (mdoc_term.c:314-318): term_fill() runs with an
    // infinite target (term.c:143-144), so `after space` stays on one row
    // and only the BODY column follows.
    let item = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.nf\n.No after space\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "after space", "{item:#?}");
    assert!(
        !item.layout.inline_term(),
        "the fill-mode boundary closed the head row before BODY: {item:#?}"
    );
}

#[test]
fn tag_marker_split_head_closes_its_final_row() {
    // The exact sources passed fixed CVS -Tascii/-Tutf8/-Tlint. In-word
    // \p markers already flushed term_fill() passes, so the term_flushln()
    // tail rule (term.c:250-252) closes a NOBREAK-without-HANG (tag) row at
    // the final pass: BODY starts its own row even without a trailing
    // break. A HANG field keeps the shared row for its body.
    let tag = review_definition_item(
        ".Bl -tag -width 4n\n.It Xo\n.No \"x\\p y\\p z\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&tag.terms[0]), "x\ny\nz", "{tag:#?}");
    assert!(
        !tag.layout.inline_term(),
        "tag body must start its own row: {tag:#?}"
    );
    let hang = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.No \"x\\p y\\p z\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&hang.terms[0]), "x\ny\nz", "{hang:#?}");
    assert!(
        hang.layout.inline_term(),
        "hang body stays on the last row: {hang:#?}"
    );
}

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
        let item = review_definition_item(source);
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
fn no_fill_head_projects_each_buffered_word_end_row_once() {
    // Both exact sources passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::ESCAPE_BREAK writes a newline into the native buffer; each
    // later NODE_LINE invokes term_newln(), even after an earlier row ended.
    for (breaks, expected) in [(1, "X\n\nY"), (2, "X\n\n\nY")] {
        let item = review_definition_item(&format!(
            ".nf\n.Bl -ohang\n.It Xo\n.No X\n{}.No Y\n.Xc\n.No BODY\n.El\n",
            ".No \\p\n".repeat(breaks)
        ));
        assert_eq!(inline_text(&item.terms[0]), expected, "{item:#?}");
    }
}

#[test]
fn literal_definition_body_shares_only_a_continued_head_row() {
    // Exact positive and negative inputs passed fixed CVS -Tascii/-Tutf8/
    // -Tlint. mdoc_term.c enters no-fill BODY at NODE_LINE; TERMP_NONEWLINE
    // from a final \c suppresses that row break and survives the IR drain.
    for (head, joins) in [(".No X\\c", true), (".No X", false)] {
        let item = review_definition_item(&format!(
            ".nf\n.Bl -hang -width 4n\n.It Xo\n{head}\n.Xc\n.No BODY\n.El\n"
        ));
        assert!(matches!(
            item.description.first(),
            Some(Block::Preformatted { .. })
        ));
        assert_eq!(item.layout.inline_term(), joins, "{head}: {item:#?}");
        assert_eq!(
            item.inline_description().is_some(),
            joins,
            "{head}: {item:#?}"
        );
    }
}

#[test]
fn rejected_hang_link_field_does_not_reintroduce_a_wrapped_suffix() {
    // Exact HANG/TAG sources passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_fill() returns nbr=0 when a \p is followed by the next
    // word's ordinary separator before any graph in that pass. A typed Lk
    // wrapper cannot turn the rejected suffix into a committed prefix.
    for style in ["hang", "tag"] {
        let item = review_definition_item(&format!(
            ".Bl -{style} -width 4n\n.It Xo\n.Lk https://example.com \\p QAXAQ\n.Xc\n.No BODY\n.El\n"
        ));
        assert!(
            !inline_text(&item.terms[0]).contains("QAXAQ"),
            "{style}: {item:#?}"
        );
        let Block::Paragraph { children, .. } = &item.description[0] else {
            panic!("expected BODY paragraph: {item:#?}");
        };
        assert_eq!(inline_text(children), "BODY");
    }
}

#[test]
fn diagnostic_xo_spelling_is_not_an_explicit_definition_head_scope() {
    // Exact -diag and -inset inputs passed fixed CVS -Ttree/-Tascii/-Tutf8/
    // -Tlint. mdoc_macro.c::blk_exp_close() breaks an intermediate It only
    // for an actual explicit block; -diag parses this Xo as literal TEXT.
    fn first_it_head(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
        if node.kind == libmandoc_rs::NodeKind::Block && node.macro_name.as_deref() == Some("It") {
            return node
                .children
                .iter()
                .find(|child| child.kind == libmandoc_rs::NodeKind::Head);
        }
        node.children.iter().find_map(first_it_head)
    }
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (list, head, expected_kind) in [
        ("diag", ".It Xo\n", libmandoc_rs::NodeKind::Text),
        (
            "inset",
            ".It Xo first\n.Xc\n",
            libmandoc_rs::NodeKind::Block,
        ),
    ] {
        let source = format!("{prefix}.Bl -{list}\n{head}.No BODY\n.El\n");
        let report = Parser::default()
            .parse_bytes("definition-head.1", source.as_bytes())
            .unwrap();
        let head = first_it_head(&report.document.root).expect("It HEAD");
        assert_eq!(head.children[0].kind, expected_kind, "{list}: {head:#?}");
        assert_eq!(
            head.children[0].flags.broken,
            list == "inset",
            "mdoc_macro.c::blk_exp_close() preserves native close evidence: {head:#?}"
        );
    }
}

#[test]
fn inset_head_drain_keeps_native_word_separator_state() {
    // Exact variants checked with fixed CVS -Tascii/-Tlint. In
    // mdoc_term.c::termp_it_pre(), the inset BODY executes term_word("\\ ")
    // even when a pending HEAD glyph consumed that cell. term.c::term_word()
    // then consumes NOSPACE for an empty BODY word before the visible word.
    for (middle, expected_body) in [
        (".No \"\"\n", " BODY"),
        (".No \\&\n", " BODY"),
        (".No \\fB\n", " BODY"),
        ("", "BODY"),
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
        assert_eq!(inline_text(&items[0].terms[0]), "X", "{middle}: {items:#?}");
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
    // Exact forms checked with fixed CVS -Tascii/-Tlint. A completed \zX
    // belongs to the HEAD row. An empty or font-only BODY word adds no new
    // cell, while \& adds a genuine invisible BODY cell and its own row.
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
        assert_eq!(inline_text(&items[0].terms[0]), "X", "{items:#?}");
        let Block::Preformatted { children, .. } = &items[0].description[0] else {
            panic!("{body_word}: {items:#?}");
        };
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
fn final_definition_head_break_moves_once_into_stacked_layout() {
    // Exact br, sp 0, and sp 1 forms checked with fixed CVS -Tascii/-Tlint.
    // roff_term.c::roff_term_pre_br() closes the HEAD row; termp_it_pre()
    // cannot turn that completed row back into a run-in BODY field.
    for (request, expected_term) in [(".br", "X"), (".sp 0", "X"), (".sp 1", "X\n")] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -inset\n.It Xo X\n{request}\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("closed-definition-head.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{request}: {document:#?}");
        };
        let item = &items[0];
        assert_eq!(inline_text(&item.terms[0]), expected_term, "{item:#?}");
        assert!(!item.layout.inline_term(), "{request}: {item:#?}");
        assert!(
            matches!(&item.description[0], Block::Paragraph { children, .. } if inline_text(children) == " BODY"),
            "{request}: {item:#?}"
        );
    }
}

#[test]
fn completed_definition_head_and_body_rows_have_distinct_owners() {
    // Both exact inputs passed fixed CVS -Tascii/-Tutf8/-Tlint. CVS
    // mdoc_term.c::termp_it_pre() creates the inset BODY separator only after
    // the HEAD has closed; the later roff_term.c::roff_term_pre_br() closes
    // that new row, not the already represented HEAD row.
    for (head_request, expected_term) in [(".br", "X"), (".sp 1", "X\n")] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -inset\n.It Xo X\n{head_request}\n.Xc\n.br\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("closed-head-new-body-row.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{head_request}: {document:#?}");
        };
        assert_eq!(inline_text(&items[0].terms[0]), expected_term);
        assert!(
            matches!(&items[0].description[0], Block::Paragraph { children, .. } if inline_text(children) == " \nBODY"),
            "{head_request}: {items:#?}"
        );
    }
}

#[test]
fn detached_definition_head_keeps_authored_vertical_rows() {
    // Exact hang/tag/ohang .sp 1/.sp 2 cases passed fixed CVS
    // -Tascii/-Tutf8/-Tlint. term.c::term_vspace() has already emitted its
    // row before the detached HEAD returns to mdoc_term.c::termp_it_post().
    for (style, request, expected_term) in [
        ("hang -width 4n", ".sp 1", "X"),
        ("hang -width 4n", ".sp 2", "X\n"),
        ("tag -width 4n", ".sp 2", "X\n"),
        ("ohang", ".sp 1", "X\n"),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -{style}\n.It Xo X\n{request}\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("detached-head-vertical-rows.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{style} {request}: {document:#?}");
        };
        assert_eq!(inline_text(&items[0].terms[0]), expected_term);
        assert!(
            !items[0].layout.inline_term(),
            "{style} {request}: {items:#?}"
        );
    }
}

#[test]
fn colon_breakpoint_wraps_the_head_row_like_the_reference() {
    // Fixed CVS -Tascii: an overrunning operand breaks at its buffered
    // ASCII_BREAK cell (term.c:287-300) and the remainder wraps, while a
    // fitting operand stays one row and joins BODY.
    // Width sweep against fixed CVS -Tascii (all rows verified): the
    // buffered ASCII_BREAK ends the row only when the pass truncates or
    // breaks at it (term.c:294-295, 362-366) — the ZcWrapTrace probe pins
    // both shapes; a fitting operand stays one row whatever sits inside.
    for (words, expected_rows) in [
        // `X YYYYY` / `Z     BODY`: the tail truncated at the breakpoint.
        (r"YYYYY\:Z", 2),
        // `X YYYY` / `ZZ    BODY`: the overrun broke at the recorded
        // word-end candidate (term.c:350-351 with 296-299).
        (r"YYYY\:ZZ", 2),
        // `X Y` / `ZZZZ  BODY`: the four-column tail overruns.
        (r"Y\:ZZZZ", 2),
        // Fitting operands keep one row and BODY concatenates:
        // `X YYZZBODY`, `X YYYZBODY`.
        (r"YY\:ZZ", 1),
        (r"YYY\:Z", 1),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No {words}\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("colon-breakpoint-rows.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{words}: {document:#?}");
        };
        let row_breaks = items[0].terms[0]
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak { .. }))
            .count();
        assert_eq!(row_breaks + 1, expected_rows, "{words}: {items:#?}");
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
            HeadBodyRelation::RunIn,
        ),
        (
            ".Bl -tag -width 4n\n.It X\n.No BODY\n.El\n",
            // A short tag fits the field: the reference keeps `X     BODY`
            // on one row, like hang.
            HeadBodyRelation::RunIn,
        ),
        (
            ".Bl -tag -width 4n\n.It plain head\n.No BODY\n.El\n",
            // The reference closes the over-long tag row: `plain head`
            // then BODY at the description column.
            HeadBodyRelation::Separate,
        ),
        (
            // f3 family: `.fi` arms TERMP_NOSPACE inside the still-open
            // head, so the body's first word concatenates onto it; the
            // reference prints `body linetail text` as one row.
            ".Bl -hang -width 4n\n.It Xo\n.nf\n.No body line\n.fi\n.Xc\n.No tail text\n.El\n",
            HeadBodyRelation::JoinedNoSpace,
        ),
        (
            // c family: the control cleared NOBREAK and the field word
            // overran the width, so the body starts at the description
            // column; the reference prints `     aftertail text`.
            ".Bl -hang -width 2n\n.It Xo\n.sp\n.No after\n.Xc\n.No tail text\n.El\n",
            HeadBodyRelation::FlushAtBody,
        ),
    ] {
        let item = review_definition_item(body);
        assert_eq!(item.layout.head_body_relation, expected, "{item:#?}");
    }
}

#[test]
fn semantic_link_wrapper_does_not_own_a_closed_no_fill_row() {
    // All exact UR/MT and font/word-end variants were checked with fixed
    // CVS -Tascii/-Tlint. roff_term_pre_br() calls term_newln() at fi; a
    // semantic link annotation cannot create another physical empty row.
    for (open, close, target) in [
        ("UR", "UE", "https://example.org"),
        ("MT", "ME", "user@example.org"),
    ] {
        for middle in [
            ".nf\nlabel\n.fi",
            ".nf\nlabel\\p\n.fi",
            ".nf\nlabel\\p\n.ft B\n.fi",
            ".nf\nlabel\n.fi\n.ft B",
        ] {
            let source = format!(
                ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.{open} {target}\n{middle}\n.{close}\nafter\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("link-closed-no-fill-row.1"),
                source.as_bytes(),
            )
            .unwrap();
            let blocks = &document.sections[0].blocks;
            let literal = blocks.iter().find_map(|block| match block {
                Block::Preformatted { children, .. } => Some(children),
                _ => None,
            });
            let literal = literal.expect("label row");
            assert_eq!(
                inline_text(literal),
                "label",
                "{open}/{middle}: {document:#?}"
            );
            assert!(
                blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains(&format!("⟨{target}⟩ after")))),
                "{open}/{middle}: {document:#?}"
            );
        }
    }
}

#[test]
fn synopsis_head_consumes_pending_zero_advance_before_body() {
    // Both exact sources checked with fixed CVS -Tascii/-Tlint. The common
    // print_man_node()/print_mdoc_node() traversal executes HEAD before BODY
    // against the same termp zero-width state.
    let man = b".TH TEST 1\n.SH SYNOPSIS\n\\z\n.SY call\narg\n.YS\n";
    let document = parse_manual_bytes(std::path::Path::new("sy-head-zero.1"), man).unwrap();
    let text = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<String>();
    assert!(text.trim_start().starts_with("all arg"), "{document:#?}");

    let mdoc = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh SYNOPSIS\n\\z\n.Nm call\n.Ar arg\n";
    let document = parse_manual_bytes(std::path::Path::new("nm-head-zero.1"), mdoc).unwrap();
    let text = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<String>();
    assert!(text.trim_start().starts_with("all"), "{document:#?}");
    assert!(!text.trim_start().starts_with("call"), "{document:#?}");
    assert!(text.contains("arg"), "{document:#?}");

    // man_term.c::print_man_node() resets fonts at each SY BLOCK/HEAD/BODY
    // boundary. Its head alone is bold; a preceding .ft B cannot style BODY.
    let man = b".TH TEST 1\n.SH SYNOPSIS\n.ft B\n.SY call\narg\n.YS\nafter\n\\fPprevious\n";
    let document = parse_manual_bytes(std::path::Path::new("sy-head-font.1"), man).unwrap();
    let strong = strong_document_text(&document);
    assert!(strong.contains("call"), "{document:#?}");
    assert!(!strong.contains("arg"), "{document:#?}");
    assert!(!strong.contains("after"), "{document:#?}");
    assert!(!strong.contains("previous"), "{document:#?}");

    let man = b".TH TEST 1\n.SH SYNOPSIS\n.SY \\fIcall\n\\fParg\n.YS\n";
    let document =
        parse_manual_bytes(std::path::Path::new("sy-head-previous-font.1"), man).unwrap();
    // Exact input checked with fixed CVS -Tascii/-Tlint. The HEAD post and
    // BODY entry both run man_term.c's Roman term_fontrepl() transition.
    assert!(
        !strong_document_text(&document).contains("arg"),
        "{document:#?}"
    );
    assert!(
        !emphasized_document_text(&document).contains("arg"),
        "{document:#?}"
    );

    // The no-fill SY path must use the same HEAD execution. Exact source
    // checked with fixed CVS -Tascii/-Thtml/-Tlint; BODY's arg stays intact.
    let man = b".TH TEST 1\n.SH SYNOPSIS\n\\z\n.SY call\n.nf\narg\n.YS\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("sy-head-nofill-zero.1"), man).unwrap();
    assert!(document.sections[0].blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("arg"))), "{document:#?}");

    // HEAD post is a real formatter flush in both macros when BODY exists.
    // Fixed CVS -Tascii retains X from a trailing \zX on the head row.
    for (name, source) in [
        (
            "sy",
            b".TH TEST 1\n.SH SYNOPSIS\n.SY call\\zX\narg\n.YS\n".as_slice(),
        ),
        (
            "nm",
            b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh SYNOPSIS\n.Nm call\\zX\n.Ar arg\n"
                .as_slice(),
        ),
    ] {
        let document = parse_manual_bytes(std::path::Path::new(name), source).unwrap();
        let text = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    Some(inline_text(children))
                }
                _ => None,
            })
            .collect::<String>();
        assert!(text.contains('X'), "{name}: {document:#?}");
        assert!(text.contains("arg"), "{name}: {document:#?}");
    }
}

#[test]
fn definition_heads_execute_no_fill_source_rows() {
    // Both exact inputs checked with fixed CVS -Tascii/-Thtml/-Tlint.
    // mdoc_term.c::print_mdoc_node() applies NODE_NOFILL/NODE_LINE to each
    // Xo child and Fo/Fa event before the It HEAD's field is laid out.
    for (name, head, expected) in [
        ("xo", ".It Xo\nfirst\nsecond\n.Xc", "first\nsecond"),
        (
            "fo",
            ".It Fo call\n.Fa first\n.Fa second\n.Fc",
            "call(\nfirst,\nsecond)",
        ),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Bl -tag -width xxx\n{head}\nbody\n.El\n.fi\n"
        );
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes()).unwrap();
        let terms = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::DefinitionList { items, .. } => items.first().map(|item| &item.terms),
                _ => None,
            })
            .expect("definition term");
        assert!(
            terms
                .iter()
                .any(|term| inline_text(term).contains(expected)),
            "{name}: {document:#?}"
        );
    }
}

#[test]
fn definition_head_rows_follow_executed_flags_after_nf_and_macro_expansion() {
    // Both exact inputs checked with fixed CVS -Tascii/-Thtml/-Tlint.
    // mdoc_term.c::print_mdoc_node() handles each NODE_LINE/NODE_NOFILL
    // entry, including two expanded words that share one source coordinate.
    for (name, source) in [
        (
            "head-enters-no-fill.1",
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width xxx\n.It Xo\n.nf\nfirst\nsecond\n.fi\n.Xc\nbody\n.El\n",
        ),
        (
            "head-expanded-rows.1",
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.de XX\nfirst\nsecond\n..\n.nf\n.Bl -tag -width xxx\n.It Xo\n.XX\n.Xc\nbody\n.El\n.fi\n",
        ),
    ] {
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes()).unwrap();
        let terms = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::DefinitionList { items, .. } => items.first().map(|item| &item.terms),
                _ => None,
            })
            .expect("definition term");
        assert!(
            terms
                .iter()
                .any(|term| inline_text(term).contains("first\nsecond")),
            "{name}: {document:#?}"
        );
    }
}

#[test]
fn definition_head_anchor_does_not_shift_explicit_term_separator() {
    // Exact input checked with fixed CVS -Tascii/-Thtml/-Tlint. The .Tg
    // anchor is attached to It HEAD, while mdoc_term.c still executes .Pp
    // between first and second as a separate native term paragraph.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width xxx\n.It Xo\n.Tg sample\nfirst\n.Pp\nsecond\n.Xc\nbody\n.El\n";
    let document =
        parse_manual_bytes(std::path::Path::new("definition-anchor-pp.1"), source).unwrap();
    let item = document.sections[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => items.first(),
            _ => None,
        })
        .expect("definition item");
    assert_eq!(item.terms.len(), 2, "{item:#?}");
    assert_eq!(inline_text(&item.terms[0]), "first");
    assert_eq!(inline_text(&item.terms[1]), "second");

    // Consecutive Pp requests may reuse one projected LineBreak when an
    // intervening bare \z has not occupied a formatter cell. The later Pp
    // must still split the following term. Exact source checked with fixed
    // CVS -Tascii/-Thtml/-Tlint; mdoc_term.c executes each Pp in order.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width xxx\n.It Xo\nfirst\n.Pp\n\\z\n.Pp\nsecond\n.Pp\nthird\n.Xc\nbody\n.El\n";
    let document =
        parse_manual_bytes(std::path::Path::new("definition-repeated-pp.1"), source).unwrap();
    let item = document.sections[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => items.first(),
            _ => None,
        })
        .expect("definition item");
    assert_eq!(item.terms.len(), 3, "{item:#?}");
    assert_eq!(inline_text(&item.terms[0]), "first");
    assert!(inline_text(&item.terms[1]).contains("cond"), "{item:#?}");
    assert_eq!(inline_text(&item.terms[2]), "third");
}

#[test]
fn function_head_target_stays_with_the_active_output_channel() {
    struct Anchors(Vec<(String, Option<u32>)>);
    impl<'ir> Visit<'ir> for Anchors {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor {
                id, owner_source, ..
            } = inline
            {
                self.0
                    .push((id.as_str().to_owned(), owner_source.map(|span| span.line)));
            }
            visit::walk_inline(self, inline);
        }
    }
    // Each exact input was checked with pinned CVS -Ttree/-Thtml. The
    // mdoc_validate.c::post_tg() target belongs to Fo HEAD; mdoc_html.c
    // writes its id on the visible Fn in filled and no-fill output, even
    // when a structural child interrupts the BODY.
    for (name, source, preformatted, head_line) in [
        (
            "filled",
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Tg call\n.Fo call\n.Fa first\n.Fa second\n.Fc\n",
            false,
            6,
        ),
        (
            "nofill",
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Tg call\n.Fo call\n.Fa first\n.Fa second\n.Fc\n.fi\n",
            true,
            7,
        ),
        (
            "structural",
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Tg call\n.Fo call\n.Fa first\n.Bl -bullet\n.It\nitem\n.El\n.Fa second\n.Fc\n",
            false,
            6,
        ),
    ] {
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes()).unwrap();
        let blocks = &document.sections[0].blocks;
        let paired = blocks.iter().any(|block| {
            let children = match block {
                Block::Paragraph { children, .. } if !preformatted => children,
                Block::Preformatted { children, .. } if preformatted => children,
                _ => return false,
            };
            children
                .iter()
                .any(|child| matches!(child, Inline::Anchor { id, .. } if id.as_str() == "call"))
                && inline_text(children).contains("call(")
        });
        assert!(paired, "{name}: {blocks:#?}");
        let mut anchors = Anchors(Vec::new());
        anchors.visit_document(&document);
        assert_eq!(
            anchors.0.iter().filter(|(id, _)| id == "call").count(),
            1,
            "{name}: {:#?}",
            anchors.0
        );
        assert!(
            anchors.0.contains(&(String::from("call"), Some(head_line))),
            "{name}: {:#?}",
            anchors.0
        );
    }
}

#[test]
fn mdoc_empty_function_head_still_emits_body_punctuation() {
    // Exact input checked against fixed CVS tree, HTML and UTF-8 output.
    // mdoc_html.c::mdoc_fo_pre/post emit parentheses for the BODY even when
    // the HEAD has no child; terminal omits the HTML-only prose semicolon.
    let source =
        b".Dd September 28, 2026\n.Dt EMPTYFO 1\n.Os\n.Sh DESCRIPTION\n.Fo\n.Fa x\n.Fc\nnext\n";
    let document = parse_manual_bytes(std::path::Path::new("empty-fo.1"), source)
        .expect("lower empty-head function");
    assert_eq!(projected_document_text(&document), "DESCRIPTION(x) next");
}

#[test]
fn bibliography_wrapper_uses_the_active_filled_or_no_fill_text_execution() {
    for (label, mode) in [("filled", ""), ("no-fill", ".nf\n")] {
        // Exact inputs checked against pinned CVS -Tascii/-Tlint.  Rs has
        // no terminal post break or punctuation of its own: mdoc_term.c
        // termp_rs_pre() only adds spacing in SEE ALSO. The following Y
        // overstrikes the pending \zX in the same termp row even across Re.
        let manual = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh DESCRIPTION\n{mode}.Rs\n\\zX\\c\n.Re\nY\n{}",
            if mode.is_empty() { "" } else { ".fi\n" },
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("mdoc-rs-active-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse reference wrapper fixture");
        let [block] = document.sections[1].blocks.as_slice() else {
            panic!("{label}: Rs split execution: {:#?}", document.sections);
        };
        match (mode.is_empty(), block) {
            (true, Block::Paragraph { children, .. })
            | (false, Block::Preformatted { children, .. }) => {
                assert_eq!(inline_text(children), "Y", "{label}: {children:?}");
            }
            _ => panic!("{label}: wrong output channel: {block:?}"),
        }
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

#[test]
fn fill_mode_closed_head_scope_does_not_add_a_row() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. NODE_BROKEN
    // is parser state the terminal renderer never reads (no reference in
    // mdoc_term.c or term.c): the body `.br` closes the head row once
    // (roff_term_pre_br term_newln) and fill mode joins the open row
    // (mdoc.c:238-250). Only no-fill mode runs another term_newln() at the
    // next NODE_LINE (mdoc_term.c:314-317), which is where the closed-scope
    // extra row comes from.
    let item = review_definition_item(".Bl -inset\n.It Xo X\n.Xc\n.br\n.No BODY\n.El\n");
    assert_eq!(inline_text(&item.terms[0]), "X", "{item:#?}");
    let blank_rows: u16 = item
        .description
        .iter()
        .filter_map(|block| match block {
            Block::VerticalSpace { lines, .. } => Some(*lines),
            _ => None,
        })
        .sum();
    assert_eq!(blank_rows, 0, "fill mode adds no row: {item:#?}");
}
