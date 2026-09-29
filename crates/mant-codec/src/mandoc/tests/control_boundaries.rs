use super::*;

fn review_definition_item(body: &str) -> mant_ir::DefinitionItem {
    let source = format!(
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n{body}"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("definition-physical-row-review.1"),
        source.as_bytes(),
    )
    .expect("lower reviewed definition source");
    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("expected definition list: {document:#?}");
    };
    items[0].clone()
}

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
        !inset.layout.inline_term,
        "inset body must start its own row: {inset:#?}"
    );
    let diag = review_definition_item(
        ".Bl -diag\n.It Xo\n.No alpha\\p\n.No \"\"\n.Xc\n.No tail text\n.El\n",
    );
    assert!(
        diag.layout.inline_term,
        "diag NOBREAK keeps the shared row: {diag:#?}"
    );
    let armed_only =
        review_definition_item(".Bl -inset\n.It Xo\n.No alpha\\p\n.Xc\n.No tail text\n.El\n");
    assert!(
        armed_only.layout.inline_term,
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
fn empty_head_sp_keeps_its_blank_rows() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. An empty
    // HEAD field still executes term_vspace(): term.c:486-498 runs one
    // conditional term_newln() and then one unconditional endline per
    // requested row, so `.sp` contributes a blank row before the head's
    // remaining words. The post-request row-width wrap (NOBREAK cleared)
    // remains a separate open behavior and is not pinned here.
    let item = review_definition_item(
        ".Bl -tag -width 4n\n.It Xo\n.sp\n.No after space\n.Xc\n.No tail text\n.El\n",
    );
    let term = inline_text(&item.terms[0]);
    assert!(
        term.starts_with("\n\n") && term.contains("after space"),
        "the requested blank row must precede the head words: {item:#?}"
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
        !tag.layout.inline_term,
        "tag body must start its own row: {tag:#?}"
    );
    let hang = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.No \"x\\p y\\p z\"\n.Xc\n.No tail text\n.El\n",
    );
    assert_eq!(inline_text(&hang.terms[0]), "x\ny\nz", "{hang:#?}");
    assert!(
        hang.layout.inline_term,
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
fn hang_field_flush_preserves_native_trailspace_without_reusing_br_gap() {
    // Both exact inputs passed fixed CVS -Tascii/-Tutf8/-Tlint. term.c's
    // term_flushln() restores minbl=trailspace even when nbr=0; explicit
    // roff_term_pre_br() clears BRIND, while NODE_LINE sets NOSPACE.
    for (middle, expected) in [(".No \\p", "X Y"), (".br\n.No \"\"", "XY")] {
        let item = review_definition_item(&format!(
            ".nf\n.Bl -hang -width 4n\n.It Xo\n.No X\n{middle}\n.No Y\n.Xc\n.No BODY\n.El\n"
        ));
        assert_eq!(inline_text(&item.terms[0]), expected, "{middle}: {item:#?}");
        if middle == ".No \\p" {
            assert!(
                !item
                    .entry
                    .as_ref()
                    .is_some_and(|entry| entry.names.iter().any(|name| name == "XY")),
                "a lost native separator changed entry identity: {item:#?}"
            );
        }
    }
}

#[test]
fn hang_field_rejects_only_the_unaccepted_word_end_suffix() {
    // Exact single-TEXT and cross-TEXT cases passed fixed CVS
    // -Tascii/-Tutf8/-Tlint. term.c::term_fill() restarts from the remaining
    // buffer after every accepted prefix, resetting graph for each pass.
    for (head, expected) in [
        (".No \"X\\p Y\"\n.No \"\\p Z\"", "X\nY"),
        (".No X\\p\n.No \\p\n.No \"\"\n.No Z", "X"),
        (".No X\n.No \\p\n.No Y", "X"),
        (".No X\\p\n.No \"\"\n.No Y", "X\nY"),
    ] {
        let item = review_definition_item(&format!(
            ".Bl -hang -width 4n\n.It Xo\n{head}\n.Xc\n.No BODY\n.El\n"
        ));
        assert_eq!(inline_text(&item.terms[0]), expected, "{head}: {item:#?}");
        assert!(!inline_text(&item.terms[0]).contains('Z'));
    }
    // The same term_fill() consumption runs for OHANG even though its HEAD
    // does not use HANG geometry. The pinned reference drops Y here too.
    let item =
        review_definition_item(".Bl -ohang\n.It Xo\n.No X\n.No \\p\n.No Y\n.Xc\n.No BODY\n.El\n");
    assert_eq!(inline_text(&item.terms[0]), "X\n", "{item:#?}");
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
        assert_eq!(item.layout.inline_term, joins, "{head}: {item:#?}");
        assert_eq!(
            item.inline_description().is_some(),
            joins,
            "{head}: {item:#?}"
        );
    }
}

#[test]
fn definition_body_uses_continuation_after_generated_words_and_zero_row_requests() {
    // Each exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. In
    // mdoc_term.c::termp_it_pre(), inset BODY's term_word("\\ ") consumes a
    // pending \c before NODE_LINE. In roff_term.c::pre_sp/pre_ce, a zero-row
    // request calls pre_br()/term_newln() without clearing TERMP_NONEWLINE;
    // term.c::term_flushln() can keep a HANG device row open.
    let inset =
        review_definition_item(".nf\n.Bl -inset\n.It Xo\n.No X\\c\n.Xc\n.No BODY\n.El\n.fi\n");
    assert_eq!(inline_text(&inset.terms[0]), "X");
    assert!(!inset.layout.inline_term, "{inset:#?}");
    for request in [".sp 0", ".ce 0", ".rj 0"] {
        let item = review_definition_item(&format!(
            ".nf\n.Bl -hang -width 4n\n.It Xo\n.No X\\c\n{request}\n.Xc\n.No BODY\n.El\n.fi\n"
        ));
        assert!(item.layout.inline_term, "{request}: {item:#?}");
        assert!(item.inline_description().is_some(), "{request}: {item:#?}");
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
fn hang_field_restarts_term_fill_from_the_actual_breakable_blank() {
    // Exact sources passed fixed CVS -Tascii/-Tutf8/-Tlint. term.c::term_fill()
    // records nbr at the first ordinary space, then restarts from that space
    // after \p. With "X \p Y", the next pass has no graph and rejects Y/Z;
    // with "X\p Y", it accepts Y in the next pass.
    for (first, expected, rejected) in [("X \\p Y", "X", true), ("X\\p Y", "X\nY Z", false)] {
        let item = review_definition_item(&format!(
            ".Bl -hang -width 4n\n.It Xo\n.No \"{first}\"\n.No Z\n.Xc\n.No BODY\n.El\n"
        ));
        assert_eq!(inline_text(&item.terms[0]), expected, "{first}: {item:#?}");
        assert_eq!(inline_text(&item.terms[0]).contains('Z'), !rejected);
    }
}

#[test]
fn tag_margin_flush_does_not_print_an_unconsumed_next_field_separator() {
    // This exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. In
    // roff_term.c::roff_term_pre_mc(), TERMP_NOBREAK flushes the TAG field;
    // mdoc_term.c::termp_it_post() then closes HEAD. The ordinary separator
    // for a *later* term_word() has not occupied a second physical row.
    let item = review_definition_item(
        ".nf\n.Bl -tag -width 4n\n.It Xo\n.No QHEADQ\\c\n.mc |\n.Xc\n.No QBODYQ\n.El\n.fi\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "QHEADQ", "{item:#?}");
    assert!(!item.layout.inline_term, "{item:#?}");
    assert!(!matches!(
        item.description.first(),
        Some(Block::VerticalSpace { .. })
    ));
}

#[test]
fn margin_control_flushes_rejected_native_field_before_starting_another() {
    // These exact HANG/TAG sources passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // roff_term.c::roff_term_pre_mc() calls term_flushln() when p->col is
    // occupied; term.c::term_fill() can reject the suffix after \\p there,
    // before `.mc` changes NOBREAK and prepares the following field.
    for style in ["hang", "tag"] {
        let item = review_definition_item(&format!(
            ".Bl -{style} -width 4n\n.It Xo\n.No \"QAA \\p QBB\"\n.mc |\n.Xc\n.No QBODYQ\n.El\n"
        ));
        assert_eq!(
            inline_text(&item.terms[0]).trim_end(),
            "QAA",
            "{style}: {item:#?}"
        );
    }
    let empty = review_definition_item(
        ".Bl -hang -width 4n\n.It Xo\n.No \"\\p QAA\"\n.mc |\n.Xc\n.No QBODYQ\n.El\n",
    );
    assert!(!inline_text(&empty.terms[0]).contains("QAA"), "{empty:#?}");
}

#[test]
fn every_post_margin_field_flush_rejects_the_pending_suffix() {
    // Exact br/sp0/ce0/rj0 sources passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // roff_term.c::pre_br() calls term_newln(), which reaches term_fill()
    // even after a prior `.mc` opened another NOBREAK field. The accepted
    // QBB prefix survives; the later QCC suffix is never printed.
    for request in [".br", ".sp 0", ".ce 0", ".rj 0"] {
        let item = review_definition_item(&format!(
            ".Bl -hang -width 4n\n.It Xo\n.No QAA\n.mc |\n.No \"QBB \\p QCC\"\n{request}\n.Xc\n.No QBODYQ\n.El\n"
        ));
        let term = inline_text(&item.terms[0]);
        assert!(
            term.contains("QAA") && term.contains("QBB"),
            "{request}: {item:#?}"
        );
        assert!(!term.contains("QCC"), "{request}: {item:#?}");
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

fn emphasized_document_text(document: &mant_ir::Document) -> String {
    struct Collector(String);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Emphasis { children } = inline {
                self.0.push_str(&inline_text(children));
            } else {
                visit::walk_inline(self, inline);
            }
        }
    }
    let mut collector = Collector(String::new());
    collector.visit_document(document);
    collector.0
}

fn strong_document_text(document: &mant_ir::Document) -> String {
    struct Collector(String);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Strong { children } = inline {
                self.0.push_str(&inline_text(children));
            } else {
                visit::walk_inline(self, inline);
            }
        }
    }
    let mut collector = Collector(String::new());
    collector.visit_document(document);
    collector.0
}

fn document_link_targets(document: &mant_ir::Document) -> Vec<mant_ir::LinkTarget> {
    struct Collector(Vec<mant_ir::LinkTarget>);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link { target, .. } = inline {
                self.0.push(target.clone());
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut collector = Collector(Vec::new());
    collector.visit_document(document);
    collector.0
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
fn completed_empty_text_rows_survive_filled_output_drains() {
    // Exact combinations checked with fixed CVS -Tascii/-Tlint.
    // man_term.c::print_man_node() sends an empty TEXT to term_vspace();
    // term.c::term_vspace() emits one row independently of later PP or fi/nf.
    // pre_alternate() instead sends BR's empty operands through term_word().
    for (empty, completed_rows) in [
        (".B \"\"", 1),
        (".I \"\"", 1),
        (".B \"\"\n.B \"\"", 2),
        (".B \"\"\n.B \"\" \"\"", 2),
        (".B \"\"\n.B \"\" \"\"\n.B \"\" \"\"", 2),
        (".B \"\"\n.B \"\" \"\"\n.B \"\"", 3),
        (".B \"\"\n\\&\n.B \"\"", 3),
        (".B \"\"\n.BR \"\" \"\"", 1),
        (".B \"\"\n\\&", 2),
    ] {
        for boundary in [".PP", ".fi", ".nf"] {
            let source = format!(
                ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\n{empty}\n{boundary}\nAFTER\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("filled-empty-text-rows.1"),
                source.as_bytes(),
            )
            .unwrap();
            let blocks = &document.sections[0].blocks;
            assert!(
                matches!(&blocks[0], Block::Paragraph { children, .. } if inline_text(children) == "BEFORE"),
                "{empty}, {boundary}: {blocks:#?}"
            );
            assert!(
                matches!(&blocks[1], Block::VerticalSpace { lines, .. } if *lines == completed_rows),
                "{empty}, {boundary}: {blocks:#?}"
            );
            assert!(
                matches!(&blocks[2], Block::Paragraph { children, .. } | Block::Preformatted { children, .. } if inline_text(children) == "AFTER"),
                "{empty}, {boundary}: {blocks:#?}"
            );
            let expected_paragraph_gap = u16::from(boundary == ".PP");
            assert_eq!(
                mant_ir::geometry::block_gap(&blocks[2]),
                expected_paragraph_gap,
                "{empty}, {boundary}: {blocks:#?}"
            );
        }
    }
}

#[test]
fn completed_vertical_rows_do_not_replay_their_invisible_cell() {
    // Exact inputs checked with fixed CVS -Tascii/-Tlint. term.c::term_vspace()
    // completes the empty TEXT row; a subsequent \& occupies just one more
    // row, which term_newln() closes at the next mode or paragraph request.
    for (before, empty_words, request, expected_lines) in [
        ("", ".B \"\"\n", ".nf", 2),
        ("BEFORE\n", ".B \"\"\n", ".nf", 2),
        ("", ".B \"\"\n", ".PP", 2),
        ("BEFORE\n", ".B \"\"\n", ".PP", 2),
        ("", ".B \"\"\n", ".sp 0", 2),
        ("BEFORE\n", ".B \"\"\n", ".sp 0", 2),
        ("", ".B \"\"\n", ".fi", 2),
        ("", ".B \"\"\n.B \"\"\n", ".nf", 3),
    ] {
        let source = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n{before}{empty_words}\\&\n{request}\nAFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("completed-invisible-cell.1"),
            source.as_bytes(),
        )
        .unwrap();
        let blocks = &document.sections[0].blocks;
        let rows = blocks
            .iter()
            .filter_map(|block| match block {
                Block::VerticalSpace { lines, .. } => Some(*lines),
                _ => None,
            })
            .sum::<u16>();
        assert_eq!(rows, expected_lines, "{request}: {blocks:#?}");
        assert_eq!(
            blocks
                .iter()
                .filter(|block| matches!(block, Block::VerticalSpace { lines, .. } if *lines == expected_lines))
                .count(),
            1,
            "{request}: {blocks:#?}"
        );
    }
}

#[test]
fn authored_an_words_use_no_fill_source_rows() {
    // Exact forms checked with fixed CVS -Tascii/-Tlint. In
    // mdoc_term.c::print_mdoc_node(), NODE_NOFILL/NODE_LINE executes before
    // termp_an_pre(); mode-only An changes SPLIT without emitting a word.
    for mode in ["", ".An -split\n", ".An -nosplit\n"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n{mode}.An Alice\n.An Bob\n.fi\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("no-fill-authors.1"), source.as_bytes())
                .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "Alice\nBob"),
            "{mode}: {document:#?}"
        );
    }
}

