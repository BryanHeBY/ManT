//! Wave-3 macro-gap contracts: generated glyphs, declaration fonts, and
//! typed reference links.
//!
//! Every expectation was first reproduced with the pristine pinned reference
//! binary (`target/mandoc-migration/reference/mandoc -Tutf8`); the upstream
//! rule is cited per case. The `%U`/`%R` link enrichment follows the HTML
//! renderer's typed-target expansion (`mdoc_html.c::mdoc__x_pre`), which the
//! terminal word itself does not show.

use super::*;

#[test]
fn generated_enclosure_glyphs_follow_the_device_catalog() {
    // mdoc_term.c::termp_quote_pre/post (1600-1669): all quote macros emit
    // their marks through the character catalog (`\(lq`, `\(rq`, `\(oq`,
    // `\(cq`, `\(la`, `\(ra`), except the sole-`.Mt`-child angle form which
    // stays ASCII (1600-1603, 1658-1661).
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
        ".Aq foo\n.Aq Mt user@host.example\n.Dq quoted\n.Sq single\n",
        ".Ql literal\n.Op opt\n",
    );
    let native = native_terminal(source);
    let lowered = without_line_indentation(&lowered_terminal(source));
    for expected in [
        "⟨foo⟩",
        "<user@host.example>",
        "“quoted”",
        "‘single’",
        "‘literal’",
        "[opt]",
    ] {
        assert!(
            native.contains(expected),
            "native lost {expected:?} in\n{native}"
        );
        assert!(
            lowered.contains(expected),
            "lowered lost {expected:?} in\n{lowered}"
        );
    }
}

#[test]
fn nd_prints_the_reference_en_dash() {
    // mdoc_term.c::termp_nd_pre() (1120-1125) prints `\(en` (U+2013),
    // never an em dash.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd en dash description\n",
    );
    let native = native_terminal(source);
    let lowered = without_line_indentation(&lowered_terminal(source));
    assert!(native.contains("– en dash description"), "{native}");
    assert!(lowered.contains("– en dash description"), "{lowered}");
    assert!(!lowered.contains('—'), "{lowered}");
}

