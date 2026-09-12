use super::{
    ASCII_BREAK, ASCII_HYPH, ASCII_NBRSP, PresentationKind, RoffFont, RoffInlineEvent, decode,
    visible_text,
};

#[test]
fn emits_text_font_and_renderer_link_events() {
    assert_eq!(
        decode(r"\X'tty: link https://example.test'\fB\-h\fR\X'tty: link' FILE"),
        vec![
            RoffInlineEvent::Link(Some("https://example.test".to_owned())),
            RoffInlineEvent::Font(RoffFont::Strong),
            RoffInlineEvent::Text("-h".to_owned()),
            RoffInlineEvent::Font(RoffFont::Regular),
            RoffInlineEvent::Link(None),
            RoffInlineEvent::Text(" FILE".to_owned()),
        ]
    );
}

#[test]
fn keeps_no_space_distinct_from_generic_presentation_state() {
    assert_eq!(
        decode(r"\z\c"),
        vec![RoffInlineEvent::ZeroAdvance, RoffInlineEvent::NoSpace]
    );
}

#[test]
fn recognizes_constant_width_and_pandoc_verbatim_font_families() {
    assert_eq!(
        decode(r"\f[C]code\f[V]verbatim\f[VB]bold\f[VI]italic\f[R]"),
        vec![
            RoffInlineEvent::Font(RoffFont::Code),
            RoffInlineEvent::Text("code".to_owned()),
            RoffInlineEvent::Font(RoffFont::Code),
            RoffInlineEvent::Text("verbatim".to_owned()),
            RoffInlineEvent::Font(RoffFont::CodeStrong),
            RoffInlineEvent::Text("bold".to_owned()),
            RoffInlineEvent::Font(RoffFont::CodeEmphasis),
            RoffInlineEvent::Text("italic".to_owned()),
            RoffInlineEvent::Font(RoffFont::Regular),
        ]
    );
}