#[test]
fn author_mode_request_does_not_render_rejected_operands() {
    // Exact input checked with fixed CVS -Tascii/-Tlint. The parser reports
    // excess An operands; mdoc_term.c::termp_an_pre() returns 0 for both
    // author modes, so neither operand is visited as a formatter word.
    for mode in ["split", "nosplit"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n.An -{mode} Alice\n.An Bob\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("author-mode-extra-operand.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "Bob"),
            "{mode}: {document:#?}"
        );
    }
}

#[test]
fn author_mode_request_respects_no_fill_source_continuation() {
    // Exact split/nosplit forms checked with fixed CVS -Tascii/-Tlint.
    // mdoc_term.c::print_mdoc_node() skips NODE_LINE's term_newln() under
    // TERMP_NONEWLINE; termp_an_pre() changes only the author mode.
    for mode in ["split", "nosplit"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\nALPHA\\c\n.An -{mode}\nBETA\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("author-mode-no-fill-continuation.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "ALPHABETA"),
            "{mode}: {document:#?}"
        );
    }
}

#[test]
fn bare_zero_advance_keeps_generated_closing_glyph_in_its_row() {
    // Exact enclosure and function forms checked with fixed CVS -Tascii/
    // -Tlint. term.c::term_word() buffers a generated glyph after bare \z;
    // mdoc_term.c's post emits it before the next NODE_LINE term_newln().
    for (scope, expected) in [
        (".Ao\n\\z\n.Ac", "<\n>\nNEXT"),
        (".Bo\n\\z\n.Bc", "[\n]\nNEXT"),
        (".Fo call\n\\z\n.Fc", "call(\n)\nNEXT"),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n{scope}\nNEXT\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("bare-zero-generated-post.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == expected),
            "{scope}: {document:#?}"
        );
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
                .filter(|inline| matches!(inline, Inline::LineBreak))
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
        assert!(!item.layout.inline_term, "{request}: {item:#?}");
        assert!(
            matches!(&item.description[0], Block::Paragraph { children, .. } if inline_text(children) == " BODY"),
            "{request}: {item:#?}"
        );
    }
}

#[test]
fn consecutive_empty_formatter_words_occupy_one_native_row() {
    // Exact no-fill pairs and single/Ns counters checked with fixed CVS
    // -Tascii/-Tlint. term.c::term_word() writes the automatic separator
    // before decoding the second empty word; Ns suppresses that write.
    for words in [
        ".No \"\" No \"\"",
        ".No \\fB No \\fI",
        ".No \"\" Em \"\"",
        ".An \"\" An \"\"",
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n{words}\n.No AFTER\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("empty-word-cell.1"), source.as_bytes())
                .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "\nAFTER"),
            "{words}: {document:#?}"
        );
    }
    for words in [".No \"\"", ".No \"\" Ns No \"\""] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n{words}\n.No AFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("empty-word-counter.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "AFTER"),
            "{words}: {document:#?}"
        );
    }
}

#[test]
fn empty_word_cell_survives_filled_control_boundaries() {
    // Exact br/sp/Pp forms checked with fixed CVS -Tascii/-Tlint. A native
    // separator cell is completed before the following line/space request.
    for (request, expected_rows) in [(".br", 1), (".sp 0", 1), (".sp 1", 2), (".Pp", 2)] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.No \"\" No \"\"\n{request}\n.No AFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("empty-word-filled-boundary.1"),
            source.as_bytes(),
        )
        .unwrap();
        let blocks = &document.sections[1].blocks;
        let vertical_rows: u16 = blocks
            .iter()
            .filter_map(|block| match block {
                Block::VerticalSpace { lines, .. } => Some(*lines),
                _ => None,
            })
            .sum();
        let hard_rows = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(
                    children
                        .iter()
                        .filter(|inline| matches!(inline, Inline::LineBreak))
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>();
        assert_eq!(
            vertical_rows + u16::try_from(hard_rows).unwrap(),
            expected_rows,
            "{request}: {blocks:#?}"
        );
    }
}

#[test]
fn final_hang_field_reestablishes_body_word_gap() {
    // Exact br/sp 0/sp 1 forms checked with fixed CVS -Tascii/-Tlint.
    // term_flushln() settles each HANG field independently; an earlier
    // completed field cannot consume the final Y/BODY word separator.
    for request in [".br", ".sp 0", ".sp 1"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width Ds\n.It Xo X\n{request}\n.No Y\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("hang-final-field-gap.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{request}: {document:#?}");
        };
        let item = &items[0];
        assert!(inline_text(&item.terms[0]).ends_with('Y'), "{item:#?}");
        assert!(item.layout.inline_term, "{request}: {item:#?}");
        assert_eq!(item.layout.min_term_gap_columns, 1, "{item:#?}");
        assert!(
            matches!(&item.description[0], Block::Paragraph { children, .. } if inline_text(children) == "BODY"),
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
            !items[0].layout.inline_term,
            "{style} {request}: {items:#?}"
        );
    }
}

#[test]
fn final_hang_field_only_consumes_a_proven_body_gap() {
    // All exact fields passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_fill() may break a field at ordinary spaces after .br;
    // cumulative field width is not the final native row column. An
    // indivisible last word spanning the BODY origin still proves no gap.
    for (words, expected_gap) in [
        ("AA BB CC", 1),
        ("YYYYY Z", 1),
        ("YYYYY ZZZZZZZZ", 0),
        (r"YYYYY\:Z", 1),
        (r"YYY\:Z", 0),
        (r"YYY\pZ", 0),
        (r"A BBBBBB\zC", 0),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No {words}\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("hang-final-physical-field.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{words}: {document:#?}");
        };
        assert_eq!(
            items[0].layout.min_term_gap_columns, expected_gap,
            "{items:#?}"
        );
    }
}

#[test]
fn crossed_body_close_pops_font_stack_without_restoring_an_old_value() {
    // Exact input checked with fixed CVS -Tascii/-Tlint. term.c's
    // term_fontrepl() changes the active fontq slot, while mdoc_term.c's
    // print_mdoc_node() uses term_fontpopq() at the original BODY close.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.ft B\n.Bf -emphasis\ninside\n.Ac\nafter\n.Ef\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("crossed-font-body.1"), source).unwrap();
    assert!(
        emphasized_document_text(&document).contains("inside"),
        "{document:#?}"
    );
    let strong = strong_document_text(&document);
    assert!(strong.contains('>'), "{document:#?}");
    assert!(strong.contains("after"), "{document:#?}");
    assert!(strong.contains("tail"), "{document:#?}");
}

#[test]
fn man_links_execute_labels_in_the_surrounding_text_stream() {
    // Exact UR/MT variants checked with fixed CVS -Tascii/-Tlint.
    // man_term.c::print_man_node() visits each BODY child in the same termp,
    // applies NODE_LINE in no-fill, and resets font at man macro boundaries.
    for (open, close, target) in [
        ("UR", "UE", "https://example.com"),
        ("MT", "ME", "test@example.com"),
    ] {
        let source =
            format!(".TH TEST 1\n.SH DESCRIPTION\n\\z\n.{open} {target}\nlabel\n.{close}\nafter\n");
        let document =
            parse_manual_bytes(std::path::Path::new("man-link-zero.1"), source.as_bytes()).unwrap();
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
        let link_label = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => {
                    children.iter().find_map(|inline| match inline {
                        Inline::Link { children, .. } => Some(inline_text(children)),
                        _ => None,
                    })
                }
                _ => None,
            })
            .expect("visible link label");
        assert_eq!(link_label, "abel", "{open}: {document:#?}");
        assert!(text.ends_with("after"), "{open}: {document:#?}");

        for (label, separator) in [("label", " "), ("label\\c", "")] {
            let source =
                format!(".TH TEST 1\n.SH DESCRIPTION\n.{open} {target}\n{label}\n.{close}\n");
            let document = parse_manual_bytes(
                std::path::Path::new("man-link-post-boundary.1"),
                source.as_bytes(),
            )
            .unwrap();
            let text = document.sections[0]
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Paragraph { children, .. } => Some(inline_text(children)),
                    _ => None,
                })
                .collect::<String>();
            assert!(
                text.contains(&format!("label{separator}⟨{target}⟩")),
                "{open} {label}: {document:#?}"
            );
        }

        let source = format!(
            ".TH TEST 1\n.SH DESCRIPTION\n.nf\n.{open} {target}\nfirst\nsecond\n.{close}\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("man-link-no-fill.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(document.sections[0].blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("first\nsecond"))), "{open}: {document:#?}");

        // man_term.c::print_man_node() suppresses NODE_LINE after a \c row.
        // The fixed CVS terminal oracle keeps the two label words together.
        let source = format!(
            ".TH TEST 1\n.SH DESCRIPTION\n.nf\n.{open} {target}\nfirst\\c\nsecond\n.{close}\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("man-link-no-fill-continuation.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(document.sections[0].blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("firstsecond"))), "{open}: {document:#?}");

        let source = format!(
            ".TH TEST 1\n.SH DESCRIPTION\n.ft B\n.{open} {target}\nlabel\n.{close}\nafter\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("man-link-font.1"), source.as_bytes()).unwrap();
        assert!(
            !strong_document_text(&document).contains("after"),
            "{open}: {document:#?}"
        );

        let source = format!(
            ".TH TEST 1\n.SH DESCRIPTION\n.{open} {target}\n.ft B\nlabel\n.{close}\n\\fPafter\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("man-link-body-font.1"),
            source.as_bytes(),
        )
        .unwrap();
        let strong = strong_document_text(&document);
        assert!(strong.contains("label"), "{open}: {document:#?}");
        assert!(!strong.contains(target), "{open}: {document:#?}");
        assert!(!strong.contains("after"), "{open}: {document:#?}");
    }
}