#[test]
fn fd_is_bold_and_ends_its_row_in_every_section() {
    // mdoc_term.c dispatch (142/149): Cd and Fd both run termp_fd_pre =
    // termp_bold_pre unconditionally; termp_fd_post (1261-1265) breaks the
    // row after `.Fd` even outside SYNOPSIS, while `.Cd` never breaks.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
        "Before\n.Fd #include <x.h>\n.No After CONFIG end\n",
        ".No normal\n.Cd CFG\n.No tail\n",
    );
    let native = native_terminal(source);
    let lowered = without_line_indentation(&lowered_terminal(source));
    // Reference rows: `Before #include <x.h>` then `After CONFIG end` on
    // the next row, and the Cd row stays filled.
    assert!(
        native.contains("Before #include <x.h>\n     After CONFIG end"),
        "{native}"
    );
    assert!(
        lowered.contains("Before #include <x.h>\nAfter CONFIG end"),
        "{lowered}"
    );
    assert!(native.contains("normal CFG tail"), "{native}");
    assert!(lowered.contains("normal CFG tail"), "{lowered}");

    let document = lowered_document(source);
    struct StrongInclude(bool);
    impl<'ir> Visit<'ir> for StrongInclude {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Strong { children } = inline
                && mant_ir::inline_plain_text(children).contains("include")
            {
                self.0 = true;
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut include_bold = StrongInclude(false);
    struct StrongConfig(bool);
    impl<'ir> Visit<'ir> for StrongConfig {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Strong { children } = inline
                && mant_ir::inline_plain_text(children) == "CFG"
            {
                self.0 = true;
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut config_bold = StrongConfig(false);
    visit::walk_document(&mut include_bold, &document);
    visit::walk_document(&mut config_bold, &document);
    assert!(
        include_bold.0,
        "Fd lost its unconditional Strong scope: run split detected"
    );
    assert!(config_bold.0, "Cd lost its unconditional Strong scope");
}

#[test]
fn lk_projects_label_colon_uri_with_trailing_punctuation() {
    // mdoc_term.c::termp_lk_pre (1880-1930): descriptive label, generated
    // `:` with NOSPACE, the URI, then trailing DELIMC punctuation. A
    // URI-only Lk prints the bare address (no colon).
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
        ".Lk https://example.com/x label\n",
        ".Lk https://example.com/y\n",
        ".Lk https://example.com/z named.\n",
    );
    let native = native_terminal(source);
    let lowered = without_line_indentation(&lowered_terminal(source));
    // The third link may wrap after its generated colon at the terminal
    // width, so its pieces are pinned separately from the single-row forms.
    for expected in [
        "label: https://example.com/x",
        " https://example.com/y",
        "named.:",
        "https://example.com/z",
    ] {
        assert!(
            native.contains(expected),
            "native lost {expected:?} in\n{native}"
        );
        assert!(
            lowered.contains(expected),
            "lowered lost {expected:?} in\n{lowered}"
        );
    }
}

#[test]
fn reference_fields_carry_typed_external_targets() {
    // mdoc_html.c::mdoc__x_pre (1553-1581): `%U` always links its argument;
    // `%R` links exactly `RFC <digits>` to the canonical rfc-editor page.
    // The terminal word keeps the authored argument spelling.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh AUTHORS\n",
        ".An Alice\n.Rs\n.%R RFC 1149\n.%U https://u.example/x\n.Re\n",
    );
    let native = native_terminal(source);
    assert!(
        native.contains("RFC 1149, https://u.example/x."),
        "{native}"
    );

    let document = lowered_document(source);
    struct ExternalTargets(Vec<String>);
    impl<'ir> Visit<'ir> for ExternalTargets {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link {
                target: LinkTarget::External { uri, .. },
                ..
            } = inline
            {
                self.0.push(uri.clone());
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut targets = ExternalTargets(Vec::new());
    visit::walk_document(&mut targets, &document);
    assert!(
        targets
            .0
            .contains(&"https://www.rfc-editor.org/rfc/rfc1149.html".to_owned()),
        "%%R lost its rfc-editor target: {:?}",
        targets.0
    );
    assert!(
        targets.0.contains(&"https://u.example/x".to_owned()),
        "%%U lost its argument target: {:?}",
        targets.0
    );
    let lowered = without_line_indentation(&lowered_terminal(source));
    assert!(
        lowered.contains("RFC 1149: https://www.rfc-editor.org/rfc/rfc1149.html"),
        "{lowered}"
    );
    assert!(lowered.contains("https://u.example/x"), "{lowered}");
}

#[test]
fn declared_column_widths_set_the_reference_offsets() {
    // mdoc_term.c::termp_bl_pre (701-733): each column starts at the sum of
    // the earlier declared `Bl -column` widths plus dcol (4 blanks for fewer
    // than five declared columns). "Sy Fl x" declares width 7, so the second
    // column starts eleven cells after the row origin.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
        ".Bl -column \"Sy Fl x\" \"desc\"\n",
        ".It Sy Fl \\-x Ta one\n.It Sy Fl \\-y Ta two longer\n.El\n",
    );
    let native = native_terminal(source);
    let lowered = without_line_indentation(&lowered_terminal(source));
    assert!(native.contains("--x        one"), "{native}");
    assert!(native.contains("--y        two longer"), "{native}");
    assert!(lowered.contains("--x        one"), "{lowered}");
    assert!(lowered.contains("--y        two longer"), "{lowered}");
}

#[test]
fn z_escape_separators_settle_like_the_reference() {
    // term.c BACKBEFORE/BACKAFTER settle with the escape separator: `\z`
    // overstrikes the next word's first glyph through the separator without
    // inventing or dropping a boundary blank.
    for (tail, expected) in [
        ("A\\z\\eB tail", "AB tail"),
        ("A\\z\\|B tail", "ABtail"),
        ("A\\z\\&B tail", "ABtail"),
        ("A\\z\\(enB tail", "AB tail"),
    ] {
        let source =
            format!(".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {tail}\n");
        let native = native_terminal(&source);
        let lowered = without_line_indentation(&lowered_terminal(&source));
        assert!(
            native.contains(expected),
            "native lost {expected:?} for {tail:?} in\n{native}"
        );
        assert!(
            lowered.contains(expected),
            "lowered lost {expected:?} for {tail:?} in\n{lowered}"
        );
    }
}
