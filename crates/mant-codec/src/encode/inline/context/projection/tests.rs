fn equivalent<'a>(pieces: impl Iterator<Item = &'a str>, whole: &str) -> bool {
    super::equivalent(pieces.map(|piece| (piece, None)), whole)
}

#[test]
fn outer_style_cannot_certify_reinterpreted_child_delimiters() {
    for (child, corrupted) in [
        (r"_`a`_b\*__", r"**_`a`_b\*__**"),
        (
            r"_[a](https://a.test)_b\*__",
            r"**_[a](https://a.test)_b\*__**",
        ),
    ] {
        assert!(!super::equivalent(
            [(child, Some(super::Style::Strong))].into_iter(),
            corrupted,
        ));
    }
    assert!(super::equivalent(
        [(" `a` ", Some(super::Style::Strong))].into_iter(),
        " **`a`** ",
    ));
    assert!(!super::equivalent(
        [(" `a` ", Some(super::Style::Strong))].into_iter(),
        "** `a` **",
    ));
}

#[test]
fn complete_check_rejects_remote_pairing_changed_styles_and_link_occurrences() {
    assert!(!equivalent(
        ["**$((**", "*arithmetic_expression*"].into_iter(),
        "**$((***arithmetic_expression*"
    ));
    assert!(!equivalent(["**A**", "_B_"].into_iter(), "_A_**B**"));
    assert!(!equivalent(
        ["[A](https://one.test)", "[B](https://one.test)"].into_iter(),
        "[AB](https://one.test)"
    ));
    assert!(!equivalent(
        ["[A](https://one.test \"first\")"].into_iter(),
        "[A](https://one.test \"second\")"
    ));
    assert!(!equivalent(["`A`"].into_iter(), "A"));
    assert!(equivalent(
        ["**--arg=**", "_reuse_level_"].into_iter(),
        "**--arg=**_reuse_level_"
    ));
}

#[test]
fn final_bytes_reject_row_entity_flanks_and_unprotected_image_openers() {
    let framing = super::Framing::Rows {
        manual_links: false,
        interruptible: true,
    };
    // Independent accepted source children, before the final delimiter
    // choices and row-edge rewrite. Restoring the old raw-only validator
    // would admit the damaged first sequence; removing seam protection
    // would admit the image in the second sequence.
    assert!(!super::equivalent_framed(
        [("_a ***b***_", None), (" ", None)].into_iter(),
        "_a _**b**__&#32;",
        framing,
    ));
    assert!(!super::equivalent_framed(
        [("!", None), ("[a](https://a.test)", None)].into_iter(),
        "![a](https://a.test)",
        framing,
    ));
    assert!(super::equivalent_framed(
        [("!", None), ("[a](https://a.test)", None)].into_iter(),
        "\\![a](https://a.test)",
        framing,
    ));
}

#[test]
fn bracket_witness_retains_original_label_metadata_and_occurrence_extent() {
    // Accepted child meaning precedes the candidate's actual bracket flanks.
    let meaning =
        super::link_meaning("_! ***a***_ ", "https://a.test/&amp;", Some("say \\ \n")).unwrap();
    for corrupted in [
        "[_! _**a**__ ](https://a.test/&amp;amp; \"say \\\\ &#10;\")",
        "[! a ](https://a.test/&amp; \"say \\\\ &#10;\")",
        "[! a ](https://a.test/&amp;amp; \"say &#10;\")",
    ] {
        assert!(!super::matches_link(&meaning, corrupted));
    }
    let plain = super::link_meaning("a ", "https://a.test/&amp;", Some("say \\\n")).unwrap();
    assert!(super::matches_link(
        &plain,
        "[a ](https://a.test/&amp;amp; \"say \\\\&#10;\")"
    ));
}

#[test]
fn decoded_control_witness_agrees_with_actual_reader_not_only_bare_parser() {
    let meaning = super::link_meaning("a&#13;&#10;b", "https://a.test", Some("t u")).unwrap();
    let raw = "[a&#13;<br>b](https://a.test \"t&#12;u\")";
    assert!(super::matches_link(&meaning, raw));
    let framing = super::Framing::Rows {
        manual_links: false,
        interruptible: true,
    };
    let compare = |candidate| {
        super::equivalent_with_meanings(
            [(raw, None, Some(&meaning))].into_iter(),
            candidate,
            framing,
        )
    };
    assert!(compare("[a&#13;  \nb](https://a.test \"t&#12;u\")"));
    // Without decoded normalization the local witness would wrongly reject
    // this exact safe meaning; without final CR protection it would admit
    // an output whose actual reader no longer owns one Link.
    assert!(!compare("[a\r  \nb](https://a.test \"t u\")"));
    assert!(!compare("[a&#13;  \nb](https://other.test \"t u\")"));
    assert!(!compare("[a&#13;  \nb](https://a.test \"t x\")"));
}

#[test]
fn mathematical_witness_keeps_event_boundaries_styles_and_link_owners() {
    assert!(equivalent(["$a$", "$b$"].into_iter(), "$a$$b$"));
    assert!(!equivalent(["$a$", "$b$"].into_iter(), "$ab$"));
    assert!(!equivalent(["$a$"].into_iter(), "a"));
    assert!(!equivalent(["$a$"].into_iter(), "$$a$$"));
    assert!(super::equivalent(
        [("$a$", Some(super::Style::Strong))].into_iter(),
        "**$a$**"
    ));
    assert!(!super::equivalent(
        [("$a$", Some(super::Style::Strong))].into_iter(),
        "$a$"
    ));
    let meaning = super::link_meaning("$a$$b$", "https://example.com", None).unwrap();
    assert!(super::matches_link(
        &meaning,
        "[$a$$b$](https://example.com)"
    ));
    assert!(!super::matches_link(
        &meaning,
        "[$ab$](https://example.com)"
    ));
    assert!(!super::matches_link(
        &meaning,
        "[$a$](https://example.com)$b$"
    ));
}
