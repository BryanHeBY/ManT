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