#[test]
fn man_links_preserve_empty_body_identity_and_terminal_target() {
    // Both exact inputs checked with fixed CVS -Tascii/-Thtml/-Tlint.
    // man_html.c::man_UR_pre() chooses HEAD only if BODY has no syntax
    // children. man_term.c::post_UR() still writes the bracketed HEAD after
    // executing a nonprinting BODY child.
    let source = b".TH TEST 1\n.SH DESCRIPTION\n.UR https://example.com\n\\&\n.UE\nafter\n";
    let document =
        parse_manual_bytes(std::path::Path::new("man-link-invisible-label.1"), source).unwrap();
    let text = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<String>();
    assert!(
        text.contains("⟨https://example.com⟩ after"),
        "{document:#?}"
    );
    assert_eq!(
        text.matches("https://example.com").count(),
        1,
        "{document:#?}"
    );
    let source = b".TH TEST 1\n.SH DESCRIPTION\n.UR https://example.com\n.ft B\n.UE\nafter\n";
    let document =
        parse_manual_bytes(std::path::Path::new("man-link-font-only-label.1"), source).unwrap();
    let text = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<String>();
    assert!(
        text.contains("⟨https://example.com⟩ after"),
        "{document:#?}"
    );
    assert!(
        !strong_document_text(&document).contains("after"),
        "{document:#?}"
    );
}

#[test]
fn man_link_entry_observes_no_fill_source_rows_without_ending_a_continuation() {
    // Exact UR/MT inputs checked with fixed CVS -Tascii/-Tlint. In
    // man_term.c::print_man_node(), NODE_LINE calls term_newln() only when
    // TERMP_NONEWLINE is clear; pre_UR() adds no further row boundary.
    for (open, close) in [("UR", "UE"), ("MT", "ME")] {
        for prefix in ["prefix\\c", "prefix\\zX\\c"] {
            let source = format!(
                ".TH TEST 1\n.SH DESCRIPTION\n.nf\n{prefix}\n.{open} https://example.com\nlabel\n.{close}\n.fi\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("man-link-continuation.1"),
                source.as_bytes(),
            )
            .unwrap();
            let literal = document.sections[0]
                .blocks
                .iter()
                .find_map(|block| match block {
                    Block::Preformatted { children, .. } => Some(inline_text(children)),
                    _ => None,
                })
                .expect("continued literal row");
            assert!(
                literal.contains("prefixlabel"),
                "{open} {prefix}: {document:#?}"
            );
            assert!(
                !literal.contains("prefixX"),
                "{open} {prefix}: {document:#?}"
            );
            assert!(
                !literal.contains("prefix\nlabel"),
                "{open} {prefix}: {document:#?}"
            );
        }
    }

    // Without \c, the same CVS NODE_LINE entry ends the preceding row.
    let source =
        b".TH TEST 1\n.SH DESCRIPTION\n.nf\nprefix\n.UR https://example.com\nlabel\n.UE\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("man-link-new-row.1"), source).unwrap();
    let literal = document.sections[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Preformatted { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .expect("literal rows");
    assert!(literal.contains("prefix\nlabel"), "{document:#?}");

    // Exact empty-BODY variants checked with fixed CVS -Tascii/-Tlint:
    // post_UR() supplies the first visible word, so it owns the same source
    // continuation decision as a real BODY label.
    for (prefix, expected) in [
        ("prefix\\c", "prefix⟨https://example.com⟩"),
        ("prefix", "prefix\n⟨https://example.com⟩"),
    ] {
        let source = format!(
            ".TH TEST 1\n.SH DESCRIPTION\n.nf\n{prefix}\n.UR https://example.com\n.UE\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("man-link-empty-body-row.1"),
            source.as_bytes(),
        )
        .unwrap();
        let literal = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Preformatted { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .expect("empty-BODY literal row");
        assert!(literal.contains(expected), "{prefix}: {document:#?}");
    }
}

#[test]
fn generated_man_link_post_keeps_a_no_fill_word_end_break() {
    // Each exact source was checked with fixed CVS -Tascii/-Tlint. term.c's
    // term_fill() records \p until the next generated term_word() boundary;
    // returning a Rust fragment cannot consume its emitted hard break.
    for (open, close, target) in [("UR", "UE", "x"), ("MT", "ME", "user@example.com")] {
        for (label, expected_break) in [
            ("label\\p\n", true),
            ("label\\p\\p\n", true),
            ("label\\p\\c\n", false),
            ("label\\p\n.ft B\n", true),
        ] {
            let source = format!(
                ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\n.{open} {target}\n{label}.{close}\nafter\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("no-fill-link-word-end.1"),
                source.as_bytes(),
            )
            .unwrap();
            let rows = document.sections[0]
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Preformatted { children, .. } => Some(inline_text(children)),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let boundary = if expected_break {
                "label\n⟨"
            } else {
                "label⟨"
            };
            assert!(rows.contains(boundary), "{open} {label}: {document:#?}");
            assert!(
                rows.contains(&format!("⟨{target}⟩\nafter")),
                "{document:#?}"
            );
        }
    }

    // This bold BODY variant was also checked with fixed CVS -Tascii/-Tlint.
    let source =
        b".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\n.UR x\n.B \"label\\p\"\n.UE\nafter\n";
    let document =
        parse_manual_bytes(std::path::Path::new("bold-link-word-end.1"), source).unwrap();
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("unexpected bold no-fill blocks: {document:#?}");
    };
    assert_eq!(inline_text(children), "label\n⟨x⟩\nafter");
}

#[test]
fn man_paragraph_body_returns_its_live_word_to_the_enclosing_link_post() {
    // Exact UR/MT x PP/P/LP x filled/no-fill inputs were checked with fixed
    // CVS -Tascii/-Tlint. man_term.c::pre_PP() closes the previous row, but
    // its action table gives PP/P/LP no post; only post_UR() next writes <HEAD>.
    // term.c::term_word() lets a tight \c keep \zX for that generated word.
    for (open, close, target) in [("UR", "UE", "x"), ("MT", "ME", "user@example.com")] {
        for paragraph in ["PP", "P", "LP"] {
            for no_fill in [false, true] {
                let mode = if no_fill { ".nf\n" } else { "" };
                let source = format!(
                    ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n{mode}.{open} {target}\nfirst\n.{paragraph}\nsecond\\zX\\c\n.{close}\nafter\n"
                );
                let document = parse_manual_bytes(
                    std::path::Path::new("link-live-paragraph-body.1"),
                    source.as_bytes(),
                )
                .unwrap();
                let trailing = document.sections[0]
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Paragraph { children, .. }
                        | Block::Preformatted { children, .. } => Some(inline_text(children)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let expected = if no_fill {
                    format!("second⟨{target}⟩\nafter")
                } else {
                    format!("second⟨{target}⟩ after")
                };
                assert!(
                    trailing.contains(&expected),
                    "{open} {paragraph}: {document:#?}"
                );
                assert!(!trailing.contains("secondX"), "{document:#?}");
            }
        }
    }
}

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
fn other_man_block_nodes_restore_font_before_enclosing_link_post() {
    // Exact HP/IP/TP/RS cases were checked with fixed CVS -Tascii/-Tlint.
    // print_man_node() applies the same generic font replacement around
    // every printable BLOCK, HEAD, and BODY, including post_IP/post_TP and
    // post_RS paths. A later \fP must not recover the inner bold font.
    for (name, body) in [
        ("HP", ".HP\n\\fBlabel"),
        ("IP", ".IP tag 4\n\\fBlabel"),
        ("TP", ".TP\ntag\n\\fBlabel"),
        ("RS", ".RS\n\\fBlabel\n.RE"),
    ] {
        let source = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.UR \\fPhttps://example.org\n{body}\n.UE\n\\fPafter\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("man-block-font-register.1"),
            source.as_bytes(),
        )
        .unwrap();
        let strong = strong_document_text(&document);
        assert!(strong.contains("label"), "{name}: {document:#?}");
        assert!(
            !strong.contains("https://example.org"),
            "{name}: {document:#?}"
        );
        assert!(!strong.contains('⟩'), "{name}: {document:#?}");
        assert!(!strong.contains("after"), "{name}: {document:#?}");
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
fn man_link_empty_target_still_executes_terminal_block_post() {
    // Exact source checked with fixed CVS -Tascii/-Thtml/-Tlint. The HTML
    // fallback loses the first glyph of after; terminal post_UR() always
    // executes its generated words, even when HEAD decodes to empty text.
    let source = b".TH TEST 1\n.SH DESCRIPTION\n.UR \\&\nlabel\\z\n.UE\nafter\n";
    let document = parse_manual_bytes(std::path::Path::new("empty-link-target.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert!(text.contains("label"), "{document:#?}");
    assert!(text.contains("after"), "{document:#?}");
    assert!(document_link_targets(&document).is_empty(), "{document:#?}");
}

#[test]
fn man_link_body_previous_font_uses_all_macro_boundaries() {
    // Exact input checked with fixed CVS -Tascii/-Thtml/-Tlint.
    // man_term.c::print_man_node() calls term_fontrepl() on BLOCK, HEAD,
    // and BODY entry; each call updates fontlast even for Roman -> Roman.
    let source =
        b".TH TEST 1\n.SH DESCRIPTION\n.ft B\n.UR https://example.com\n\\fPlabel\n.UE\nafter\n";
    let document = parse_manual_bytes(std::path::Path::new("man-link-fontlast.1"), source).unwrap();
    let strong = strong_document_text(&document);
    assert!(!strong.contains("label"), "{document:#?}");
    assert!(!strong.contains("after"), "{document:#?}");
}

#[test]
fn pending_zero_advance_label_glyph_keeps_link_ownership() {
    // All exact inputs checked with fixed CVS -Tascii/-Thtml/-Tlint.
    // term.c::term_word() settles BACKBEFORE at the generated BLOCK-post
    // bracket's word boundary, while a trailing \c permits an overstrike.
    for (name, body, expected_label) in [
        ("suffix", "label\\zX", "labelX"),
        ("only", "\\zX", "X"),
        ("continued", "label\\zX\\c", "label"),
    ] {
        let source = format!(".TH TEST 1\n.SH DESCRIPTION\n.UR x\n{body}\n.UE\nafter\n");
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes()).unwrap();
        let link = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => {
                    children.iter().find_map(|inline| match inline {
                        Inline::Link {
                            children, target, ..
                        } => Some((children, target)),
                        _ => None,
                    })
                }
                _ => None,
            })
            .expect("semantic link");
        assert_eq!(inline_text(link.0), expected_label, "{name}: {document:#?}");
        assert!(matches!(link.1, mant_ir::LinkTarget::External { uri } if uri == "x"));
        let text = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .collect::<String>();
        assert!(text.contains("⟨x⟩ after"), "{name}: {document:#?}");
    }
}

#[test]
fn pending_glyph_before_man_link_stays_outside_its_label() {
    struct Labels<'a>(&'a mut Vec<String>);
    impl<'ir> Visit<'ir> for Labels<'_> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link { children, .. } = inline {
                self.0.push(inline_text(children));
            }
            visit::walk_inline(self, inline);
        }
    }

    // Exact empty, visible BODY, and tight-join UR/MT inputs checked against
    // fixed CVS -Tascii/-Tlint. man_term.c::print_man_node() keeps one termp;
    // term.c::term_word() may emit the prior BACKBEFORE glyph only after the
    // link BODY or the generated post_UR() target starts executing.
    for (open, close, prefix, body, expected_label) in [
        ("UR", "UE", "\\zX", "", "https://example.com"),
        ("UR", "UE", "\\zX", "label\n", "label"),
        ("UR", "UE", "\\zX\\c", "label\n", "label"),
        ("MT", "ME", "\\zX", "label\n", "label"),
    ] {
        let source = format!(
            ".TH TEST 1\n.SH DESCRIPTION\n{prefix}\n.{open} https://example.com\n{body}.{close}\nafter\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("pending-before-man-link.1"),
            source.as_bytes(),
        )
        .unwrap();
        let mut labels = Vec::new();
        for block in &document.sections[0].blocks {
            if let Block::Paragraph { children, .. } = block {
                for inline in children {
                    if let Inline::Link { children, .. } = inline {
                        labels.push(inline_text(children));
                    }
                }
            }
        }
        assert_eq!(labels, [expected_label], "{open}: {document:#?}");
        assert!(
            !labels.iter().any(|label| label.contains('X')),
            "{open}: {document:#?}"
        );
    }

    // Exact nested source also checked with fixed CVS -Tascii/-Tlint: the
    // incoming X precedes both link labels, while both targets survive.
    let source = b".TH TEST 1\n.SH DESCRIPTION\n\\zX\n.UR https://example.com\n.UR inner\ninnerlabel\n.UE\nouterlabel\n.UE\nafter\n";
    let document =
        parse_manual_bytes(std::path::Path::new("pending-nested-link.1"), source).unwrap();
    let targets = document_link_targets(&document);
    assert_eq!(targets.len(), 2, "{document:#?}");
    let mut labels = Vec::new();
    Labels(&mut labels).visit_document(&document);
    assert!(
        labels.iter().all(|label| !label.contains('X')),
        "{document:#?}"
    );

    // These exact BODY controls were checked with fixed CVS -Tascii/-Tlint.
    // They flush or switch IR destinations while the incoming BACKBEFORE
    // glyph is still outside UR's semantic label.
    for (name, body, retained) in [
        ("paragraph", ".PP\nlabel", "label"),
        ("definition", ".IP item 4\nbody", "item"),
        ("no-fill", ".nf\nlabel\n.fi", "label"),
    ] {
        let source = format!(".TH TEST 1\n.SH DESCRIPTION\n\\zX\n.UR outer\n{body}\n.UE\nafter\n");
        let document = parse_manual_bytes(
            std::path::Path::new("pending-before-link-segment.1"),
            source.as_bytes(),
        )
        .unwrap();
        let mut labels = Vec::new();
        Labels(&mut labels).visit_document(&document);
        assert_eq!(labels.len(), 1, "{name}: {document:#?}");
        assert!(labels[0].contains(retained), "{name}: {document:#?}");
        assert!(!labels[0].contains('X'), "{name}: {document:#?}");
        assert!(
            projected_document_text(&document).contains('X'),
            "{name}: {document:#?}"
        );
    }
}

