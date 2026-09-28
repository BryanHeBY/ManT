use super::*;

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
    // A nonprinting BODY is still executed, but CVS man_html.c::man_UR_pre()
    // chooses HEAD text when there is no printable label.
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
    assert!(text.contains("https://example.com after"), "{document:#?}");
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
    assert!(text.contains("https://example.com after"), "{document:#?}");
    assert!(
        !strong_document_text(&document).contains("after"),
        "{document:#?}"
    );
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
