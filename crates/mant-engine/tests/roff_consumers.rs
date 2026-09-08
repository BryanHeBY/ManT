//! Complete corpus regressions stay outside the published crate source set.
#[allow(dead_code)]
mod common;
use mant_ir::{
    DefinitionItem,
    visit::{self, Visit},
};
use std::fmt::Write as _;

thread_local! {
    static FIXTURE_READS: std::cell::RefCell<Option<std::collections::BTreeSet<String>>> =
        const { std::cell::RefCell::new(None) };
}

fn fixture(name: &str) -> mant_engine::ResolvedContent {
    FIXTURE_READS.with_borrow_mut(|reads| {
        if let Some(reads) = reads {
            reads.insert(format!("lowering-{name}.1"));
        }
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/mant-audit")
        .join(format!("lowering-{name}.1"));
    let source = std::fs::read(path).unwrap();
    mant_engine::query_roff_bytes(&source).unwrap()
}

#[test]
fn every_complete_consumer_fixture_has_an_executed_behavior_check() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/mant-audit");
    let files: std::collections::BTreeSet<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with("lowering-") && name.ends_with(".1"))
        .collect();
    // Record actual fixture reads while executing the assertions, rather than
    // maintaining a second filename list that could drift from the tests.
    FIXTURE_READS.set(Some(std::collections::BTreeSet::new()));
    for check in [
        nested_wrapper_payloads,
        enclosure_font_scope,
        display_final_spacing,
        link_and_include_font_state,
        function_logical_adjacency,
        plain_text_lines_keep_word_boundaries,
        literal_macro_descendants_keep_source_lines,
    ] {
        check();
    }
    let exercised = FIXTURE_READS.take().unwrap();
    assert_eq!(files, exercised, "orphan or unregistered lowering fixture");
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
    for (open, close) in [(".Eo [", ".Ec ]"), (".Oo", ".Oc")] {
        for display in [None, Some("-literal"), Some("-unfilled")] {
            for (payload, table) in [
                (".TS\nl l.\nWORD\tCELLTWO\n.TE", true),
                (".Bl -bullet\n.It\nWORD CELLTWO\n.El", false),
                (".Bl -enum\n.It\nWORD CELLTWO\n.El", false),
                (".Bl -item\n.It\nWORD CELLTWO\n.El", false),
                (".Bl -tag -width Ds\n.It WORD\nCELLTWO\n.El", false),
                (".Bl -column one two\n.It WORD Ta CELLTWO\n.El", true),
            ] {
                let mut source =
                    String::from(".Dd September 8, 2026\n.Dt PAYLOAD 1\n.Os\n.Sh DESCRIPTION\n");
                if let Some(mode) = display {
                    writeln!(source, ".Bd {mode}").unwrap();
                }
                writeln!(source, "{open}\n.Bk -words\n{payload}\n.Ek\n{close}").unwrap();
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
                    for block in &blocks {
                        if let mant_ir::Block::Table { rows, source, .. } = block {
                            assert!(source.is_some(), "table lost its source: {payload}");
                            assert_eq!(rows.len(), 1);
                            assert_eq!(rows[0].cells.len(), 2);
                            for (cell, word) in rows[0].cells.iter().zip(["WORD", "CELLTWO"]) {
                                // Enclosure punctuation may attach to an edge cell;
                                // each original payload must still occupy its own cell.
                                assert_eq!(
                                    common::block_slice_text(&cell.blocks)
                                        .trim()
                                        .trim_matches(['[', ']']),
                                    word
                                );
                            }
                        }
                    }
                }
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
fn address_sequence_shares_one_font_scope_without_merging_email_targets() {
    #[derive(Default)]
    struct Addresses(Vec<String>);
    impl<'ir> Visit<'ir> for Addresses {
        fn visit_inline(&mut self, inline: &'ir mant_ir::Inline) {
            if let mant_ir::Inline::Link {
                target: mant_ir::LinkTarget::Email { address },
                ..
            } = inline
            {
                self.0.push(address.clone());
            }
            visit::walk_inline(self, inline);
        }
    }
    for (second, next_style, resumed_style) in
        [("NEXT@example.org", 1, 2), (r"\fPNEXT@example.org", 2, 1)]
    {
        let source = format!(
            ".Dd September 8, 2026\n.Dt MAIL 1\n.Os\n.Sh DESCRIPTION\n.Mt \\fBWORD@example.org {second}\nTAIL\n\\fPRESUMED\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        style(&query, "WORD", 1);
        style(&query, "NEXT", next_style);
        style(&query, "TAIL", 0);
        style(&query, "RESUMED", resumed_style);
        let mut addresses = Addresses::default();
        addresses.visit_document(query.document.as_ref().unwrap());
        assert_eq!(addresses.0, ["WORD@example.org", "NEXT@example.org"]);
        assert!(description(&query).contains("WORD@example.org NEXT@example.org"));
    }
}

#[test]
fn function_logical_adjacency() {
    assert_eq!(description(&fixture("fa-delimiter")).trim(), "WORD(x, y)");
    assert_eq!(description(&fixture("fa-prose")).trim(), "WORD(x NEXT y)");
    for (middle, expected) in [
        (".Sm off", "WORD(x, y)"),
        (".Tg position", "WORD(x, y)"),
        (".Bf -emphasis\n.No NEXT\n.Ef", "WORD(x NEXT y)"),
        (".Bk -words\n.No NEXT\n.Ek", "WORD(x NEXT y)"),
    ] {
        let source = format!(
            ".Dd September 8, 2026\n.Dt FUNCTION 1\n.Os\n.Sh DESCRIPTION\n.Fo WORD\n.Fa x\n{middle}\n.Fa y\n.Fc\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        assert_eq!(description(&query).trim(), expected, "{middle}");
    }
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
