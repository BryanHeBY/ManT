//! Original source probes: spacing lifetime and font lifetime are independent.
use super::inline_boundaries::{query, variants};
use super::*;

fn assert_style(query: &ResolvedContent, word: &str, expected: u8) {
    struct Styles<'a> {
        word: &'a str,
        current: u8,
        found: Vec<u8>,
    }
    impl<'ir> Visit<'ir> for Styles<'_> {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            let previous = self.current;
            match inline {
                Inline::Strong { .. } => self.current |= 1,
                Inline::Emphasis { .. } => self.current |= 2,
                Inline::Code { value } if value.contains(self.word) => {
                    self.found.push(self.current | 4);
                }
                Inline::Text { value } if value.contains(self.word) => {
                    self.found.push(self.current);
                }
                _ => {}
            }
            visit::walk_inline(self, inline);
            self.current = previous;
        }
    }
    let mut styles = Styles {
        word,
        current: 0,
        found: Vec::new(),
    };
    styles.visit_document(query.document.as_ref().unwrap());
    assert!(!styles.found.is_empty(), "missing {word}");
    assert!(
        styles.found.iter().all(|style| *style == expected),
        "{word}: {:?}\n{}",
        styles.found,
        crate::render_markdown(query)
    );
}

#[test]
fn mdoc_operand_font_escapes_do_not_escape_their_scope() {
    for (body, expected) in [
        (r".Em \fBWORD No NEXT", 0),
        (r".No \fBWORD Em NEXT", 2),
        (r".No \fIWORD No NEXT", 0),
        // Native No coalesces these words into one text run. The escape
        // remains active within that run, whether or not it was quoted.
        (r".No \fBWORD NEXT", 1),
        (r".No \fBWORD \fPNEXT", 0),
        (r#".No "\fBWORD NEXT""#, 1),
    ] {
        for input in variants(body) {
            let query = query(&input);
            assert_style(&query, "NEXT", expected);
            assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
            let markdown = crate::render_markdown(&query);
            // Literal/code projections intentionally carry code presentation,
            // not individual font runs. For prose, check the actual export.
            if input == body {
                let reparsed = crate::query_markdown_text(&markdown, None).unwrap();
                assert_style(&reparsed, "NEXT", expected);
            }
        }
    }
}

#[test]
fn mdoc_font_scope_exit_preserves_previous_font_and_pending_boundaries() {
    let query = query(".Em \\fBWORD\n.No NEXT\n.Pp\n.No LATER");
    assert_style(&query, "NEXT", 0);
    assert_style(&query, "LATER", 0);
    let query = super::inline_boundaries::query(
        ".ft B\n.No WORD\n.No \\fINEXT\n.No LAST\n.ft P\n.No RESET",
    );
    assert_style(&query, "WORD", 0);
    assert_style(&query, "NEXT", 2);
    assert_style(&query, "LAST", 0);
    assert_style(&query, "RESET", 0);
    let query = super::inline_boundaries::query(".Em \\fBWORD Ap s\n.No NEXT");
    assert!(crate::render_query_text(&query).contains("WORD's NEXT"));
    assert_style(&query, "NEXT", 0);
}

#[test]
fn mdoc_bf_and_man_persistent_fonts_keep_their_existing_lifetimes() {
    for literal in [false, true] {
        let body = ".Bf -emphasis\n.No \\fBWORD\n.No NEXT\n.Ef\n.No LAST";
        let query = query(&if literal {
            format!(".Bd -literal\n{body}\n.Ed")
        } else {
            body.into()
        });
        assert_style(&query, "NEXT", 0);
        assert_style(&query, "LAST", 0);
    }
    let query = crate::query_roff_bytes(
        b".TH PROBE 1\n.SH DESCRIPTION\n\\fBWORD\nNEXT\n.ft I\nITALIC\n.ft P\nBACK\n.PP\nRESET\n",
    )
    .unwrap();
    assert_style(&query, "NEXT", 1);
    assert_style(&query, "ITALIC", 2);
    assert_style(&query, "BACK", 1);
    assert_style(&query, "RESET", 0);
}

#[test]
fn mdoc_macros_select_fonts_instead_of_adding_to_the_outer_font() {
    for (body, word, tail) in [
        (".ft B\n.No WORD\nTAIL", 0, 1),
        (".ft B\n.Em WORD\nTAIL", 2, 1),
        (".ft I\n.Sy WORD\nTAIL", 1, 2),
        (".ft I\n.Fl WORD\nTAIL", 1, 2),
        (".Em \\fBWORD\nTAIL", 1, 0),
        (".Sy \\fIWORD\nTAIL", 2, 0),
        (".Em \\fRWORD\nTAIL", 0, 0),
    ] {
        let query = query(body);
        assert_style(&query, "WORD", word);
        assert_style(&query, "TAIL", tail);
        let exported = crate::render_markdown(&query);
        let reparsed = crate::query_markdown_text(&exported, None).unwrap();
        assert_style(&reparsed, "WORD", word);
        assert_style(&reparsed, "TAIL", tail);
    }
}

#[test]
fn mdoc_simple_font_macros_share_the_same_scope_rules() {
    for (macros, style) in [
        ("Nm Fl Cm Ic Sy Ms", 1),
        ("Ar Pa Em Va Vt Ft Fa Ad Fr Sx", 2),
        ("No Dv", 0),
        ("Li", 4),
    ] {
        for name in macros.split_whitespace() {
            // Select the opposite surrounding font to detect accidental
            // additive styling or inheritance instead of a real selection.
            let (outer, outer_style) = if style == 1 { ("I", 2) } else { ("B", 1) };
            let query = query(&format!(".ft {outer}\n.{name} WORD\nTAIL"));
            assert_style(&query, "WORD", style);
            assert_style(&query, "TAIL", outer_style);
        }
    }
}

#[test]
fn transparent_macros_do_not_create_font_scopes() {
    let query = query(".Pf \\fBWORD No NEXT\nTAIL");
    assert_style(&query, "WORD", 1);
    assert_style(&query, "NEXT", 0);
    assert_style(&query, "TAIL", 1);
}

#[test]
fn mdoc_font_pop_restores_current_but_not_previous_selection() {
    // mandoc keeps the last explicit font selection across a scope pop.
    // groff differs here; this assertion follows the pinned mdoc formatter.
    let query = query(".No \\fBWORD\\fIINNER\n\\fPTAIL");
    assert_style(&query, "WORD", 1);
    assert_style(&query, "INNER", 2);
    assert_style(&query, "TAIL", 1);
}

#[test]
fn nested_font_blocks_allow_local_overrides_and_resume_the_outer_font() {
    for literal in [false, true] {
        let body = ".Bf -emphasis\nOUTER\n.No NORMAL\n.Bf -symbolic\nINNER\n.Em LOCAL\n.Ef\nRESUMED\n.Ef\nTAIL";
        let source = if literal {
            format!(".Bd -literal\n{body}\n.Ed")
        } else {
            body.into()
        };
        let query = query(&source);
        for (word, style) in [
            ("OUTER", 2),
            ("NORMAL", 0),
            ("INNER", 1),
            ("LOCAL", 2),
            ("RESUMED", 2),
            ("TAIL", 0),
        ] {
            assert_style(&query, word, style);
        }
    }
}

#[test]
fn font_blocks_pass_effective_fonts_through_lists_and_displays() {
    for container in [
        ".Bl -bullet\n.It\nWORD\n.No NORMAL\n.El",
        ".Bl -enum\n.It\nWORD\n.No NORMAL\n.El",
        ".Bl -item\n.It\nWORD\n.No NORMAL\n.El",
        ".Bl -tag -width Ds\n.It Fl x\nWORD\n.No NORMAL\n.El",
        ".Bl -column Ds Ds\n.It WORD Ta No NORMAL\n.El",
        ".Bd -literal\nWORD\n.No NORMAL\n.Ed",
        ".Bd -filled\nWORD\n.No NORMAL\n.Ed",
        ".Bd -unfilled\nWORD\n.No NORMAL\n.Ed",
    ] {
        let query = query(&format!(".Bf -emphasis\n{container}\nRESUMED\n.Ef\nTAIL"));
        assert_style(&query, "WORD", 2);
        assert_style(&query, "NORMAL", 0);
        assert_style(&query, "RESUMED", 2);
        assert_style(&query, "TAIL", 0);
        assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    }
}
