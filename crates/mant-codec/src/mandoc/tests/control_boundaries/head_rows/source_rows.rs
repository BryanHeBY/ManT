use super::*;

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
    let item = definition_item_from_source(
        ".Bl -tag -width 4n\n.It Xo\n.sp\n.No after space\n.Xc\n.No tail text\n.El\n",
    );
    // A repeated exact reference run also verifies the count: an empty
    // buffer makes term_newln() a no-op (term.c:475-480), so there is one
    // initial vertical row, not a second conditional close to project.
    assert_eq!(inline_text(&item.terms[0]), "\nafter\nspace", "{item:#?}");
}

#[test]
fn no_fill_head_projects_each_buffered_word_end_row_once() {
    // Both exact sources passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::ESCAPE_BREAK writes a newline into the native buffer; each
    // later NODE_LINE invokes term_newln(), even after an earlier row ended.
    for (breaks, expected) in [(1, "X\n\nY"), (2, "X\n\n\nY")] {
        let item = definition_item_from_source(&format!(
            ".nf\n.Bl -ohang\n.It Xo\n.No X\n{}.No Y\n.Xc\n.No BODY\n.El\n",
            ".No \\p\n".repeat(breaks)
        ));
        assert_eq!(inline_text(&item.terms[0]), expected, "{item:#?}");
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
        assert!(!item.inline_term(), "{request}: {item:#?}");
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
        assert!(!items[0].inline_term(), "{style} {request}: {items:#?}");
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
    let item = definition_item_from_source(".Bl -inset\n.It Xo X\n.Xc\n.br\n.No BODY\n.El\n");
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
