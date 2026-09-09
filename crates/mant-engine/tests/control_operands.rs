//! Original regressions for control operands versus printable source words.
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

fn source(mode: &str, body: &str) -> String {
    match mode {
        "man" => format!(".TH CONTROL 1\n.SH TEST\n{body}\n"),
        "nf" => format!(".TH CONTROL 1\n.SH TEST\n.nf\n{body}\n.fi\n"),
        "EX" => format!(".TH CONTROL 1\n.SH TEST\n.EX\n{body}\n.EE\n"),
        "mdoc" => format!(".Dd September 9, 2026\n.Dt CONTROL 1\n.Os\n.Sh TEST\n{body}\n"),
        mode => format!(
            ".Dd September 9, 2026\n.Dt CONTROL 1\n.Os\n.Sh TEST\n.Bd -{mode} -compact\n{body}\n.Ed\n"
        ),
    }
}

#[test]
fn operands_never_become_text_in_filled_and_literal_flows() {
    for mode in ["man", "nf", "EX", "mdoc", "literal", "unfilled", "filled"] {
        for (request, operand) in [
            (".ll 50n", "50n"),
            (".po 0n", "0n"),
            (".mc |", "|"),
            (".ti 3n", "3n"),
            (".ta 7n", "7n"),
            (".ll", ""),
            (".po", ""),
            (".mc", ""),
        ] {
            let input = source(mode, &format!("BEFORE\n{request}\nBODY\nAFTER"));
            let query = load_roff_bytes(input.as_bytes()).unwrap();
            let text = render_query_text(&query);
            if !operand.is_empty() {
                assert!(!text.contains(operand), "{input}\n{text}");
            }
            for token in ["BEFORE", "BODY", "AFTER"] {
                assert_eq!(text.matches(token).count(), 1, "{input}\n{text}");
            }
            let document = query.document.as_ref().unwrap();
            assert!(mant_ir::validate_document(document).is_empty(), "{input}");
        }
    }
}

#[test]
fn omitted_page_controls_preserve_continuations_and_font_state() {
    use mant_ir::visit::{Visit, walk_inline};
    #[derive(Default)]
    struct BoldWords(Vec<String>);
    impl<'ir> Visit<'ir> for BoldWords {
        fn visit_inline(&mut self, inline: &'ir mant_ir::Inline) {
            if let mant_ir::Inline::Strong { children } = inline {
                for child in children {
                    if let mant_ir::Inline::Text { value } = child {
                        self.0.push(value.clone());
                    }
                }
            }
            walk_inline(self, inline);
        }
    }
    for mode in ["man", "nf", "EX", "mdoc", "literal", "unfilled"] {
        let input = source(
            mode,
            "ALPHA\\c\n.ll 50n\n.po 0n\nBETA\n.ft B\nGAMMA\n.ft R\nDELTA",
        );
        let query = load_roff_bytes(input.as_bytes()).unwrap();
        let text = render_query_text(&query);
        assert!(text.contains("ALPHABETA"), "{input}\n{text}");
        let mut bold = BoldWords::default();
        bold.visit_document(query.document.as_ref().unwrap());
        assert!(bold.0.iter().any(|word| word.contains("GAMMA")), "{input}");
        assert!(!bold.0.iter().any(|word| word.contains("DELTA")), "{input}");
        assert!(!text.contains("50n") && !text.contains("0n"), "{text}");
    }
}

#[test]
fn control_spellings_and_numbers_remain_visible_when_authored_as_words() {
    for mode in ["man", "nf", "EX"] {
        let input = source(mode, ".B \"ll 50n po 0n mc ti\"\nAFTER");
        let query = load_roff_bytes(input.as_bytes()).unwrap();
        assert!(render_query_text(&query).contains("ll 50n po 0n mc ti"));
    }
}

#[test]
fn table_inline_recovery_consumes_controls_without_discarding_real_words() {
    for dialect in ["man", "mdoc"] {
        let word = if dialect == "man" {
            ".B BODY"
        } else {
            ".Sy BODY"
        };
        let body =
            format!(".TS\nl.\nT{{\n.ll 50n\n.po 0n\n.mc |\n.ft B\n{word}\n.ft R\nTAIL\nT}}\n.TE");
        let input = source(dialect, &body);
        let query = load_roff_bytes(input.as_bytes()).unwrap();
        let text = render_query_text(&query);
        assert!(
            text.contains("BODY") && text.contains("TAIL"),
            "{input}\n{text}"
        );
        assert!(
            !text.contains("50n") && !text.contains("0n"),
            "{input}\n{text}"
        );
        assert!(
            !query
                .document
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref()
                    == Some("manual.unhandled-table-text-block")),
            "{input}"
        );
    }
}