#[test]
fn man_link_bodies_execute_structural_children_and_nested_targets() {
    // Exact IP, PP, and nested UR/MT inputs checked with fixed CVS
    // -Tascii/-Tlint; HTML also confirms IP and nesting (its PP case crashes).
    // man_term.c::print_man_node() visits BODY recursively, then post_UR()
    // prints the HEAD target after it.
    for (open, close, target) in [("UR", "UE", "outer"), ("MT", "ME", "outer@example.com")] {
        let source = format!(
            ".TH TEST 1\n.SH DESCRIPTION\n.{open} {target}\nfirst\n.IP item 4\nbody\n.{close}\nafter\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("link-ip.1"), source.as_bytes()).unwrap();
        let text = projected_document_text(&document);
        for word in ["first", "item", "body", target, "after"] {
            assert!(text.contains(word), "{open} {word}: {document:#?}");
        }
        assert!(document.sections[0].blocks.iter().any(|block| {
            matches!(block, Block::DefinitionList { items, .. } if items.iter().any(|item| item.terms.iter().any(|term| inline_text(term).contains("item")) && item.description.iter().any(|part| matches!(part, Block::Paragraph { children, .. } if inline_text(children).contains("body")))))
        }), "{open}: {document:#?}");
        assert_eq!(document_link_targets(&document).len(), 1, "{document:#?}");

        let source = format!(
            ".TH TEST 1\n.SH DESCRIPTION\n.{open} {target}\nfirst\n.PP\nsecond\n.{close}\nafter\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("link-pp.1"), source.as_bytes()).unwrap();
        let paragraphs = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(paragraphs.iter().any(|part| part.contains("first")));
        assert!(paragraphs.iter().any(|part| part.contains("second")));
        assert!(paragraphs.len() >= 2, "{open}: {document:#?}");
    }

    let source = b".TH TEST 1\n.SH DESCRIPTION\n.UR outer\nfirst\n.UR inner\ninnerlabel\n.UE\nlast\n.UE\nafter\n";
    let document = parse_manual_bytes(std::path::Path::new("nested-ur.1"), source).unwrap();
    let targets = document_link_targets(&document);
    assert!(
        targets.iter().any(
            |target| matches!(target, mant_ir::LinkTarget::External { uri } if uri == "outer")
        ),
        "{document:#?}"
    );
    assert!(
        targets.iter().any(
            |target| matches!(target, mant_ir::LinkTarget::External { uri } if uri == "inner")
        ),
        "{document:#?}"
    );
    let text = projected_document_text(&document);
    for word in ["first", "innerlabel", "inner", "last", "outer", "after"] {
        assert!(text.contains(word), "{word}: {document:#?}");
    }
}

#[test]
fn man_link_body_fill_controls_switch_the_active_output_channel() {
    // Both exact inputs checked with fixed CVS -Tascii/-Thtml/-Tlint.
    // man_term.c::print_man_node() applies NODE_NOFILL at each BODY child,
    // so a mode request inside UR changes the following source row.
    for (name, source, literal_text, filled_text) in [
        (
            "link-enter-no-fill.1",
            ".TH TEST 1\n.SH DESCRIPTION\n.UR outer\n.nf\nfirst\nsecond\n.fi\nthird\n.UE\nafter\n",
            "first\nsecond",
            "third",
        ),
        (
            "link-leave-no-fill.1",
            ".TH TEST 1\n.SH DESCRIPTION\n.nf\n.UR outer\nfirst\n.fi\nsecond\n.UE\nafter\n",
            "first",
            "second",
        ),
        (
            "mail-enter-no-fill.1",
            ".TH TEST 1\n.SH DESCRIPTION\n.MT outer@example.com\n.nf\nfirst\nsecond\n.fi\nthird\n.ME\nafter\n",
            "first\nsecond",
            "third",
        ),
        (
            "mail-leave-no-fill.1",
            ".TH TEST 1\n.SH DESCRIPTION\n.nf\n.MT outer@example.com\nfirst\n.fi\nsecond\n.ME\nafter\n",
            "first",
            "second",
        ),
    ] {
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes()).unwrap();
        assert!(document.sections[0].blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children) == literal_text)), "{name}: {document:#?}");
        assert!(document.sections[0].blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains(filled_text))), "{name}: {document:#?}");
        let text = projected_document_text(&document);
        assert!(text.contains("outer"), "{name}: {document:#?}");
        assert!(text.contains("after"), "{name}: {document:#?}");
    }
}

#[test]
fn man_link_annotation_preserves_surrounding_paragraph() {
    // Exact input checked with fixed CVS -Tascii/-Thtml/-Tlint. A UR BLOCK
    // does not itself end the formatter paragraph; only its BODY is linked.
    let source =
        b".TH TEST 1\n.SH DESCRIPTION\nprefix\n.UR https://example.com\nlabel\n.UE\nsuffix\n";
    let document = parse_manual_bytes(std::path::Path::new("link-surrounding.1"), source).unwrap();
    let paragraphs = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(children),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(paragraphs.len(), 1, "{document:#?}");
    assert!(
        inline_text(paragraphs[0]).contains("prefix label ⟨https://example.com⟩ suffix"),
        "{document:#?}"
    );
    let labels = paragraphs[0]
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(labels, ["label"], "{document:#?}");
}

#[test]
fn structural_link_start_keeps_only_its_body_inside_the_annotation() {
    // Exact input checked with fixed CVS -Tascii/-Thtml/-Tlint. The IP node
    // flushes the shared paragraph, but the preceding prefix is outside UR.
    let source =
        b".TH TEST 1\n.SH DESCRIPTION\nprefix\n.UR outer\nfirst\n.IP item 4\nbody\n.UE\nafter\n";
    let document = parse_manual_bytes(std::path::Path::new("link-prefix-ip.1"), source).unwrap();
    let first = document.sections[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. } if inline_text(children).contains("prefix") => {
                Some(children)
            }
            _ => None,
        })
        .expect("leading paragraph");
    assert!(inline_text(first).contains("prefix first"), "{document:#?}");
    let labels = first
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(labels, ["first"], "{document:#?}");
}

#[test]
fn no_fill_link_label_owns_pending_zero_advance_glyph() {
    // Both exact inputs checked with fixed CVS -Tascii/-Thtml/-Tlint.
    // man_term.c enters the UR BLOCK post on the active no-fill row; term.c
    // settles BACKBEFORE on its first generated word unless \c joins it.
    for (name, body, label) in [
        ("no-fill-zero.1", "label\\zX", "labelX"),
        ("no-fill-zero-continued.1", "label\\zX\\c", "label"),
    ] {
        let source = format!(".TH TEST 1\n.SH DESCRIPTION\n.nf\n.UR x\n{body}\n.UE\n.fi\nafter\n");
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes()).unwrap();
        let link_label = document.sections[0]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Preformatted { children, .. } => {
                    children.iter().find_map(|inline| match inline {
                        Inline::Link { children, .. } => Some(inline_text(children)),
                        _ => None,
                    })
                }
                _ => None,
            })
            .expect("literal link label");
        assert_eq!(link_label, label, "{name}: {document:#?}");
        assert!(
            projected_document_text(&document).contains("⟨x⟩"),
            "{name}: {document:#?}"
        );
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
fn generated_function_events_follow_no_fill_source_order_and_rows() {
    // Both exact inputs were run through the fixed CVS -Ttree, -Thtml,
    // -Tutf8, and -Tlint oracle before these assertions. In
    // mdoc_term.c::print_mdoc_node(), NODE_NOFILL and NODE_LINE are applied
    // to each Fa and explicit Fc marker before its generated words execute.
    let order = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n.Dl marker\n.nf\ninside\n.Fc\n.fi\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("fo-nofill-order.1"), order).unwrap();
    let blocks = &document.sections[0].blocks;
    let marker = blocks.iter().position(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("marker"))).unwrap();
    let inside = blocks.iter().position(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("inside"))).unwrap();
    assert!(marker < inside, "{blocks:#?}");
    let Block::Preformatted { children, .. } = &blocks[inside] else {
        unreachable!()
    };
    assert_eq!(inline_text(children), "inside)", "{blocks:#?}");

    let arguments = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n.nf\n.Fa first\n.Fa second\n.fi\n.Dl body\n.Fc\n";
    let document =
        parse_manual_bytes(std::path::Path::new("fo-nofill-arguments.1"), arguments).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("first,\nsecond"))), "{blocks:#?}");
}

