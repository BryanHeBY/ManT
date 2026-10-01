use super::*;

// The selected UTF-8 formatter preserves CVS's executed glyphs: mdoc Ao/Ac
// select the Unicode angle glyphs, while man post_UR always calls term_word
// with literal ASCII "<"/">" (man_term.c:896-908). These assertions were
// rechecked against pristine ASCII/UTF-8/tree/lint before synchronizing them.

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
fn no_fill_end_marker_shares_scope_post_state() {
    // Exact input checked against fixed CVS tree, HTML, terminal, and lint.
    // mdoc.c::mdoc_endbody_alloc links Ac to Ao's BODY, and
    // mdoc_term.c::print_mdoc_node marks that BODY ended at the marker.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Dl marker\n.nf\n.Bo\ninside\n.Ac\nafter\n.Bc\n.fi\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("nofill-scope-post.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert_eq!(text.matches('⟩').count(), 1, "{text:?}");
    assert_eq!(text.matches(']').count(), 1, "{text:?}");
    assert!(text.contains("inside\n⟩"), "{text:?}");

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
        ("Ao", b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Ao\nfirst\nsecond\n.Ac\n.fi\n".as_slice(), "⟨\nfirst\nsecond⟩"),
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
    assert!(!text.contains("X⟩"), "{text:?}");
    assert!(text.contains('⟩'), "{text:?}");

    // Exact reverse input checked with CVS -Tutf8. Eo's authored Ec tail is
    // a separate NODE_LINE, so X must be committed on its own row instead
    // of being consumed by CLOSE.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Eo OPEN\n\\zX\n.Ec CLOSE\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("nofill-eo-zero.1"), source).unwrap();
    let text = projected_document_text(&document);
    assert!(text.contains("OPEN\nX\nCLOSE"), "{text:?}");
}

#[test]
fn crossed_display_end_does_not_undo_later_no_fill_request() {
    // Exact input checked with pinned CVS -Ttree/-Tutf8. mdoc_macro.c's
    // blk_exp_close restores Bd fill at Ed's source position, before nf.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bd -literal\n.Bo\ninside\n.Ed\n.nf\nafter\n.Bc\n.Ac\n.fi\ntail\n";
    let document = parse_manual_bytes(std::path::Path::new("crossed-ed-nf.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("after]⟩"))), "{blocks:#?}");
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
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).starts_with("after⟩"))), "{blocks:#?}");

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
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains('⟩'))), "{blocks:#?}");
    assert!(!blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains('⟩'))), "{blocks:#?}");
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
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("before\n⟨\ninside"))), "{blocks:#?}");

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
    assert!(text.contains("inside⟩"), "{text:?}");
    assert!(!text.contains("inside\n⟩"), "{text:?}");
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
    assert!(text.contains("inside\n⟩"), "{text:?}");
}

#[test]
fn display_local_no_fill_does_not_change_parent_enclosure_channel() {
    // Exact input checked against pinned CVS -Thtml and -Tutf8.
    // mdoc_macro.c::blk_exp_close restores the pre-display fill mode;
    // the following Ao text and post return to the incoming fill channel.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bd -literal\n.nf\ninside\n.Ed\nnext\n.Ac\n";
    let document = parse_manual_bytes(std::path::Path::new("bd-local-nofill.1"), source).unwrap();
    let blocks = &document.sections[0].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children).contains("next⟩"))), "{blocks:#?}");
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
    assert_eq!(text.matches('⟩').count(), 1, "{text:?}");
    assert!(text.contains("inside⟩ after-ac"), "{text:?}");
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
    assert!(text.contains("[inside⟩ after-angle]"), "{text:?}");
    assert_eq!(text.matches('⟩').count(), 1, "{text:?}");
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
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert_eq!(
            children
                .iter()
                .filter(|node| matches!(node, Inline::LineBreak { .. }))
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
                .filter(|node| matches!(node, Inline::LineBreak { .. }))
                .count(),
            breaks,
            "{label}: {children:?}"
        );
    }
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
        // active formatter cell before the next one owns the row. Non-last
        // BODYs retain native trailspace=1 as minbl for the next field,
        // rather than adding unprinted padding to the first semantic cell.
        // Declared columns consume that geometry; no-fill never changes the
        // cell's text executor (term.c:233-237, term_field():409-435).
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
            let [Block::Paragraph { children, .. }] = cell.blocks.as_slice() else {
                panic!("{label}: pending glyph left its cell: {cell:?}");
            };
            assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        }
        assert_eq!(inline_text(outer), "Y", "{label}: {outer:?}");
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