#[test]
fn consumes_every_supported_argument_shape_as_typed_presentation_state() {
    let events = decode(r"\mX\m(bl\m[blue]\s2\s-2\s(12\s[+12]\s'+3'");
    let presentations = events
        .into_iter()
        .filter_map(|event| match event {
            RoffInlineEvent::Presentation { kind, argument } => Some((kind, argument)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(presentations.len(), 8);
    assert_eq!(presentations[0].1.as_deref(), Some("X"));
    assert_eq!(presentations[1].1.as_deref(), Some("bl"));
    assert_eq!(presentations[2].1.as_deref(), Some("blue"));
    assert!(
        presentations[..3]
            .iter()
            .all(|(kind, _)| *kind == PresentationKind::Color)
    );
    assert_eq!(
        presentations[3..]
            .iter()
            .map(|(_, argument)| argument.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("2"), Some("-2"), Some("12"), Some("+12"), Some("+3")]
    );
}

#[test]
fn signed_legacy_size_consumes_one_digit_before_visible_text() {
    assert_eq!(
        decode(r"\s-20000"),
        vec![
            RoffInlineEvent::Presentation {
                kind: PresentationKind::PointSize,
                argument: Some("-2".to_owned()),
            },
            RoffInlineEvent::Text("0000".to_owned()),
        ]
    );
    assert_eq!(visible_text(r"\s+300ff"), "00ff");
    assert_eq!(visible_text(r"\s20000"), "000");
}

#[test]
fn normalizes_internal_markers_and_known_zero_width_controls() {
    let source = format!("git{ASCII_HYPH}config{ASCII_NBRSP}(1){ASCII_BREAK}next\\&.\\|.\\|.");

    assert_eq!(visible_text(&source), "git-config (1)next...");
}

#[test]
fn renders_the_cvs_utf8_device_escape_as_visible_text() {
    // CVS roff_escape.c recognizes `\\*[.T]` as ESCAPE_DEVICE and term.c
    // emits `utf8` for its UTF-8 output device.  Keep that renderer-visible
    // value while unresolved ordinary string requests remain formatter state.
    assert_eq!(visible_text(r"device=\*[.T]"), "device=utf8");
    assert_eq!(visible_text(r"string=\*[unknown]"), "string=");
}

#[test]
fn decodes_bracketed_unicode_and_composite_character_names() {
    assert_eq!(
        visible_text(r"Ma\[u0161]l\[u00E1] \[u2014] \[u01F642]"),
        "Mašlá — 🙂"
    );
    assert_eq!(visible_text(r"\[u0061_0301]"), "a\u{301}");
    assert_eq!(visible_text(r"Dole\[vc]ek"), "Doleček");
}

#[test]
fn decodes_documented_groff_default_composite_glyphs() {
    // groff_char(7) documents `\\[base accent ...]`; the installed
    // composite.tmac maps `ad` to a combining diaeresis and permits multiple
    // accents.  Preserve NFD rather than relying on a presentation-only
    // precomposed substitute.
    assert_eq!(
        visible_text(r"re\[e ad]nabled caf\[e aa] a\[a aa ac]"),
        "re\u{0065}\u{0308}nabled caf\u{0065}\u{0301} a\u{0061}\u{0301}\u{0327}"
    );
    // A user-defined or malformed composite mapping is not part of groff's
    // default table; retain the authored spelling rather than guessing.
    assert_eq!(visible_text(r"\[e unknown]"), r"\[e unknown]");
}

#[test]
fn retains_invalid_unicode_names_as_visible_fallbacks() {
    assert_eq!(
        visible_text(r"\[uD800] \[u110000] \[u12]"),
        r"\[uD800] \[u110000] \[u12]"
    );
}

#[test]
fn preserves_literal_positive_horizontal_motion_as_a_word_boundary() {
    assert_eq!(visible_text(r"1.\h'+01'\c"), "1. ");
    assert_eq!(visible_text(r"a\h'1n'b"), "a b");
    assert_eq!(visible_text(r"a\h'.5m'b"), "a b");
    assert_eq!(visible_text(r"a\h'-04'b"), "ab");
    assert_eq!(visible_text(r"a\h'+0'b"), "ab");
    assert_eq!(visible_text(r"a\h'|1i'b"), "ab");
    assert_eq!(visible_text(r"a\h'\n[x]'b"), "ab");
}

#[test]
fn retains_legacy_sphinx_empty_destinations_as_typed_evidence() {
    let source = r"btrfs-subvolume(8) \%<>";

    assert_eq!(
        decode(source),
        vec![
            RoffInlineEvent::Text("btrfs-subvolume(8) ".to_owned()),
            RoffInlineEvent::EmptyDestination,
        ]
    );
    assert_eq!(visible_text(source), "btrfs-subvolume(8) <>");
    assert_eq!(visible_text(r"literal \%value"), "literal value");
}

#[test]
fn preserves_roff_reverse_solidus_characters_in_windows_paths() {
    assert_eq!(
        visible_text(r"C:\[rs]path\[rs]file \[rs]\[rs]server\[rs]share"),
        r"C:\path\file \\server\share",
    );
}

#[test]
fn resolves_named_characters_from_the_pinned_mandoc_catalog() {
    assert_eq!(
        visible_text(
            r"at=\(at ga=\(ga oq=\(oq cq=\(cq lq=\(lq rq=\(rq arrow=\(-> larrow=\(<- mu=\(mu lB=\(lB rB=\(rB"
        ),
        "at=@ ga=` oq=‘ cq=’ lq=“ rq=” arrow=→ larrow=← mu=× lB=[ rB=]"
    );
    assert_eq!(visible_text(r"zero=\[:]width"), "zero=width");
    assert_eq!(visible_text(r"\`left\'right \_"), "`left´right _");
}

#[test]
fn preserves_documented_groff_caron_spellings_absent_from_mandoc() {
    // groff_char(7) documents the S/s and Z/z forms.  The pinned CVS table
    // does not know them, while generated Slovene manuals use the compact
    // two-character form.
    assert_eq!(visible_text(r"\(vC\(vc \(vS\(vs \(vZ\(vz"), "Čč Šš Žž");
}

#[test]
fn retains_unknown_named_characters_in_a_visible_source_form() {
    assert_eq!(
        visible_text(r"a=\(zz b=\[future-glyph] c=\C'other'"),
        r"a=\(zz b=\[future-glyph] c=\C'other'"
    );
}

#[test]
fn malformed_and_undefined_escapes_are_bounded_and_predictable() {
    assert_eq!(visible_text("alpha\\m[unterminated"), "alpha");
    assert_eq!(visible_text("alpha\\"), "alpha\\");
    assert_eq!(visible_text(r"alpha\qbeta"), "alphaqbeta");
    assert_eq!(visible_text(r"\EfBbold\EfR"), "bold");
    assert_eq!(visible_text(r"before\N1after"), r"before\N1after");
    assert_eq!(visible_text(r"before\zXafter"), "beforeafter");
}

#[test]
fn numbered_glyphs_follow_the_terminal_range_not_unicode_indices() {
    for (source, expected) in [
        (r"\N'65'", "A"),
        (r"\N|65|", "A"),
        (r"\N'255'", "ÿ"),
        (r"\N'0'", " "),
        (r"\N'27'", " "),
    ] {
        assert_eq!(visible_text(source), expected, "{source}");
    }
    for source in [
        r"\N'256'",
        r"\N'128512'",
        r"\N'-1'",
        r"\N'abc'",
        r"\N'65",
        r"\N'999999999999999999999'",
        r"\N1",
    ] {
        assert_eq!(visible_text(source), source);
    }
}

#[test]
fn zero_advance_discards_complete_overstrike_glyphs_without_recursion() {
    for source in [r"\z\(emTOKENB", r"\z\[em]TOKENB", r"\z\C'em'TOKENB"] {
        assert_eq!(visible_text(source), "TOKENB", "{source}");
    }
    assert_eq!(visible_text(r"\z\[future-glyph]"), "");
    for source in [
        r"A\z\fBXB\fP END",
        r"A\z\m[red]XB\m[] END",
        r"A\z\s[12]XB\s0 END",
    ] {
        assert_eq!(visible_text(source), "AB END", "{source}");
    }
    assert_eq!(visible_text(r"\z"), "");
    assert_eq!(visible_text("TOKEN\\zX\nNEXT"), "TOKENX\nNEXT");
    assert_eq!(visible_text(&format!("{}Y", r"\zX".repeat(20_000))), "Y");
}

#[test]
fn copy_mode_escape_chains_are_decoded_iteratively() {
    let source = format!(r"\E{}fBbold\EfR", "E".repeat(16_384));

    assert_eq!(visible_text(&source), "bold");
}

#[test]
fn masks_terminal_controls_in_source_and_undefined_escapes() {
    assert_eq!(visible_text("before\u{1b}[2Jafter"), "before [2Jafter");
    assert_eq!(visible_text("before\\\u{7}after"), "before after");
}