#[test]
fn filled_and_no_fill_segments_share_one_pending_text_execution() {
    // This exact source was run through the fixed CVS -Tutf8/-Tlint oracle.
    // mdoc_term.c::print_mdoc_node() changes the fill channel at NODE_LINE,
    // while term.c::term_word() keeps the backtracking and word-end registers
    // in the same termp across the output destinations.
    let source = b".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd shared text execution\n.Sh DESCRIPTION\n.No BEFORE\\zX\\c\n.nf\n.No NEXT\n.No SECOND\\p\n.fi\n.No AFTER\n";
    let document = parse_manual_bytes(std::path::Path::new("shared-fill-state.1"), source)
        .expect("lower fill changes with one formatter");
    let blocks = &document.sections[1].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "BEFOREX")), "{blocks:#?}");
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("NEXT\nSECOND"))), "{blocks:#?}");
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "AFTER")), "{blocks:#?}");
}

#[test]
fn aligned_no_fill_rows_settle_pending_glyphs_at_the_native_flush() {
    // Every exact input was run through the fixed CVS -Tutf8/-Tlint oracle.
    // roff_term.c::roff_term_pre_ce() flushes each grouped line; its .br
    // child calls roff_term_pre_br() and therefore term_newln() first.
    for (name, request, tail, expected) in [
        ("center", ".ce 1", "", "X"),
        ("right", ".rj 1", "", "X"),
        ("center-break", ".ce 2", ".br\nY\n", "XY"),
    ] {
        let source = format!(
            ".TH PROBE 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.nf\n{request}\n\\zX\n{tail}.fi\nAFTER\n"
        );
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes())
            .expect("lower aligned no-fill row");
        let blocks = &document.sections[0].blocks;
        let literal_text = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Preformatted { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(
            literal_text.replace('\n', ""),
            expected,
            "{name}: {blocks:#?}"
        );
        assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "AFTER")), "{name}: {blocks:#?}");
    }
}

#[test]
fn keep_words_survives_display_output_switches() {
    // Exact inputs checked with pinned CVS -Tascii/-Tlint.  Its
    // mdoc_term.c::termp_bk_pre/post keeps TERMP_PREKEEP/KEEP in the native
    // formatter across Bd BODY entry and exit.  term.c's escape break stays
    // within the kept formatter word, so both display variants read A B.
    for (name, body) in [
        (
            "keep-inside-display",
            ".Bd -literal\n.Bk -words\n.No A\\p No B\n.Ek\n.Ed",
        ),
        (
            "keep-outside-display",
            ".Bk -words\n.Bd -literal\n.No A\\p No B\n.Ed\n.Ek",
        ),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd keep probe\n.Sh DESCRIPTION\n{body}\nTAIL\n"
        );
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes()).unwrap();
        let blocks = &document.sections[1].blocks;
        assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("A B"))), "{name}: {blocks:#?}");
    }
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
fn no_fill_end_marker_shares_scope_post_state() {
    // Exact input checked against fixed CVS tree, HTML, terminal, and lint.
    // mdoc.c::mdoc_endbody_alloc links Ac to Ao's BODY, and
    // mdoc_term.c::print_mdoc_node marks that BODY ended at the marker.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Dl marker\n.nf\n.Bo\ninside\n.Ac\nafter\n.Bc\n.fi\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("nofill-scope-post.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert_eq!(text.matches('>').count(), 1, "{text:?}");
    assert_eq!(text.matches(']').count(), 1, "{text:?}");
    assert!(text.contains("inside\n>"), "{text:?}");

    // Exact crossed Fo/Fc input also checked with the same fixed oracle.
    // The temporary no-fill builder must mark the original Fo BODY ended
    // before its outer structural walk reaches the ordinary post.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n.Dl marker\n.nf\n.Bo\ninside\n.Fc\nafter\n.Bc\n.fi\ntail\n";
    let document =
        parse_manual_bytes(std::path::Path::new("nofill-function-post.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert_eq!(text.matches(')').count(), 1, "{text:?}");
    assert!(text.contains("inside\n)"), "{text:?}");
}

#[test]
fn ordinary_no_fill_scopes_execute_each_authored_line() {
    // Exact input checked with pinned CVS -Ttree/-Tutf8. In
    // mdoc_term.c::print_mdoc_node(), NODE_LINE runs on each Fa before its
    // generated argument; Fo's post shares the second argument's row.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Fo call\n.Fa first\n.Fa second\n.Fc\n.fi\n";
    let document =
        parse_manual_bytes(std::path::Path::new("ordinary-nofill-fo.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("call(\nfirst,\nsecond)"))), "{blocks:#?}");

    // Both exact inputs checked with the same CVS terminal oracle. Ao's
    // generated open and Eo's authored head each enter their own NODE_LINE;
    // ordinary text lines remain distinct before their respective posts.
    for (name, source, expected) in [
        ("Ao", b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Ao\nfirst\nsecond\n.Ac\n.fi\n".as_slice(), "<\nfirst\nsecond>"),
        ("Eo", b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Eo OPEN\nfirst\nsecond\n.Ec CLOSE\n.fi\n".as_slice(), "OPEN\nfirst\nsecond\nCLOSE"),
    ] {
        let document = parse_manual_bytes(std::path::Path::new(name), source).unwrap();
        let text = projected_document_text(&document);
        assert!(text.contains(expected), "{name}: {text:?}");
    }
}

#[test]
fn generated_post_consumes_the_current_zero_advance_cell() {
    // Exact input checked with pinned CVS -Tutf8. term.c::term_word() keeps
    // BACKBEFORE until the following generated Ao post word overstrikes X.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Ao\n.Dl marker\n\\zX\n.Ac\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("nofill-post-zero.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert!(!text.contains("X>"), "{text:?}");
    assert!(text.contains('>'), "{text:?}");

    // Exact reverse input checked with CVS -Tutf8. Eo's authored Ec tail is
    // a separate NODE_LINE, so X must be committed on its own row instead
    // of being consumed by CLOSE.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Eo OPEN\n\\zX\n.Ec CLOSE\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("nofill-eo-zero.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert!(text.contains("OPEN\nX\nCLOSE"), "{text:?}");
}

#[test]
fn crossed_body_end_restores_enclosing_font_before_post() {
    // Exact input checked with pinned CVS -Ttree/-Tutf8. The Ac BODY-end
    // marker invokes mdoc_term.c::print_mdoc_node()'s term_fontpopq() for
    // Ao's original BODY before it writes the closing delimiter.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bf -emphasis\ninside\n.Ac\nafter\n.Ef\ntail\n";
    let document =
        parse_manual_bytes(std::path::Path::new("body-font-checkpoint.1"), source).unwrap();
    let emphasis = emphasized_document_text(&document);
    assert!(
        emphasis.contains("inside"),
        "{:#?}",
        document.sections[0].blocks
    );
    assert!(
        !emphasis.contains('>'),
        "{:#?}",
        document.sections[0].blocks
    );
    assert!(
        !emphasis.contains("after"),
        "{:#?}",
        document.sections[0].blocks
    );

    // Each exact source was also checked with pinned CVS -Tutf8. Its
    // mdoc_term.c::print_mdoc_node() BODY checkpoint applies to the original
    // BODY whatever macro owns it; Eo's Ec tail executes before that pop.
    for (name, body, close_stays_emphasized) in [
        (
            "Bo",
            ".Bo\n.Bf -emphasis\ninside\n.Bc\nafter\n.Ef\ntail\n",
            false,
        ),
        (
            "Eo",
            ".Eo OPEN\n.Bf -emphasis\ninside\n.Ec CLOSE\nafter\n.Ef\ntail\n",
            true,
        ),
        (
            "Fo",
            ".Fo call\n.Bf -emphasis\ninside\n.Fc\nafter\n.Ef\ntail\n",
            false,
        ),
        (
            "Bk",
            ".Bk -words\n.Bf -emphasis\ninside\n.Ek\nafter\n.Ef\ntail\n",
            false,
        ),
        (
            "Bd",
            ".Bd -literal\n.Bf -emphasis\ninside\n.Ed\nafter\n.Ef\ntail\n",
            false,
        ),
        (
            "Bl",
            ".Bl -bullet\n.It\n.Bf -emphasis\ninside\n.El\nafter\n.Ef\ntail\n",
            false,
        ),
    ] {
        let source = format!(".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n{body}");
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes()).unwrap();
        let emphasis = emphasized_document_text(&document);
        assert!(
            emphasis.contains("inside"),
            "{name}: {:#?}",
            document.sections[0].blocks
        );
        assert!(
            !emphasis.contains("after"),
            "{name}: {:#?}",
            document.sections[0].blocks
        );
        assert_eq!(
            emphasis.contains("CLOSE"),
            close_stays_emphasized,
            "{name}: {:#?}",
            document.sections[0].blocks
        );
    }
}

#[test]
fn body_font_checkpoint_only_pops_pushed_scopes() {
    // Exact input checked with fixed CVS -Tutf8. term.c::term_fontpopq()
    // leaves `.ft B` in the current stack slot; only a pushed mdoc font is
    // unwound at Ao BODY exit. The later \fP selects the previous slot.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.ft B\ninside\n.Ac\nafter\n\\fPprevious\n";
    let document = parse_manual_bytes(std::path::Path::new("body-ft-slot.1"), source).unwrap();
    let strong = strong_document_text(&document);
    assert!(
        strong.contains("inside"),
        "{:#?}",
        document.sections[0].blocks
    );
    assert!(
        strong.contains("after"),
        "{:#?}",
        document.sections[0].blocks
    );
    assert!(
        !strong.contains("previous"),
        "{:#?}",
        document.sections[0].blocks
    );
}

#[test]
fn crossed_outer_close_keeps_inner_body_font_checkpoint() {
    // Exact input checked with pinned CVS -Ttree/-Tutf8. Ac closes Ao inside
    // Bo, but mdoc_term.c::print_mdoc_node() later uses Bo's own prev_font at
    // Bc. A Bf scope opened between Ac and Bc must then be unwound.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bo\n.Ac\n.Bf -emphasis\ninside\n.Bc\nafter\n.Ef\ntail\n";
    let document =
        parse_manual_bytes(std::path::Path::new("crossed-font-bodies.1"), source).unwrap();
    let emphasis = emphasized_document_text(&document);
    assert!(
        emphasis.contains("inside"),
        "{:#?}",
        document.sections[0].blocks
    );
    assert!(
        !emphasis.contains("after"),
        "{:#?}",
        document.sections[0].blocks
    );
}

#[test]
fn crossed_display_end_does_not_undo_later_no_fill_request() {
    // Exact input checked with pinned CVS -Ttree/-Tutf8. mdoc_macro.c's
    // blk_exp_close restores Bd fill at Ed's source position, before nf.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bd -literal\n.Bo\ninside\n.Ed\n.nf\nafter\n.Bc\n.Ac\n.fi\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("crossed-ed-nf.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("after]>"))), "{blocks:#?}");
}

#[test]
fn generated_posts_follow_current_fill_after_fi_inside_an_old_no_fill_body() {
    // Both exact inputs were run against the pinned CVS -Ttree/-Thtml/-Tutf8
    // oracle. mdoc_term.c::print_mdoc_node applies the current per-node
    // NODE_NOFILL mode; a BODY's earlier flag does not freeze later posts.
    let angle = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Ao\ninside\n.fi\nafter\n.Ac\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("angle-fill-return.1"), angle).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("inside"))), "{blocks:#?}");
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).starts_with("after>"))), "{blocks:#?}");

    let function = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Fo call\n.fi\n.Fa arg\n.Fc\ntail\n";
    let document =
        parse_manual_bytes(std::path::Path::new("function-fill-return.1"), function).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains("arg)"))), "{blocks:#?}");
}

