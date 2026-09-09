//! Small upstream-contract examples; assert semantics as well as visible text.
use super::*;

#[test]
fn typewriter_and_typographic_quotes_keep_distinct_delimiters() {
    for (body, expected) in [
        (".Qq hello", "\"hello\""),
        (".Qo hello Qc", "\"hello\""),
        (".Qq", "\"\""),
        (".Qo\n.Qc", "\"\""),
        (".Dq hello", "“hello”"),
        (".Do hello Dc", "“hello”"),
        (".Dq", "“”"),
        (".Do\n.Dc", "“”"),
        (".Qq Dq hello", "\"“hello”\""),
        (".Dq Qq hello", "“\"hello\"”"),
    ] {
        for input in [body.to_owned(), format!(".TS\nl.\nT{{\n{body}\nT}}\n.TE")] {
            let document = mdoc(&input);
            let query = ResolvedContent {
                label: "probe".into(),
                address: None,
                document: Some(document),
                tldr: None,
            };
            let text = mant_engine::render_query_text(&query);
            assert!(text.contains(expected), "{input}: {text}");
        }
    }
}

fn links(document: &mant_ir::Document) -> Vec<(mant_ir::LinkTarget, String)> {
    struct Collector(Vec<(mant_ir::LinkTarget, String)>);
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link {
                target, children, ..
            } = inline
            {
                self.0.push((target.clone(), inline_text(children)));
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut collector = Collector(Vec::new());
    collector.visit_document(document);
    collector.0
}

#[test]
fn mt_retains_each_address_with_its_own_target() {
    for body in [
        ".Mt first@example.com second@example.com .",
        ".Mt first@example.com , second@example.com .",
        ".No Contact Mt first@example.com second@example.com .",
        ".TS\nl.\nT{\n.Mt first@example.com second@example.com .\nT}\n.TE",
    ] {
        let document = mdoc(body);
        let actual = links(&document);
        assert_eq!(actual.len(), 2, "{body}: {actual:?}");
        for ((target, label), address) in actual
            .iter()
            .zip(["first@example.com", "second@example.com"])
        {
            assert_eq!(label, address);
            assert_eq!(
                target,
                &mant_ir::LinkTarget::Email {
                    address: address.into()
                }
            );
        }
    }
}

fn mdoc(body: &str) -> mant_ir::Document {
    let source = format!(".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{body}\n");
    parse_manual_bytes(std::path::Path::new("probe.1"), source.as_bytes()).unwrap()
}

fn styled_text(document: &mant_ir::Document) -> Vec<(String, bool, bool)> {
    #[derive(Default)]
    struct Collector {
        runs: Vec<(String, bool, bool)>,
        strong: bool,
        emphasis: bool,
    }
    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            let previous = (self.strong, self.emphasis);
            match inline {
                Inline::Strong { .. } => self.strong = true,
                Inline::Emphasis { .. } => self.emphasis = true,
                Inline::Text { value } => {
                    self.runs.push((value.clone(), self.strong, self.emphasis));
                }
                _ => {}
            }
            visit::walk_inline(self, inline);
            (self.strong, self.emphasis) = previous;
        }
    }
    let mut collector = Collector::default();
    collector.visit_document(document);
    collector.runs
}

