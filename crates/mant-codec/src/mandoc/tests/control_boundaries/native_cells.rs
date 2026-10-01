use super::*;

fn plain_receipt_text(body: &str) -> String {
    let source = format!(
        ".Dd October 1, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n{body}.Sh ENDTEST\n.No FINISH\n"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("plain-native-receipt.1"),
        source.as_bytes(),
    )
    .unwrap();
    let mut text = String::new();
    for block in &document.sections[1].blocks {
        if let Block::Paragraph { children, .. } | Block::Preformatted { children, .. } = block {
            let projected = inline_text(children);
            assert!(
                !projected.contains('\0'),
                "private receipt marker: {source}"
            );
            text.push_str(&projected);
        }
    }
    // Device left padding is the frozen responsive geometry difference;
    // retain every physical row and every interior word separator.
    text.split('\n')
        .map(|row| row.trim_start_matches(' '))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn accepted_plain_flushes_project_every_final_native_pass() {
    // All eight exact sources ran pristine UTF-8/ASCII/tree/lint first.
    // roff_term_pre_ta() changes stops without flushing (roff_term.c:217-222).
    // term_flushln() still prints all accepted passes and their row events
    // at retirement (term.c:143-220), including a deferred first pass.
    for (body, expected) in [
        (".No X\\p\n.ta 2n\n.No Y\n.No AFTER\n", "X\nY AFTER"),
        (".No \\zX\\p\n.ta 2n\n.No Y\n.No AFTER\n", "XY\nAFTER"),
        (".No \\p\n.ta 2n\n.No Y\n.No AFTER\n", ""),
        (".No X\\p\n.ta 2n\n.No \\p\n.No Z\n.No AFTER\n", "X"),
        (".Em X\\p\n.ta 2n\n.Sy Y\n.No AFTER\n", "X\nY AFTER"),
        (".No X\\p\n.No Y\n.No AFTER\n", "X\nY AFTER"),
        (
            ".No \"X\\p Y\"\n.ta 2n\n.No \"Z\\p W\"\n.No AFTER\n",
            "X\nY Z\nW AFTER",
        ),
        (
            ".No X\\p\n.ta 2n\n.Lk https://example.org Y\n.No AFTER\n",
            "X\nY: https://example.org AFTER",
        ),
    ] {
        assert_eq!(plain_receipt_text(body), expected, "{body}");
    }
}

#[test]
fn spacing_transitions_use_the_incoming_native_word_boundary() {
    // All exact sources ran pristine before these assertions. termp_sm_pre()
    // updates NONOSPACE, leaving incoming NOSPACE alone; term_word() writes
    // its separator before updating NOSPACE (mdoc_term.c:1820-1835;
    // term.c:573-589). Empty words, BreakMarker and NBRZW still execute that
    // transition, even when their semantic output has no visible glyph.
    for (body, expected) in [
        (".No \\p\n.Sm off\n.No Y\n.Sm on\n.No AFTER\n", ""),
        (
            ".No \\p\\&\n.Sm off\n.No Y\n.Sm on\n.No AFTER\n",
            "\nY AFTER",
        ),
        (
            ".No \\p\n.Sm off\n.Bk -words\n.No Y\n.Ek\n.Sm on\n.No AFTER\n",
            "",
        ),
        (
            ".No \\p\\&\n.Sm off\n.Bk -words\n.No Y\n.Ek\n.Sm on\n.No AFTER\n",
            "\nY AFTER",
        ),
        (".No \\&\n.Sm off\n.No Y\n.Sm on\n.No AFTER\n", "Y AFTER"),
        (
            ".No \\zX\\p\n.Sm off\n.No Y\n.Sm on\n.No AFTER\n",
            "XY\nAFTER",
        ),
        (".No \"\"\n.Sm off\n.No Y\n.Sm on\n.No AFTER\n", "Y AFTER"),
        (".No \\z\n.Sm off\n.No Y\n.Sm on\n.No AFTER\n", "YAFTER"),
        (
            ".No X\n.br\n.Sm off\n.No Y Z\n.Sm on\n.No AFTER\n",
            "X\nYZ AFTER",
        ),
        (".No X\n.Sm off\n.No Y Z\n.Sm on\n.No AFTER\n", "X YZ AFTER"),
    ] {
        assert_eq!(plain_receipt_text(body), expected, "{body}");
    }
}

#[test]
fn no_fill_receipt_retirement_does_not_replay_a_completed_source_row() {
    // Exact pristine runs first: NODE_LINE already executed term_newln()
    // before ta/Sm handler dispatch (mdoc_term.c:314-318). Their words must
    // use the newly retired buffer, retaining an actual NBRZW-only row.
    assert_eq!(
        plain_receipt_text(".nf\n.No X\\p\n.ta 2n\n.No Y\n.No AFTER\n.fi\n"),
        "X\nY\nAFTER"
    );
    assert_eq!(
        plain_receipt_text(".nf\n.No \\p\\&\n.Sm off\n.No Y\n.Sm on\n.No AFTER\n.fi\n"),
        "\nY\nAFTER"
    );
}

#[test]
fn no_fill_words_project_only_their_surviving_native_separator() {
    // Every exact source ran pristine ASCII/UTF-8/tree/lint first. A word
    // buffers its incoming NOSPACE separator before decoding (term.c:573-589).
    // Empty/zero-width words do not erase that cell; BACKBEFORE pops a plain
    // blank or overwrites a KEEP blank (901-908). NODE_LINE retires a row
    // unless NONEWLINE is set (mdoc_term.c:314-318).
    for (body, expected) in [
        (".nf\n.No \\& No B\n.fi\n", " B"),
        (".nf\n.No \"\" No B\n.fi\n", " B"),
        (".nf\n.No \\fB No B\n.fi\n", " B"),
        (".nf\n.No \\z No B\n.fi\n", " B"),
        (".nf\n.No \\zX No B\n.fi\n", "XB"),
        (".nf\n.No \"\" No \"\" No B\n.fi\n", "  B"),
        (".nf\n.No \\&\n.No B\n.fi\n", "\nB"),
        (".nf\n.No \\&\\c\n.No B\n.fi\n", "B"),
        (".nf\n.No \\&\\c\n.Sm off\n.No B\n.fi\n", "B"),
        (".nf\n.Bk -words\n.No \\zX No B\n.Ek\n.fi\n", "XB"),
        (".nf\n.An \"\" An B\n.fi\n", " B"),
        (".nf\n.No \\& Aq B\n.fi\n", " ⟨B⟩"),
    ] {
        let source = format!(
            ".Dd October 1, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n{body}.Sh ENDTEST\n.No FINISH\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("native-word-separator.1"),
            source.as_bytes(),
        )
        .unwrap();
        let actual: String = document.sections[1]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Preformatted { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .collect();
        assert_eq!(actual, expected, "{source}\n{document:#?}");
    }
    // Filled initial padding remains the frozen responsive G-IND difference.
    // The pristine inputs also ran first; keep all interior word boundaries.
    assert_eq!(plain_receipt_text(".No \\& No B\n"), "B");
    assert_eq!(plain_receipt_text(".No \"\" No B\n"), "B");
    assert_eq!(plain_receipt_text(".No \\zX No B\n"), "XB");
}

#[test]
fn generated_man_link_words_share_no_fill_native_separators() {
    // Exact pristine sources ran first. post_UR() always executes the '<'
    // word, including after control-only BODY words (man_term.c:896-910).
    // The separator is a shared word receipt, never a link-specific patch.
    for (start, end) in [("UR", "UE"), ("MT", "ME")] {
        for (body, expected) in [
            ("\\&\n", " <https://example.org>\nafter"),
            ("\\&\\c\n", "<https://example.org>\nafter"),
            ("\\z\n", " https://example.org>\nafter"),
            ("\\zX\n", "X<https://example.org>\nafter"),
            ("\\fB\n", " <https://example.org>\nafter"),
            ("", "<https://example.org>\nafter"),
        ] {
            let source = format!(
                ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.nf\n.{start} https://example.org\n{body}.{end}\nafter\n.fi\n.SH ENDTEST\nFINISH\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("generated-native-word-separator.1"),
                source.as_bytes(),
            )
            .unwrap();
            let actual: String = document.sections[0]
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Preformatted { children, .. } => Some(inline_text(children)),
                    _ => None,
                })
                .collect();
            assert_eq!(actual, expected, "{source}\n{document:#?}");
        }
    }
}

