use super::*;

// The selected UTF-8 formatter preserves CVS's executed glyphs: mdoc Ao/Ac
// select the Unicode angle glyphs, while man post_UR always calls term_word
// with literal ASCII "<"/">" (man_term.c:896-908). These assertions were
// rechecked against pristine ASCII/UTF-8/tree/lint before synchronizing them.

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
                text.contains(&format!("label{separator}<{target}>")),
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
        text.contains("<https://example.com> after"),
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
        text.contains("<https://example.com> after"),
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
        ("prefix\\c", "prefix<https://example.com>"),
        ("prefix", "prefix\n<https://example.com>"),
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
            let boundary = if expected_break { "label\n<" } else { "label<" };
            assert!(rows.contains(boundary), "{open} {label}: {document:#?}");
            assert!(
                rows.contains(&format!("<{target}>\nafter")),
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
    assert_eq!(inline_text(children), "label\n<x>\nafter");
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
                    format!("second<{target}>\nafter")
                } else {
                    format!("second<{target}> after")
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
        assert!(text.contains("<x> after"), "{name}: {document:#?}");
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

    // These exact BODY controls were rechecked with pristine ASCII/UTF-8/
    // HTML/tree/lint before the assertions below. man_PP_pre/man_IP_pre and
    // the nf paragraph boundary close HTML's initial anchor before BODY's
    // first glyph. Keep its target identity and the native X, but do not
    // move the closed annotation onto that later output owner.
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
        assert!(labels[0].is_empty(), "{name}: {document:#?}");
        assert!(!labels[0].contains('X'), "{name}: {document:#?}");
        assert!(
            projected_document_text(&document).contains('X'),
            "{name}: {document:#?}"
        );
        let visible = projected_document_text(&document);
        let prior = visible.find('X').expect("prior glyph");
        let body = visible.find(retained).expect("retained body");
        let target = visible.find("<outer>").expect("generated terminal target");
        let after = visible.find("after").expect("following body");
        assert!(
            prior < body && body < target && target < after,
            "{name}: {visible:?}"
        );
        assert_eq!(
            document_link_targets(&document),
            [mant_ir::LinkTarget::External {
                uri: "outer".into()
            }]
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
        inline_text(paragraphs[0]).contains("prefix label <https://example.com> suffix"),
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
            projected_document_text(&document).contains("<x>"),
            "{name}: {document:#?}"
        );
    }
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