#[test]
fn nested_list_returns_fill_mode_before_parent_enclosure_post() {
    // Exact input checked against pinned CVS -Thtml and -Tutf8. In
    // mdoc_term.c::print_mdoc_node(), the It BODY executes `.fi` before
    // the outer Ao BODY's closing word, despite the detached list output.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.nf\n.Bl -bullet\n.It\ninside\n.fi\nafter\n.El\n.Ac\ntail\n";
    let document =
        parse_manual_bytes(std::path::Path::new("list-fi-parent-post.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    let list_has_filled_after = blocks.iter().any(|block| {
        let Block::List { items, .. } = block else { return false };
        items.iter().any(|item| item.blocks.iter().any(|block| {
            matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains("after"))
        }))
    });
    assert!(list_has_filled_after, "{blocks:#?}");
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains('>'))), "{blocks:#?}");
    assert!(!blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains('>'))), "{blocks:#?}");
}

#[test]
fn generated_no_fill_opening_consumes_its_own_source_line() {
    // Exact input checked against pinned CVS -Thtml and -Tutf8.
    // mdoc_term.c::print_mdoc_node calls term_newln for NODE_LINE before
    // executing Ao's opening term_word, even when that word is generated.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\nbefore\n.Ao\ninside\n.Bd -literal\nblock\n.Ed\n.Ac\n.fi\n";
    let document =
        parse_manual_bytes(std::path::Path::new("nofill-generated-line.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("before\n<\ninside"))), "{blocks:#?}");

    // Eo's first visible word comes from its HEAD child, which has no
    // NODE_LINE of its own in the fixed CVS tree. The Eo block owns it.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\nbefore\n.Eo OPEN\n.Bd -literal\nblock\n.Ed\n.Ec CLOSE\n.fi\n";
    let document =
        parse_manual_bytes(std::path::Path::new("eo-nofill-head-line.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("before\nOPEN"))), "{blocks:#?}");

    // Exact `before\c` variant checked against fixed CVS -Ttree/-Tutf8.
    // print_mdoc_node() skips term_newln when TERMP_NONEWLINE is still set.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\nbefore\\c\n.Eo OPEN\n.Bd -literal\nblock\n.Ed\n.Ec CLOSE\n.fi\n";
    let document =
        parse_manual_bytes(std::path::Path::new("eo-nofill-continued-head.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("beforeOPEN"))), "{blocks:#?}");

    // Exact crossed Ao/Dl `\c` input checked with fixed CVS tree, HTML,
    // and terminal. term.c::term_word() sets TERMP_NONEWLINE on the real
    // inside word, which joins only the following generated closing word.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Ao\n.Dl marker\ninside\\c\n.Ac\nafter\n.fi\n";
    let document =
        parse_manual_bytes(std::path::Path::new("ao-generated-continuation.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert!(text.contains("inside>"), "{text:?}");
    assert!(!text.contains("inside\n>"), "{text:?}");
}

#[test]
fn empty_containers_still_execute_their_no_fill_source_line() {
    // Both exact inputs checked with the fixed CVS tree, HTML, and terminal.
    // mdoc_term.c::print_mdoc_node checks NODE_LINE on the container itself
    // before any pre, children, or post handler can emit a word.
    let empty_head = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\nbefore\n.Fo\n.Bl -bullet\n.It\nitem\n.El\n.Fc\n.fi\n";
    let document =
        parse_manual_bytes(std::path::Path::new("empty-fo-source-line.1"), empty_head).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("before\n("))), "{blocks:#?}");

    let empty_font = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Ao\n.Dl marker\ninside\n.Bf -emphasis\n.Ef\n.Ac\n.fi\n";
    let document =
        parse_manual_bytes(std::path::Path::new("empty-bf-source-line.1"), empty_font).unwrap();
    let text = projected_document_text(&document);
    assert!(text.contains("inside\n>"), "{text:?}");
}

#[test]
fn display_local_no_fill_does_not_change_parent_enclosure_channel() {
    // Exact input checked against pinned CVS -Thtml and -Tutf8.
    // mdoc_macro.c::blk_exp_close restores the pre-display fill mode;
    // the following Ao text and post return to the incoming fill channel.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bd -literal\n.nf\ninside\n.Ed\nnext\n.Ac\n";
    let document = parse_manual_bytes(std::path::Path::new("bd-local-nofill.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains("next>"))), "{blocks:#?}");
}

#[test]
fn explicit_bf_end_restores_original_body_font_inside_crossed_enclosure() {
    // Exact input checked against fixed CVS tree, HTML, terminal, and lint.
    // mdoc_term.c::print_mdoc_node saves prev_font on the Bf BODY and uses
    // its linked body at the Ef marker when calling term_fontpopq().
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bf -emphasis\n.Ao\ninside\n.Ef\nafter\n.Ac\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("bf-crossed-close.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    let emphasized = emphasized_document_text(&document);
    assert!(emphasized.contains("inside"), "{blocks:#?}");
    assert!(!emphasized.contains("after"), "{blocks:#?}");
}

#[test]
fn explicit_bf_end_restores_font_across_detached_list_and_display() {
    // Both exact inputs checked with pinned CVS -Ttree/-Thtml/-Tutf8.
    // mdoc_term.c::print_mdoc_node restores the original Bf BODY prev_font
    // at Ef, including when the end marker is inside It or Bd. Its terminal
    // font contract deliberately differs from HTML's Bf wrapper styling.
    for (label, source) in [
        ("list", b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bf -emphasis\n.Bl -bullet\n.It\ninside\n.Ef\nafter\n.El\ntail\n".as_slice()),
        ("display", b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bf -emphasis\n.Bd -literal\ninside\n.Ef\nafter\n.Ed\ntail\n".as_slice()),
    ] {
        let document = parse_manual_bytes(std::path::Path::new(label), source).unwrap();
        let emphasized = emphasized_document_text(&document);
        assert!(emphasized.contains("inside"), "{label}: {:#?}", document.sections[0].blocks);
        assert!(!emphasized.contains("after"), "{label}: {:#?}", document.sections[0].blocks);
    }
}

#[test]
fn mdoc_explicit_body_end_executes_post_once_at_the_marker() {
    // Exact sources checked with the pinned CVS -Ttree, -Thtml, and -Tutf8.
    // mdoc.c::mdoc_endbody_alloc links the marker to the original BODY;
    // mdoc_html.c::print_mdoc_node executes marker children before its post
    // and sets NODE_ENDED so the original BODY cannot post a second time.
    for (label, source, expected) in [
        (
            "angle",
            ".Dd September 27, 2026\n.Dt CLOSE 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bo\ninside\n.Ac\nafter-angle\n.Bc\ntail\n",
            "<[inside> after-angle] tail",
        ),
        (
            "authored",
            ".Dd September 27, 2026\n.Dt EC 1\n.Os\n.Sh DESCRIPTION\n.Eo opening\n.Bo\ninside\n.Ec closing\nafter-ec\n.Bc\n",
            "opening[insideclosing after-ec]",
        ),
        (
            "function",
            ".Dd September 28, 2026\n.Dt CLOSE 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n.Bo\n.Fa arg\n.Fc\nafter-fc\n.Bc\nnext\n",
            "call([arg) after-fc] next",
        ),
        (
            "authored-nospace",
            ".Dd September 27, 2026\n.Dt EONS 1\n.Os\n.Sh DESCRIPTION\n.Eo opening\n.Bk -words\ninside\n.Ec closing\n.No pre Ns\n.Ek\n.No NEXT\n",
            "openinginsideclosing preNEXT",
        ),
    ] {
        let document = parse_manual_bytes(std::path::Path::new(label), source.as_bytes())
            .expect("lower explicit mdoc BODY close");
        let text = projected_document_text(&document);
        assert_eq!(
            text.trim(),
            format!("DESCRIPTION{expected}"),
            "{label}: {text:?}"
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
fn function_body_reenters_structural_list_and_display_before_its_close() {
    // Exact sources checked against pinned CVS -Ttree, -Thtml and -Tutf8.
    // mdoc_html.c::mdoc_fo_pre/post process the Fo BODY's structural children;
    // print_mdoc_node runs a nested Fo body-end marker at its source position.
    for (label, middle, expected_structure) in [
        ("list", ".Bl -bullet\n.It\n.Fa arg\n.Fc\n.El", "list"),
        ("display", ".Bd -literal\n.Fa arg\n.Fc\n.Ed", "display"),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt FO{} 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n{middle}\nnext\n",
            label.to_uppercase()
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("fo-{label}-close.1")),
            source.as_bytes(),
        )
        .expect("lower function with structural body");
        let blocks = &document.sections[0].blocks;
        let Some(Block::Paragraph { children: head, .. }) = blocks.first() else {
            panic!("{label}: {blocks:#?}");
        };
        assert_eq!(inline_text(head), "call(", "{label}");
        assert!(
            head.iter()
                .any(|inline| matches!(inline, Inline::Anchor { id, .. } if id == "call")),
            "{label}: {head:#?}"
        );
        match (expected_structure, blocks.get(1)) {
            ("list", Some(Block::List { .. })) | ("display", Some(Block::Preformatted { .. })) => {}
            _ => panic!("{label}: {blocks:#?}"),
        }
        let text = projected_document_text(&document);
        assert_eq!(text.matches(')').count(), 1, "{label}: {text:?}");
        assert!(text.contains("arg"), "{label}: {text:?}");
    }
}

#[test]
fn structural_function_retains_commas_between_direct_arguments() {
    // Exact input checked against fixed CVS tree, HTML and UTF-8 output.
    // mdoc_html.c::mdoc_fa_pre inserts a comma when the next logical sibling
    // is Fa, even when a later structural child splits the Fo output flow.
    let source = b".Dd September 28, 2026\n.Dt FOCOMMA 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n.Fa first\n.Fa second\n.Bl -bullet\n.It\nbody\n.El\n.Fc\n";
    let document = parse_manual_bytes(std::path::Path::new("fo-comma-list.1"), source)
        .expect("lower function arguments before a list");
    let blocks = &document.sections[0].blocks;
    let Some(Block::Paragraph { children, .. }) = blocks.first() else {
        panic!("{blocks:#?}");
    };
    assert_eq!(inline_text(children), "call(first, second");
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, Block::List { .. })),
        "{blocks:#?}"
    );
}

#[test]
fn direct_function_argument_after_display_end_uses_restored_fill_mode() {
    // Exact source checked against fixed CVS tree, HTML and UTF-8 output.
    // mdoc_macro.c::blk_exp_close restores fill at `.Ed`; mdoc_html.c's
    // print_mdoc_node checks NODE_NOFILL on the following direct Fa child.
    let source = b".Dd September 28, 2026\n.Dt FOSWITCH 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Fo call\n.Bl -bullet\n.It\nitem\n.El\n.Ed\n.Fa later\n.Fc\nnext\n";
    let document = parse_manual_bytes(std::path::Path::new("fo-display-switch.1"), source)
        .expect("lower function after display end");
    let blocks = &document.sections[0].blocks;
    assert!(
        matches!(blocks.first(), Some(Block::Preformatted { .. })),
        "{blocks:#?}"
    );
    assert!(
        matches!(blocks.get(1), Some(Block::List { .. })),
        "{blocks:#?}"
    );
    let Some(Block::Paragraph { children, .. }) = blocks.get(2) else {
        panic!("function argument did not return to filled flow: {blocks:#?}");
    };
    assert_eq!(inline_text(children), "later)");
}

#[test]
fn dl_single_line_display_does_not_require_native_no_fill_flags() {
    // Exact source checked against fixed CVS tree, HTML and UTF-8 output.
    // mdoc_html.c::mdoc_d1_pre creates a display/code container for Dl;
    // mdoc_term.c::termp_d1_pre starts its display row. Its text children do
    // not have NODE_NOFILL, unlike Bd -literal content.
    let source = b".Dd September 28, 2026\n.Dt DLFLAG 1\n.Os\n.Sh Redirections\n.Dl [n] Va redir-op Ar file\n";
    let document = parse_manual_bytes(std::path::Path::new("dl-flag.1"), source)
        .expect("lower one-line literal display");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "Dl lost its display row: {:#?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(children), "[n] redir-op file");
}

#[test]
fn display_nodes_keep_native_word_and_row_execution_across_fragments() {
    // Exact input checked with the pinned CVS -Tascii/-Thtml/-Tlint oracle.
    // mdoc_term.c::termp_fl_pre()/termp_pf_post() bind adjacent words;
    // termp_eo_post() releases NOSPACE while TERMP_NONEWLINE keeps the
    // following authored leading blank on the same literal row.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh DESCRIPTION\n.Bd -literal\n.Fl Ar file\n.No a Pf \\& No b\n.Eo [\n.No BEFORE\\c\n.Ec\n AFTER\n.Ed\n";
    let document =
        parse_manual_bytes(std::path::Path::new("display-source-execution.1"), source).unwrap();
    let [Block::Preformatted { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!(
            "display lost its literal body: {:#?}",
            document.sections[1].blocks
        );
    };
    assert_eq!(inline_text(children), "-file\na b\n[\nBEFORE  AFTER");
}

#[test]
fn explicit_display_end_restores_fill_inside_an_open_inline_scope() {
    // Exact input checked against fixed CVS tree, HTML and UTF-8 output.
    // mdoc_html.c::print_mdoc_node switches fill mode using NODE_NOFILL for
    // each node; a Bd BODY end marker can precede its ancestor Bo's close.
    let source = b".Dd September 27, 2026\n.Dt BDCLOSE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Bo\ninside\n.Ed\nafter-ed\n.Bc\nnext\n";
    let document = parse_manual_bytes(std::path::Path::new("bd-close.1"), source)
        .expect("lower explicit display close");
    let blocks = &document.sections[0].blocks;
    let [
        Block::Preformatted {
            children: literal, ..
        },
        Block::Paragraph {
            children: filled, ..
        },
        Block::Paragraph {
            children: after, ..
        },
    ] = blocks.as_slice()
    else {
        panic!("unexpected display close blocks: {blocks:#?}");
    };
    assert_eq!(inline_text(literal), "[\ninside");
    assert_eq!(inline_text(filled), "after-ed]");
    assert_eq!(inline_text(after), "next");
}

#[test]
fn enclosure_post_crosses_detached_definition_list_without_duplication() {
    // Exact input checked against fixed CVS tree, HTML and UTF-8 output.
    // mdoc.c::mdoc_endbody_alloc puts the Ao BODY end in the It BODY;
    // mdoc_html.c::print_mdoc_node posts there and marks the Ao body ended.
    let source = b".Dd September 27, 2026\n.Dt LISTCLOSE 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bl -tag -width key\n.It key\ninside\n.Ac\nafter-ac\n.El\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("list-close.1"), source)
        .expect("lower cross-list explicit close");
    let text = projected_document_text(&document);
    assert_eq!(text.matches('>').count(), 1, "{text:?}");
    assert!(text.contains("inside> after-ac"), "{text:?}");
    assert!(text.ends_with("tail"), "{text:?}");
}

#[test]
fn crossed_enclosure_and_ordinary_enclosure_post_in_detached_list() {
    // Exact source checked against pinned CVS tree, HTML and UTF-8 output.
    // mdoc_html.c::print_mdoc_node ends only Ao's linked BODY at `.Ac`;
    // the still-open Bo BODY executes its ordinary post when it unwinds.
    let source = b".Dd September 27, 2026\n.Dt NESTCLOSE 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bl -tag -width key\n.It key\n.Bo\ninside\n.Ac\nafter-angle\n.Bc\n.El\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("nested-list-close.1"), source)
        .expect("lower crossed and normal nested posts");
    let text = projected_document_text(&document);
    assert!(text.contains("[inside> after-angle]"), "{text:?}");
    assert_eq!(text.matches('>').count(), 1, "{text:?}");
    assert_eq!(text.matches(']').count(), 1, "{text:?}");
}

#[test]
fn display_end_before_nested_list_keeps_following_enclosure_post_filled() {
    // Exact input checked against fixed CVS tree, HTML and UTF-8 output.
    // mdoc_macro.c::blk_exp_close restores fill mode at `.Ed`; HTML checks
    // NODE_NOFILL before dispatching the following Bl, then Bo's later post.
    let source = b".Dd September 27, 2026\n.Dt BDLIST 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Bo\ninside\n.Ed\n.Bl -bullet\n.It\nitem\n.El\n.Bc\nnext\n";
    let document = parse_manual_bytes(std::path::Path::new("bd-list-close.1"), source)
        .expect("lower display close before nested list");
    let blocks = &document.sections[0].blocks;
    assert!(
        matches!(blocks.first(), Some(Block::Preformatted { .. })),
        "{blocks:#?}"
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, Block::List { .. })),
        "{blocks:#?}"
    );
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "]")), "{blocks:#?}");
    let text = projected_document_text(&document);
    assert_eq!(text.matches(']').count(), 1, "{text:?}");
    assert!(text.contains("item"), "{text:?}");
}

#[test]
fn mdoc_unordered_list_markers_retain_their_native_style() {
    // Exact inputs checked with fixed CVS tree, HTML and UTF-8 output.
    // mdoc_html.c::mdoc_bl_pre differentiates Bl-bullet and Bl-dash;
    // mdoc_term.c::termp_it_pre uses a bullet, dash, or no marker.
    for (style, expected) in [
        ("bullet", mant_ir::ListKind::Bullet),
        ("dash", mant_ir::ListKind::Dash),
        ("hyphen", mant_ir::ListKind::Dash),
        ("item", mant_ir::ListKind::Plain),
    ] {
        let source = format!(
            ".Dd September 27, 2026\n.Dt LISTSTYLE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style}\n.It\nentry\n.El\nafter\n"
        );
        let document = parse_manual_bytes(std::path::Path::new("list-style.1"), source.as_bytes())
            .expect("lower list marker style");
        let [Block::List { kind, .. }, Block::Paragraph { .. }] =
            document.sections[0].blocks.as_slice()
        else {
            panic!("-{style}: {:#?}", document.sections[0].blocks);
        };
        assert_eq!(*kind, expected, "-{style}");
    }
}

#[test]
fn an_empty_word_keeps_its_executed_gap_before_ns() {
    // Exact input checked against fixed CVS tree, HTML and UTF-8 output.
    // term.c::term_word() has committed the empty operand's separator before
    // mdoc Ns tightens only the following boundary.
    let source = b".Dd September 27, 2026\n.Dt EMPTYJOIN 1\n.Os\n.Sh DESCRIPTION\n.No before No \"\" Ns No after\n";
    let document = parse_manual_bytes(std::path::Path::new("empty-join.1"), source)
        .expect("lower empty word before Ns");
    assert_eq!(
        projected_document_text(&document),
        "DESCRIPTIONbefore after"
    );
}

#[test]
fn normalized_reference_title_quote_reaches_document_text() {
    // The exact source was run through the fixed oracle before the assertion.
    // CVS mdoc_validate.c::post_rs sets quote_T when %J is present;
    // mdoc_html.c::mdoc__x_pre/post encloses the %T field in curly quotes.
    let source = b".Dd September 27, 2026\n.Dt NORMALIZED-SCOPE 1\n.Os\n.Sh SYNOPSIS\n.Nm normalized-scope\n.Sh AUTHORS\n.An -split\n.An Ada\n.An Babbage\n.Sh SEE ALSO\n.Rs\n.%A Ada\n.%T Title\n.%J Journal\n.Re\n";
    let document = parse_manual_bytes(std::path::Path::new("normalized-scope.1"), source)
        .expect("lower normalized reference");
    let section = document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SEE ALSO")
        .expect("reference section");
    let text = section
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            _ => String::new(),
        })
        .collect::<String>();
    assert!(text.contains("“Title”"), "{text:?}");
}

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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak))
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
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak))
                .count(),
            breaks,
            "{label}: {children:?}"
        );
    }

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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!(
                "{label}: unexpected nested-control blocks: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak))
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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), "\nB C", "{label}: {children:?}");
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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }

    let document = parse_manual_bytes(
        std::path::Path::new("formatter-transparent-target.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\n.ti 4n\n.Tg mark\n.ti 4n\n.No B\n",
    )
    .expect("parse formatter boundaries around a transparent target");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "unexpected target-boundary blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(children), "A\nB", "{children:?}");
    assert_eq!(
        children
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak))
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
        let blocks = &document.sections[0].blocks;
        assert_eq!(blocks.len(), 3, "{label}: {blocks:#?}");
        assert!(matches!(
            blocks[0],
            Block::Paragraph { ref children, .. }
                | Block::Preformatted { ref children, .. }
                if inline_text(children) == "A"
        ));
        assert!(matches!(blocks[1], Block::VerticalSpace { lines: 1, .. }));
        assert!(matches!(
            blocks[2],
            Block::Paragraph { ref children, .. }
                | Block::Preformatted { ref children, .. }
                if inline_text(children) == "B"
        ));
        assert!(!visible_document_text(&document).contains("bogus"));
    }

    let nested = parse_manual_bytes(
        std::path::Path::new("vertical-space-invalid-nested.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\n.sp bogus\n.No B\n.Ec\n",
    )
    .expect("parse nested invalid vertical-space operand");
    let [Block::Paragraph { children, .. }] = nested.sections[0].blocks.as_slice() else {
        panic!("unexpected nested spacing blocks: {:#?}", nested.sections);
    };
    assert_eq!(inline_text(children), "[A\n\nB", "{children:?}");

    let armed = parse_manual_bytes(
        std::path::Path::new("vertical-space-armed-zero.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\z\n.sp 1\n.No B C\n",
    )
    .expect("parse vertical space after an armed zero-advance escape");
    let [
        Block::VerticalSpace { lines: 1, .. },
        Block::Paragraph { children, .. },
    ] = armed.sections[0].blocks.as_slice()
    else {
        panic!("unexpected armed spacing blocks: {:#?}", armed.sections);
    };
    assert_eq!(inline_text(children), "BC", "{children:?}");

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
        ] = document.sections[0].blocks.as_slice()
        else {
            panic!(
                "unexpected leading-break spacing blocks: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), "B C", "{label}: {children:?}");
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
            document.sections
        );
        let blocks = &document.sections[0].blocks;
        let list_index = blocks
            .iter()
            .position(|block| matches!(block, Block::List { .. }))
            .expect("retain the structural list");
        let after_index = blocks
            .iter()
            .position(|block| {
                matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "AFTER LAST")
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
            document.sections
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
        let children = match document.sections[0].blocks.as_slice() {
            [Block::Paragraph { children, .. }] if opening.is_empty() => children,
            [Block::Preformatted { children, .. }] => children,
            blocks => panic!("{label}: unexpected blocks: {blocks:#?}"),
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
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
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-nested-vertical-space.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\n.sp 2\n.No B\n.Ec\n",
    )
    .expect("parse nested vertical-space fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "unexpected nested vertical-space blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(children), "[A\n\n\nB", "{children:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-repeated-margin-flush.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No A\\c\n.mc |\n.Tg between-margins\n.mc !\n.No B\n.Ed\n",
    )
    .expect("parse repeated margin flush fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "unexpected repeated-margin blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(children), "A B", "{children:?}");
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
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
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
        let children = match document.sections[0].blocks.as_slice() {
            [Block::Paragraph { children, .. }] if label != "literal" => children,
            [Block::Preformatted { children, .. }] if label == "literal" => children,
            blocks => panic!("{label}: unexpected blocks: {blocks:#?}"),
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak))
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
            let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
                panic!("{operand_label}/{boundary_label}: {:#?}", document.sections);
            };
            assert_eq!(
                inline_text(children),
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
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("unexpected kept Bx blocks: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "BSD\nAFTER LAST", "{children:?}");
    assert_eq!(
        children
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak))
            .count(),
        1,
        "{children:?}"
    );
}