#[test]
fn invisible_native_graph_preserves_following_accepted_field() {
    // Exact fixture ran through pinned pristine CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_fill() treats ASCII_NBRZW as graph despite zero width;
    // the second accepted pass ends its own physical row before Y prints.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No X\\p\n.No \"\\p\\&\"\n.No Y\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X\n\nY", "{item:#?}");
    assert!(
        matches!(&item.description[0], Block::Paragraph { children, .. }
        if inline_text(children) == "BodyWord"),
        "{item:#?}"
    );
}

#[test]
fn first_native_pass_rejection_drops_only_its_unprinted_field() {
    // Exact fixture ran through pinned pristine CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_flushln() stops on first nbr=0 and clears that buffer,
    // including a \z glyph already written by encode1(). BODY is a new field.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No \\p\n.No \\zY\n.No Z\n.Xc\n.No BodyWord\n.El\n",
    );
    assert!(
        item.terms.iter().all(|term| inline_text(term).is_empty()),
        "{item:#?}"
    );
    assert!(
        matches!(&item.description[0], Block::Paragraph { children, .. }
        if inline_text(children) == "BodyWord"),
        "{item:#?}"
    );
}

#[test]
fn head_rejection_cannot_revoke_a_body_after_a_real_flush() {
    // Exact fixture ran through pinned pristine CVS -Tascii/-Tutf8/-Tlint.
    // roff_term_pre_br() term_newln() consumes the old HEAD buffer. Rejected
    // HEAD bytes and the later BodyWord cannot share one rejection interval.
    let item = definition_item_from_source(
        ".Bl -inset\n.It Xo\n.No \"X\\p \\p Y\"\n.Xc\n.br\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "\nBodyWord")),
        "{item:#?}"
    );
}

