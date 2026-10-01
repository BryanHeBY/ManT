use super::*;

// The selected UTF-8 formatter preserves CVS's executed glyphs: mdoc Ao/Ac
// select the Unicode angle glyphs, while man post_UR always calls term_word
// with literal ASCII "<"/">" (man_term.c:896-908). These assertions were
// rechecked against pristine ASCII/UTF-8/tree/lint before synchronizing them.

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
    assert!(strong.contains('⟩'), "{document:#?}");
    assert!(strong.contains("after"), "{document:#?}");
    assert!(strong.contains("tail"), "{document:#?}");
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
