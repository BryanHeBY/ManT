//! Independent source probes for word events and structural consumers.
use super::inline_boundaries::{assert_flow, query, variants};

#[test]
fn man_literal_synopsis_preserves_font_state_across_lines() {
    use super::font_boundaries::assert_style;
    for (body, style) in [
        ("\\fBWORD\nNEXT", 1),
        ("\\fIWORD\nNEXT", 2),
        (".ft B\nWORD\nNEXT", 1),
        ("\\fBWORD\\fIOTHER\n\\fPNEXT", 1),
    ] {
        let source =
            format!(".TH PROBE 1\n.SH DESCRIPTION\n.EX\n.SY command\n{body}\n.YS\n.EE\nTAIL\n");
        let content = crate::query_roff_bytes(source.as_bytes()).unwrap();
        assert_style(&content, "WORD", style);
        assert_style(&content, "NEXT", style);
        assert_style(&content, "TAIL", 0);
    }
}

#[test]
fn prose_function_blocks_are_inline_but_synopsis_declarations_remain_separate() {
    assert_flow(
        ".No before\n.Fo WORD\n.Fa arg\n.Fc\n.No after",
        "before WORD(arg) after",
    );
    assert_flow(
        ".No before\n.Fo WORD\n.Fa arg\n.Fc )\n.No after",
        "before WORD(arg)) after",
    );
    let doc = crate::query_roff_bytes(b".Dd Sep 8, 2026\n.Dt PROBE 1\n.Os\n.Sh SYNOPSIS\n.Fo FIRST\n.Fa arg\n.Fc\n.Fo SECOND\n.Fa arg\n.Fc\n").unwrap();
    assert_eq!(doc.document.unwrap().sections[0].blocks.len(), 2);
}

#[test]
fn font_scopes_reach_bibliographies_and_table_cells() {
    use super::font_boundaries::assert_style;
    for (mode, style) in [("emphasis", 2), ("symbolic", 1), ("literal", 4)] {
        for inner in [
            ".Rs\n.%A WORD\n.%A NEXT\n.Re",
            ".TS\nl l.\nWORD\tNEXT\n.TE",
            ".TS\nl.\nT{\nWORD\nNEXT\nT}\n.TE",
            ".TS\nl.\nT{\nWORD\n.No NEXT\nT}\n.TE",
        ] {
            let content = query(&format!(".Bf -{mode}\n{inner}\nRESUMED\n.Ef\nTAIL"));
            assert_style(&content, "WORD", style);
            assert_style(
                &content,
                "NEXT",
                if inner.contains(".No NEXT") { 0 } else { style },
            );
            assert_style(&content, "RESUMED", style);
            assert_style(&content, "TAIL", 0);
            if inner.starts_with(".Rs") {
                assert!(crate::render_query_text(&content).contains("WORD and NEXT"));
            }
        }
    }
}

#[test]
fn generated_functions_and_references_share_operand_state() {
    use super::font_boundaries::assert_style;
    for body in [r".Fn \fIWORD \fBNEXT", ".Fo \\fIWORD\n.Fa \\fBNEXT\n.Fc"] {
        let content = query(body);
        assert_style(&content, "WORD", 2);
        assert_style(&content, "NEXT", 1);
    }
    let content = query(".Xr \\fBWORD 1\nTAIL");
    assert_style(&content, "WORD", 1);
    assert_style(&content, "TAIL", 1);
    assert_flow(
        ".Oo\n.Fo WORD\n.Sm off\n.Fa x\n.Fc\n.No NEXT TAIL\n.Oc\n.Sm on",
        "[WORD(x)NEXTTAIL]",
    );
}

#[test]
fn authored_enclosures_consume_empty_words_and_reset_unclosed_joins() {
    for (body, expected) in [
        (".Eo [\n.No word Ns\n.Ec\n.No TAIL", "[word TAIL"),
        (".No x Ns\n.Eo\n.Ec\n.No y", "x y"),
        (".No x\n.Eo \"\"\n.No word\n.Ec\n.No y", "x word y"),
        (".No x\n.Eo\n.Ec ]\n.No y", "x ] y"),
        (".Eo [\n.No word Ns\n.Ec ]\n.No TAIL", "[word] TAIL"),
    ] {
        assert_flow(body, expected);
    }
}