#[test]
fn overwritten_native_graphs_do_not_keep_a_rejected_owned_suffix() {
    // Exact CVS profiles retain the last overstrike glyph C, reject Z, and
    // retain BodyWord. encode1() keeps A/B as native graph but backspaces
    // their positions; source-cell acceptance cannot count them as IR glyphs.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No \"\\zA\\zBC \\p Z\"\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "C", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")),
        "{item:#?}"
    );
    assert_no_private_field_markers(&item);
}

#[test]
fn explicit_flush_cannot_delete_a_prefix_already_accepted_by_an_earlier_pass() {
    // Exact CVS profiles keep X and BodyWord, reject Y. term_flushln()
    // consumes accepted passes before rejecting the remaining buffer; a
    // subsequent roff_term_pre_br() cannot revoke that committed prefix.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No \"X\\p \\p Y\"\n.br\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")),
        "{item:#?}"
    );
    assert_no_private_field_markers(&item);
}

#[test]
fn native_acceptance_ranges_survive_link_and_style_wrappers() {
    // Exact CVS profiles keep X/Y on separate rows and reject Z, including
    // when Lk underlining owns both passes. Macro-generated URI spelling
    // executes after the label; typed Link identity is presentation metadata.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.Lk https://example.org \"X\\p Y\" \"\\p Z\"\n.Xc\n.No BodyWord\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X\nY", "{item:#?}");
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")),
        "{item:#?}"
    );
    assert_no_private_field_markers(&item);
}

#[test]
fn portable_hidden_source_operand_retains_its_native_owner_range() {
    // Exact pristine CVS profiles retain X and URI, reject later Z.
    // Portable Markdown suppresses the executed suffix, while native readers
    // retain its accepted interval. Neither projection may revive rejected Z.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.Lk \"https://example.org\\p \\p\" X\n.No Z\n.Xc\n.No BodyWord\n.El\n",
    );
    // This exact source was rerun before changing the carrier assertion:
    // CVS prints X/URI and BodyWord on adjacent rows. The pure identity has
    // an invalid trailing blank, so export must not hide its native URI.
    // With that suffix visible, the terminal HEAD close transfers once to
    // Separate instead of being obscured inside a PortableDisplay subtree.
    assert_eq!(
        inline_text(&item.terms[0]),
        "X: https://example.org",
        "{item:#?}"
    );
    assert_eq!(item.layout.head_body_relation, HeadBodyRelation::Separate);
    assert!(
        item.description.iter().any(|block| matches!(block,
        Block::Paragraph { children, .. } if inline_text(children) == "BodyWord")),
        "{item:#?}"
    );
    assert_no_private_field_markers(&item);
}

fn assert_no_private_field_markers(item: &mant_ir::DefinitionItem) {
    fn check(nodes: &[Inline]) {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. } => assert!(!id.as_str().starts_with('\0')),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. }
                | Inline::PortableDisplay { children, .. } => check(children),
                _ => {}
            }
        }
    }
    for term in &item.terms {
        check(term);
    }
    for block in &item.description {
        if let Block::Paragraph { children, .. } | Block::Preformatted { children, .. } = block {
            check(children);
        }
    }
}