#[test]
fn no_fill_execution_state_crosses_transparent_requests_only() {
    for (label, request) in [("font", ".ft B"), ("paragraph-distance", ".PD 1")] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n{request}\nB\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-no-fill-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse transparent no-fill request fixture");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected one preformatted block: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), "AB", "{label}: {children:?}");
        if label == "font" {
            assert!(children.iter().any(|inline| matches!(
                inline,
                Inline::Strong { children } if inline_text(children) == "B"
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
        let text = document.sections[0]
            .blocks
            .iter()
            .map(|block| match block {
                Block::Preformatted { children, .. } | Block::Paragraph { children, .. } => {
                    inline_text(children)
                }
                Block::VerticalSpace { .. } => "\n".to_owned(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("AX"), "{label}: {:#?}", document.sections);
        assert!(text.contains('B'), "{label}: {:#?}", document.sections);
        assert!(!text.contains("AB"), "{label}: {:#?}", document.sections);
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-no-fill-margin-character.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n.mc |\nB\n.fi\n",
    )
    .expect("parse no-break margin-character fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "AX B", "{children:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-no-fill-margin-pending-cell.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\n\\zX\\c\n.mc |\nB\n.fi\n",
    )
    .expect("parse no-break margin pending-cell fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "X B", "{children:?}");

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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
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
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
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
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!(
            "expected literal and filled blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(literal), "AX", "{literal:?}");
    assert_eq!(inline_text(filled), "B", "{filled:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-zero-advance-at-eof.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c",
    )
    .expect("parse no-fill EOF fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "AX", "{children:?}");
}

#[test]
fn native_body_posts_settle_no_fill_rows_in_their_output_owner() {
    for (label, operand, expected) in [
        ("zero", "\\zX", "X"),
        ("continued-zero", "\\zX\\c", "X"),
        ("continued-word-end", "\\p\\c", ""),
    ] {
        // Exact inputs checked against the pinned CVS terminal and lint.
        // mdoc_term.c::termp_it_post() calls term_newln() at It BODY exit;
        // man_term.c::post_RS() does the same at RS BODY exit.  Both settle
        // the active row before the following outer `Y` formatter word.
        let mdoc = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh DESCRIPTION\n.nf\n.Bl -item -compact\n.It\n{operand}\n.El\nY\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("mdoc-item-post-{label}.1")),
            mdoc.as_bytes(),
        )
        .expect("parse mdoc list row fixture");
        let [
            Block::List { items, .. },
            Block::Preformatted {
                children: outer, ..
            },
        ] = document.sections[1].blocks.as_slice()
        else {
            panic!("{label}: unexpected mdoc blocks: {:#?}", document.sections);
        };
        let [
            Block::Preformatted {
                children: inner,
                source: Some(inner_source),
                ..
            },
        ] = items[0].blocks.as_slice()
        else {
            panic!("{label}: row left its It BODY: {:#?}", items[0].blocks);
        };
        assert_eq!(inline_text(inner), expected, "{label}: {inner:?}");
        assert_eq!(inner_source.line, 11, "{label}: {inner_source:?}");
        assert_eq!(inline_text(outer), "Y", "{label}: {outer:?}");

        let man = format!(
            ".TH TEST 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.nf\n.RS\n{operand}\n.RE\nY\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("man-rs-post-{label}.1")),
            man.as_bytes(),
        )
        .expect("parse man relative-indent row fixture");
        let [
            Block::Preformatted {
                children: inner,
                layout: inner_layout,
                source: Some(inner_source),
                ..
            },
            Block::Preformatted {
                children: outer, ..
            },
        ] = document.sections[0].blocks.as_slice()
        else {
            panic!("{label}: unexpected man blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(inner), expected, "{label}: {inner:?}");
        assert_eq!(inner_source.line, 5, "{label}: {inner_source:?}");
        assert_eq!(inner_layout.indent_columns, 7, "{label}: {inner_layout:?}");
        assert_eq!(inline_text(outer), "Y", "{label}: {outer:?}");
    }
}

#[test]
fn mdoc_column_body_posts_keep_pending_glyphs_in_their_cells() {
    for (label, row, first, second) in [
        ("first", ".It \\zX\\c Ta Z", "X", "Z"),
        ("last", ".It Q Ta \\zX\\c", "Q", "X"),
    ] {
        // Exact inputs checked with fixed CVS -Tascii and -Tlint.  For
        // LIST_column, mdoc_term.c::termp_it_post() flushes each BODY's
        // active formatter cell before the next one owns the row.
        let manual = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh DESCRIPTION\n.nf\n.Bl -column A B -compact\n{row}\n.El\nY\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("mdoc-column-post-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse mdoc column row fixture");
        let [
            Block::Table { rows, .. },
            Block::Preformatted {
                children: outer, ..
            },
        ] = document.sections[1].blocks.as_slice()
        else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        let cells = &rows[0].cells;
        assert_eq!(cells.len(), 2, "{label}: {cells:?}");
        for (cell, expected) in cells.iter().zip([first, second]) {
            let [Block::Preformatted { children, .. }] = cell.blocks.as_slice() else {
                panic!("{label}: pending glyph left its cell: {cell:?}");
            };
            assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        }
        assert_eq!(inline_text(outer), "Y", "{label}: {outer:?}");
    }
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
fn bibliography_field_posts_emit_native_punctuation_and_author_conjunction() {
    // These exact inputs were checked with fixed CVS -Tascii before writing
    // the assertions. mdoc_term.c::termp__a_pre()/termp____post() emit `and`,
    // commas and a final period at the field's own execution point;
    // termp_under_pre() styles %J and an unquoted %T.
    let authors = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh SEE ALSO\n.Rs\n.%A Ada\n.%A Babbage\n.%T Title\n.Re\nY\n";
    let document = parse_manual_bytes(std::path::Path::new("mdoc-rs-authors.1"), authors)
        .expect("parse author reference fixture");
    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("unexpected reference output: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "Ada and Babbage, Title. Y");
    assert!(children.iter().any(|inline| {
        matches!(inline, Inline::Emphasis { children } if inline_text(children) == "Title")
    }));

    let journal = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh SEE ALSO\n.Rs\n.%A Ada\n.%T Title\n.%J Journal\n.Re\nY\n";
    let document = parse_manual_bytes(std::path::Path::new("mdoc-rs-journal.1"), journal)
        .expect("parse journal reference fixture");
    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("unexpected journal output: {:#?}", document.sections);
    };
    assert!(inline_text(children).contains("Ada, “Title”, Journal. Y"));
    assert!(children.iter().any(|inline| {
        matches!(inline, Inline::Emphasis { children } if inline_text(children) == "Journal")
    }));
}

#[test]
fn bibliography_pre_and_post_obey_no_fill_source_rows() {
    // Both exact inputs were run with the fixed CVS -Tascii and -Tlint oracle.
    // mdoc_term.c::print_mdoc_node() applies NODE_LINE before termp__a_pre(),
    // while termp____post() writes a real word that consumes the field's \c.
    for (label, fields, expected) in [
        (
            "authors",
            ".%A Ada\n.%A Babbage\n.%T Title\n",
            "Ada\nand Babbage,\nTitle.\nY",
        ),
        (
            "post-continuation",
            ".%A Ada\\c\n.%T Title\n",
            "Ada,\nTitle.\nY",
        ),
    ] {
        let manual = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh SEE ALSO\n.nf\n.Rs\n{fields}.Re\nY\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("mdoc-rs-{label}.1")),
            manual.as_bytes(),
        )
        .unwrap();
        let blocks = &document.sections[1].blocks;
        assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children) == expected)), "{label}: {blocks:#?}");
    }
}

#[test]
fn bibliography_entry_preserves_continuation_and_see_also_spacing() {
    // Exact inputs checked with pinned CVS -Tascii/-Tlint. The NODE_LINE
    // check in mdoc_term.c::print_mdoc_node() respects TERMP_NONEWLINE on Rs;
    // termp_rs_pre() inserts vspace between adjacent SEE ALSO references.
    let entry = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh DESCRIPTION\n.nf\n\\zX\\c\n.Rs\nY\n.Re\nZ\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("rs-continuation.1"), entry).unwrap();
    assert!(document.sections[1].blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children) == "Y\nZ")), "{:#?}", document.sections[1].blocks);

    let adjacent = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd state probe\n.Sh SEE ALSO\n.Rs\n.%A Ada\n.%T One\n.Re\n.Rs\n.%A Bob\n.%T Two\n.Re\n";
    let document = parse_manual_bytes(std::path::Path::new("rs-adjacent.1"), adjacent).unwrap();
    let blocks = &document.sections[1].blocks;
    assert!(blocks.windows(3).any(|parts| matches!(parts, [Block::Paragraph { children: first, .. }, Block::VerticalSpace { lines: 1, .. }, Block::Paragraph { children: second, .. }] if inline_text(first) == "Ada, One." && inline_text(second) == "Bob, Two.")), "{blocks:#?}");
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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        let target = children.iter().find_map(|inline| match inline {
            Inline::Link { target, .. } => Some(target),
            _ => None,
        });
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
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{macro_name}: expected one paragraph");
        };
        assert_eq!(
            inline_text(children),
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