#[test]
fn literal_containers_preserve_nested_structural_payloads_and_targets() {
    for display in ["literal", "unfilled"] {
        for inner in [
            ".TS\nl l.\nWORD\tNEXT\n.TE",
            ".Bl -tag -width Ds\n.Tg Exact.Target\n.It Fl WORD\nNEXT\n.El",
            ".Bl -column Ds Ds\n.It WORD Ta NEXT\n.El",
            ".Bl -bullet\n.Tg Exact.Target\n.It\nWORD NEXT\n.El",
            ".Bl -enum\n.It\nWORD NEXT\n.El",
        ] {
            for wrap in [false, true] {
                let inner = if wrap {
                    format!(".Bf -emphasis\n.Bf -symbolic\n{inner}\n.Ef\n.Ef")
                } else {
                    inner.into()
                };
                let query = query(&format!(".Bd -{display}\nBEFORE\n{inner}\nAFTER\n.Ed"));
                let text = crate::render_query_text(&query);
                for word in ["BEFORE", "WORD", "NEXT", "AFTER"] {
                    assert!(text.contains(word), "{inner}: missing {word}: {text}");
                }
                let doc = query.document.as_ref().unwrap();
                assert!(mant_ir::validate_document(doc).is_empty());
                if inner.contains("Exact.Target") {
                    assert!(anchor_ids(doc).contains(&"exact-target".into()));
                }
            }
        }
    }
}

#[test]
fn zero_width_words_consume_boundaries_without_fabricating_names() {
    for operand in [r"\&", "\"\"", r"\fB"] {
        for body in [
            format!(".No a Ns No {operand} No b"),
            format!(".No a Pf {operand} No b"),
        ] {
            // Pf explicitly requests a following join after its operand.
            for input in variants(&body) {
                assert_flow(&input, "a b");
            }
        }
    }
    assert_flow(r".Op \& No b", "[ b]");
    assert_flow(r".No a No \& No b", "a  b");
    let body = ".Bl -tag -width Ds\n.It Fl a Ns No \\& Fl b\nBODY\n.El";
    assert_flow(body, "-a -b");
    let query = query(body);
    let explained = crate::explain_query(
        &query,
        &mant_protocol::ExplanationQuery {
            entry: "-a-b".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert!(
        !explained
            .evidence
            .iter()
            .any(|e| e.class == mant_protocol::EvidenceClass::DirectEntry)
    );
}
use super::*;

#[test]
fn decoded_literal_font_spellings_are_content_in_every_font() {
    struct Terms(bool);
    impl<'ir> Visit<'ir> for Terms {
        fn visit_definition_item(&mut self, item: &'ir mant_ir::DefinitionItem) {
            self.0 |= item
                .terms
                .iter()
                .any(|term| inline_text(term) == r"\fB, \fI, \fR, \fP");
            visit::walk_definition_item(self, item);
        }
    }
    for (macro_name, escape) in [("Sy", "B"), ("Em", "I"), ("Li", "C"), ("No", "R")] {
        for slash in [r"\e", r"\[rs]"] {
            for input in variants(&format!(".{macro_name} before{slash}f{escape}after")) {
                assert_flow(&input, &format!("before\\f{escape}after"));
            }
        }
    }
    let query = crate::query_roff_bytes(b".TH PROBE 1\n.SH DESCRIPTION\n.B \\efB\n").unwrap();
    assert!(crate::render_query_text(&query).contains(r"\fB"));
    let document = parse_manual_source(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/roff/real/debian/groff_man_style.7.gz"
    )))
    .unwrap();
    let mut terms = Terms(false);
    terms.visit_document(&document);
    assert!(
        terms.0,
        "real groff font-escape definition lost literal content"
    );
}
