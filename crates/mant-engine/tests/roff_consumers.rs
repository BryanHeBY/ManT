//! Complete corpus regressions stay outside the published crate source set.
#[allow(dead_code)]
mod common;
use mant_ir::{
    DefinitionItem,
    visit::{self, Visit},
};
use std::fmt::Write as _;

fn fixture(name: &str) -> mant_engine::ResolvedContent {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/mant-audit")
        .join(format!("lowering-{name}.1"));
    let source = std::fs::read(path).unwrap();
    mant_engine::query_roff_bytes(&source).unwrap()
}

fn description(query: &mant_engine::ResolvedContent) -> String {
    common::block_slice_text(
        &common::section(query.document.as_ref().unwrap(), "DESCRIPTION").blocks,
    )
}

fn style(query: &mant_engine::ResolvedContent, word: &str, expected: u8) {
    struct Styles<'a> {
        word: &'a str,
        active: u8,
        found: Vec<u8>,
    }
    impl<'ir> Visit<'ir> for Styles<'_> {
        fn visit_inline(&mut self, inline: &'ir mant_ir::Inline) {
            use mant_ir::Inline;
            let saved = self.active;
            match inline {
                Inline::Strong { .. } => self.active |= 1,
                Inline::Emphasis { .. } => self.active |= 2,
                Inline::Code { value } if value.contains(self.word) => {
                    self.found.push(self.active | 4);
                }
                Inline::Text { value } if value.contains(self.word) => self.found.push(self.active),
                _ => {}
            }
            visit::walk_inline(self, inline);
            self.active = saved;
        }
    }
    let mut styles = Styles {
        word,
        active: 0,
        found: Vec::new(),
    };
    styles.visit_document(query.document.as_ref().unwrap());
    assert!(!styles.found.is_empty(), "missing {word}");
    assert!(
        styles.found.iter().all(|value| *value == expected),
        "{word}: {:?}",
        styles.found
    );
}

#[test]
fn nested_wrapper_payloads() {
    for name in ["bk-table", "enclosure-table"] {
        let query = fixture(name);
        let doc = query.document.as_ref().unwrap();
        let tables = common::document_blocks(doc)
            .into_iter()
            .filter(|block| matches!(block, mant_ir::Block::Table { .. }))
            .collect::<Vec<_>>();
        assert_eq!(tables.len(), 1, "{name}");
        for word in ["WORD", "CELLTWO"] {
            assert_eq!(
                description(&query).matches(word).count(),
                1,
                "{name}: {word}"
            );
        }
        assert!(mant_ir::validate_document(doc).is_empty());
    }
}

