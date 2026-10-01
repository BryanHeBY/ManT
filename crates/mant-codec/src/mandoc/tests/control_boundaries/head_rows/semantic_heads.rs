use super::*;

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
                blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains(&format!("<{target}> after")))),
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
    // This exact input was rerun before updating the assertion. The TAG
    // overruns its field: term_newln() ends the row and term_vspace() adds
    // a separate empty row. The semantic alternative removes one boundary,
    // leaving that executed vertical row attached to the preceding term.
    assert_eq!(inline_text(&item.terms[0]), "first\n");
    assert_eq!(inline_text(&item.terms[1]), "second");

    // Each Pp asserts a separate vertical row even when an intervening bare
    // \z has not occupied a formatter cell. The later Pp must still split
    // the following term. Exact source rerun with fixed CVS before these
    // assertions; mdoc_term.c::termp_pp_pre executes each Pp in order.
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
    assert_eq!(inline_text(&item.terms[0]), "first\n");
    assert!(inline_text(&item.terms[1]).contains("cond"), "{item:#?}");
    assert_eq!(inline_text(&item.terms[2]), "third");
    assert_eq!(
        item.terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>()
            .join("\n"),
        "first\n\n\necond\n\nthird",
        "each actual vertical row survives its semantic alternative: {item:#?}"
    );
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