#[test]
fn man_fonts_preserve_previous_selection_and_respect_macro_scope() {
    for (body, expected) in [
        (
            ".BI TOKENA TOKENB\n\\fPTOKENC",
            [("TOKENB", false, true), ("TOKENC", false, true)],
        ),
        (
            ".BI TOKENA \"\\fPTOKENB\"",
            [("TOKENA", true, false), ("TOKENB", true, false)],
        ),
        (
            ".OP TOKENA \"\\fPTOKENB\"",
            [("TOKENA", true, false), ("TOKENB", true, false)],
        ),
        (
            "\\fBTOKENA\n.OP --opt ARG\n\\fPTOKENB",
            [("TOKENA", true, false), ("TOKENB", false, false)],
        ),
        (
            ".B \"TOKENA\\fRTOKENB\"",
            [("TOKENA", true, false), ("TOKENB", false, false)],
        ),
        (
            ".I \"TOKENA\\fRTOKENB\"",
            [("TOKENA", false, true), ("TOKENB", false, false)],
        ),
        (
            ".OP \"TOKENA\\fRTOKENB\"",
            [("TOKENA", true, false), ("TOKENB", false, false)],
        ),
        (
            "\\fBTOKENA\nTOKENB",
            [("TOKENA", true, false), ("TOKENB", true, false)],
        ),
        (
            ".nf\n\\fBTOKENA\nTOKENB\n.fi",
            [("TOKENA", true, false), ("TOKENB", true, false)],
        ),
        (
            ".ft B\nTOKENA\nTOKENB",
            [("TOKENA", true, false), ("TOKENB", true, false)],
        ),
        (
            ".B TOKENA\nTOKENB",
            [("TOKENA", true, false), ("TOKENB", false, false)],
        ),
        (
            ".B TOKENA\n\\fPTOKENB",
            [("TOKENA", true, false), ("TOKENB", true, false)],
        ),
        (
            "\\fBTOKENA\n.PP\nTOKENB",
            [("TOKENA", true, false), ("TOKENB", false, false)],
        ),
        (
            "\\fBTOKENA\n.SM TOKENB",
            [("TOKENA", true, false), ("TOKENB", true, false)],
        ),
        (
            "\\fBTOKENA\\fIITALIC\\fPTOKENB",
            [("TOKENA", true, false), ("TOKENB", true, false)],
        ),
    ] {
        for table in [false, true] {
            // Block requests intentionally fall back to complete native/source
            // text in tbl; only inline-only cells claim semantic restoration.
            if table && (body.contains(".PP") || body.contains(".nf") || body.contains(".ft")) {
                continue;
            }
            let body = if table {
                format!(".TS\nl.\nT{{\n{body}\nT}}\n.TE")
            } else {
                body.into()
            };
            let source = format!(".TH PROBE 1\n.SH DESCRIPTION\n{body}\n");
            let document =
                parse_manual_bytes(std::path::Path::new("probe.1"), source.as_bytes()).unwrap();
            let runs = styled_text(&document);
            for (token, strong, emphasis) in expected {
                assert!(
                    runs.iter()
                        .any(|(text, s, e)| text.contains(token) && *s == strong && *e == emphasis),
                    "{body}: {runs:?}"
                );
            }
        }
    }
}

#[test]
fn enclosure_parts_have_one_owner_even_when_empty_or_reparented() {
    for (body, expected) in [
        (".Op", "[]"),
        (".Pq", "()"),
        (".Oo\n.Oc", "[]"),
        (".Aq", "<>"),
        (".Brq", "{}"),
        (".Eo (\nhello\n.Ec )", "(hello)"),
        (".Eo ( hello Ec )", "(hello)"),
        (".Eo <<\nhello\n.Ec >>", "<<hello>>"),
        (".Op Pq", "[()]"),
        (".Pq ;", "();"),
        (".Eo (\n.Mt first@example.com\n.Ec )", "(first@example.com)"),
    ] {
        let document = mdoc(body);
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{document:?}")
        };
        assert_eq!(inline_text(children), expected, "{body}");
        if body.contains(".Mt") {
            assert_eq!(links(&document).len(), 1);
        }
    }
    for (body, expected) in [(".Op", "[]"), (".Pq", "()"), (".Oo\n.Oc", "[]")] {
        let document = mdoc(&format!(".TS\nl.\nT{{\n{body}\nT}}\n.TE"));
        let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{document:?}")
        };
        let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
            panic!("{rows:?}")
        };
        assert_eq!(inline_text(children), expected);
    }
}

#[test]
fn fo_counts_operands_without_counting_controls_or_targets() {
    for (body, expected) in [
        (".Fa int size_t", "probe(int, size_t)"),
        (
            ".Fa \"const char *path\" \"int flags\"",
            "probe(const char *path, int flags)",
        ),
        (".Tg anchor\n.Fa int", "probe(int)"),
        (".Fa int\n.Sm off\n.Fa char", "probe(int, char)"),
        (".Fa int\n.Tg anchor\n.Fa char", "probe(int, char)"),
    ] {
        let document = mdoc(&format!(".Fo probe\n{body}\n.Fc"));
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{document:?}")
        };
        assert_eq!(inline_text(children), expected, "{body}");
        assert!(
            children
                .iter()
                .any(|node| matches!(node, Inline::Emphasis { .. })),
            "{children:?}"
        );
        if body.contains(".Tg") {
            assert!(anchor_ids(&document).iter().any(|id| id == "anchor"));
        }
    }
    let document = mdoc(".Fa int size_t");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("{document:?}")
    };
    assert_eq!(inline_text(children), "int size_t");
}