#[test]
fn enclosure_font_scope() {
    let query = fixture("enclosure-font");
    style(&query, "WORD", 2);
    style(&query, "TAIL", 0);
    for (mode, expected) in [("emphasis", 2), ("symbolic", 1), ("literal", 4)] {
        let source = format!(
            ".Dd September 8, 2026\n.Dt FONT 1\n.Os\n.Sh DESCRIPTION\n.Oo\n.Bk -words\n.Bf -{mode}\nWORD\n.No PLAIN\n.Em EMPH\n.Ef\n.Ek\n.Oc\nTAIL\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        style(&query, "WORD", expected);
        style(&query, "PLAIN", 0);
        style(&query, "EMPH", 2);
        style(&query, "TAIL", 0);
    }
}

#[test]
fn two_level_containers_preserve_each_structural_payload() {
    for display in [None, Some("-literal"), Some("-unfilled")] {
        for (payload, table) in [
            (".TS\nl l.\nWORD\tCELLTWO\n.TE", true),
            (".Bl -bullet\n.It\nWORD CELLTWO\n.El", false),
            (".Bl -tag -width Ds\n.It WORD\nCELLTWO\n.El", false),
            (".Bl -column one two\n.It WORD Ta CELLTWO\n.El", true),
        ] {
            let mut source =
                String::from(".Dd September 8, 2026\n.Dt PAYLOAD 1\n.Os\n.Sh DESCRIPTION\n");
            if let Some(mode) = display {
                writeln!(source, ".Bd {mode}").unwrap();
            }
            writeln!(source, ".Eo [\n.Bk -words\n{payload}\n.Ek\n.Ec ]").unwrap();
            if display.is_some() {
                source.push_str(".Ed\n");
            }
            let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
            let text = description(&query);
            for word in ["WORD", "CELLTWO", "[", "]"] {
                assert_eq!(text.matches(word).count(), 1, "{source}: {text}");
            }
            let blocks = common::document_blocks(query.document.as_ref().unwrap());
            if table {
                assert_eq!(
                    blocks
                        .iter()
                        .filter(|block| matches!(block, mant_ir::Block::Table { .. }))
                        .count(),
                    1
                );
            }
        }
    }
}

#[test]
fn display_final_spacing() {
    assert!(description(&fixture("display-spacing")).contains("NEXTTAIL"));
    for kind in ["-bullet", "-tag -width Ds", "-column one two"] {
        for (initial, request, expected) in [
            ("on", "off", "NEXTTAIL"),
            ("off", "on", "NEXT TAIL"),
            ("on", "", "NEXTTAIL"),
            ("off", "", "NEXT TAIL"),
        ] {
            let source = format!(
                ".Dd September 8, 2026\n.Dt SPACING 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Sm {initial}\n.Bl {kind}\n.It WORD\n.Sm {request}\n.El\n.No NEXT No TAIL\n.Sm on\n.Ed\n"
            );
            let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
            assert!(
                description(&query).contains(expected),
                "{kind}, {initial}, {request}: {}",
                description(&query)
            );
        }
    }
}

#[test]
#[ignore = "R04: fixed HEAD scope policy; enable with macro state fix"]
fn link_and_include_font_state() {
    for (name, tail, resumed) in [
        ("lk-label-state", 0, 2),
        ("lk-uri-state", 1, 0),
        ("lk-order", 1, 0),
        ("mt-state", 0, 2),
        ("in-state", 0, 2),
    ] {
        let query = fixture(name);
        style(&query, "TAIL", tail);
        style(&query, "RESUMED", resumed);
        if name == "lk-order" {
            style(&query, "NEXT", 0);
        }
    }
}

#[test]
#[ignore = "R05: fixed logical adjacency policy; enable with punctuation fix"]
fn function_logical_adjacency() {
    assert_eq!(description(&fixture("fa-delimiter")).trim(), "WORD(x, y)");
    assert_eq!(description(&fixture("fa-prose")).trim(), "WORD(x NEXT y)");
}

#[test]
fn plain_text_lines_keep_word_boundaries() {
    assert_eq!(
        description(&fixture("sm-plain-lines")).trim(),
        "WORD NEXT TAIL"
    );
    for (body, expected) in [
        (".Sm off\n.No WORD NEXT\n.Sm on", "WORDNEXT"),
        (".Sm off\nWORD\\c\nNEXT\n.Sm on", "WORDNEXT"),
        (".Sm off\nWORD\n NEXT\n.Sm on", "WORD\n NEXT"),
    ] {
        let source = format!(".Dd September 8, 2026\n.Dt WORDS 1\n.Os\n.Sh DESCRIPTION\n{body}\n");
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        assert_eq!(description(&query).trim(), expected, "{body}");
    }
}

#[test]
fn literal_macro_descendants_keep_source_lines() {
    assert_eq!(
        description(&fixture("literal-function")).trim(),
        "WORD(\nx,\ny)"
    );
    let query = fixture("literal-enclosure");
    assert_eq!(description(&query).trim(), "[\nWORD\n]\nTAIL");
    style(&query, "WORD", 2);
}

#[test]
fn real_groff_font_escape_definition_preserves_all_four_literal_terms() {
    struct Terms(bool);
    impl<'ir> Visit<'ir> for Terms {
        fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
            self.0 |= item
                .terms
                .iter()
                .any(|term| common::inline_text(term) == r"\fB, \fI, \fR, \fP");
            visit::walk_definition_item(self, item);
        }
    }
    let document = mant_engine::parse_manual_source(std::path::Path::new(concat!(
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
