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
            let text = mant_render::render_query_text(&query);
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
        // No child → catalog \(la/\(ra glyphs (pristine oracle: `.Aq` → `⟨⟩`).
        (".Aq", "⟨⟩"),
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

#[test]
fn man_optional_argument_recovery_executes_only_the_native_operand_prefix() {
    use libmandoc_rs::{MacroToken, ManMacro, NodeKind, Parser};

    #[derive(serde::Deserialize)]
    struct Case {
        id: String,
        source: String,
        source_sha256: String,
        expected: String,
        profiles: std::collections::BTreeMap<String, (i32, String, String)>,
        operand_count: usize,
    }
    #[derive(serde::Deserialize)]
    struct Matrix {
        oracle_sha256: String,
        cases: Vec<Case>,
    }

    // The fixed pristine tree keeps every operand after post_OP's recovery
    // diagnostic. man_term.c::pre_OP visits only its first two term_word
    // operands, followed by the generated closing bracket. The expected
    // rows preserve that order and exact spacing after undoing overstrikes
    // and removing the fixed five-column device margin.
    let matrix: Matrix =
        serde_json::from_str(include_str!("upstream_inline/man_option_arity.json")).unwrap();
    assert_eq!(
        matrix.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(matrix.cases.len(), 45);
    for case in &matrix.cases {
        assert_eq!(case.source_sha256.len(), 64, "{}", case.id);
        assert_eq!(case.profiles.len(), 5, "{}", case.id);
        for (profile, (status, stdout, stderr)) in &case.profiles {
            assert_eq!(stdout.len(), 64, "{}: {profile}", case.id);
            assert_eq!(stderr.len(), 64, "{}: {profile}", case.id);
            if profile != "lint" {
                assert_eq!(*status, 0, "{}: {profile}", case.id);
            }
        }
        let parsed = Parser::default()
            .parse_bytes("probe.1", case.source.as_bytes())
            .unwrap();
        let op = parsed
            .document
            .root
            .children
            .iter()
            .flat_map(|section| &section.children)
            .filter(|part| part.kind == NodeKind::Body)
            .flat_map(|body| &body.children)
            .find(|node| node.macro_token == Some(MacroToken::Man(ManMacro::Op)))
            .unwrap();
        assert_eq!(op.children.len(), case.operand_count, "{}", case.id);

        let content = super::json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        let document = content.document.as_ref().unwrap();
        let diagnostic = document
            .diagnostics
            .iter()
            .find(|d| d.message.starts_with("skipping excess arguments: OP"));
        assert_eq!(diagnostic.is_some(), case.operand_count > 2, "{}", case.id);
        let plain = mant_render::render_query_text(&content);
        let (actual, _) = plain
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("\n\nEND\n")
            .unwrap();
        assert_eq!(actual, case.expected, "{}", case.id);
        assert_option_ansi_matches_plain(&content, &plain);

        if let Some(diagnostic) = diagnostic {
            assert_eq!(case.profiles["lint"].0, 3, "{}", case.id);
            assert_eq!(diagnostic.impact, mant_ir::DiagnosticImpact::None);
            assert_eq!(diagnostic.level, mant_ir::DiagnosticLevel::Error);
            assert_eq!(diagnostic.source.unwrap().line, op.children[2].line);
            assert_eq!(diagnostic.source.unwrap().column, op.children[2].column);

            let (variant, follower) = case.id.split_once('/').unwrap();
            let legal = match variant {
                "first-empty" => "first-empty-legal",
                "second-empty" => "second-empty-legal",
                "both-empty" => "both-empty-legal",
                _ => "two",
            };
            let control_id = format!("{legal}/{follower}");
            let control = matrix
                .cases
                .iter()
                .find(|case| case.id == control_id)
                .unwrap();
            let control = mant_loader::load_roff_bytes(control.source.as_bytes()).unwrap();
            let control = control.document.as_ref().unwrap();
            assert_eq!(styled_text(document), styled_text(control), "{}", case.id);
            assert_eq!(links(document), links(control), "{}", case.id);
        } else {
            assert_eq!(
                case.profiles["lint"].0,
                if case.operand_count == 0 { 2 } else { 0 }
            );
        }
        if case.id == "first-empty/plain" {
            assert_recovered_option_search(&content);
        }
        assert_recovered_option_readbacks(&content, &case.expected, &case.id);
    }
}

fn assert_option_ansi_matches_plain(content: &ResolvedContent, plain: &str) {
    let ansi =
        mant_render::render_query_text_with(content, |_, value| format!("\x1b[1m{value}\x1b[0m"));
    assert_eq!(ansi.replace("\x1b[1m", "").replace("\x1b[0m", ""), plain);
}

fn assert_recovered_option_search(content: &ResolvedContent) {
    for (pattern, total) in [("EXTRA", 0), ("[ B] AFTER", 1)] {
        let search = mant_query::search_query(
            content,
            &mant_protocol::SearchQuery {
                pattern: pattern.into(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(search.total, total, "{pattern}: {search:#?}");
        assert!(search.content_complete);
    }
}

fn assert_recovered_option_readbacks(content: &ResolvedContent, expected: &str, id: &str) {
    use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};

    for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
        let mut current = content.clone();
        for _ in 0..2 {
            let markdown = render_markdown_with_options(&current, options);
            current = super::json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
            let readback = mant_render::render_query_text(&current);
            assert!(readback.contains(expected), "{id}: {readback:?}");
        }
    }
}
