//! Source-backed tbl enhancement shares its execution witness across owners.

use super::{Block, REVIEW_HEADER, inline_plain_text, lower};
use mant_ir::{Document, Inline, LinkTarget, visit::Visit};

const PLACEMENTS: [&str; 4] = ["head", "body", "body-xo", "plain"];

fn placed(table: &str, place: &str) -> String {
    match place {
        "head" => format!(".Bl -tag -width Ds\n.It Xo\n{table}.Xc\nOWNER\n.El\n"),
        "body" => format!(".Bl -tag -width Ds\n.It TAG\n{table}OWNER\n.El\n"),
        "body-xo" => format!(".Bl -tag -width Ds\n.It TAG\n.Xo\n{table}.Xc\nOWNER\n.El\n"),
        "plain" => format!("{table}OWNER\n"),
        _ => unreachable!("closed source fixture positions"),
    }
}

fn cell_inlines<'a>(doc: &'a Document, place: &str) -> &'a [Inline] {
    cell_inlines_in(&doc.sections[1].blocks, place)
}

fn cell_inlines_in<'a>(blocks: &'a [Block], place: &str) -> &'a [Inline] {
    let body = if place == "plain" {
        blocks
    } else {
        let [Block::DefinitionList { items, .. }] = blocks else {
            panic!("expected one definition list: {blocks:?}");
        };
        let [item] = items.as_slice() else {
            panic!("expected one definition")
        };
        if place == "head" {
            let [term] = item.terms.as_slice() else {
                panic!("expected one HEAD term")
            };
            assert!(!inline_plain_text(term).contains("OWNER"));
            return &term.content;
        }
        &item.description
    };
    let Some(Block::Table { rows, .. }) = body.first() else {
        panic!("expected native block table: {body:?}");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected one recovered text cell");
    };
    children
}

#[test]
fn implicit_table_names_execute_the_page_spelling_and_leave_explicit_names_unchanged() {
    // deroff() preserves these escape spellings in meta.name. post_nm()
    // inserts them as generated TEXT. All 30 fixed profiles witness the
    // ordinary Nm next to the table, whose own raw language is UNSUPP.
    for (name, visible) in [
        (r"foo\-bar", "foo-bar"),
        (r"foo\(dqbar", "foo\"bar"),
        (r"\fBfoo\fP", "foo"),
        (r"\[u03B1]foo", "αfoo"),
        (r"\efoo", r"\foo"),
    ] {
        let header = REVIEW_HEADER.replace(".Nm probe\n", &format!(".Nm {name}\n"));
        for place in ["head", "body"] {
            for (fragment, expected) in [
                (".Nm", visible.to_owned()),
                (".Pq Nm", format!("({visible})")),
                (".Nm EXPLICIT", "EXPLICIT".into()),
            ] {
                let table = format!(".TS\nl.\nT{{\n{fragment}\nT}}\n.TE\n");
                let part = placed(&table, place).replace("OWNER\n", "ENDING\n");
                let source = format!("{header}.Nm\n{part}.Sh EXAMPLES\nTAIL\n");
                let query = lower(&source);
                let document = query.document.as_ref().unwrap();
                let (ordinary_name, table_owner) =
                    document.sections[1].blocks.split_first().unwrap();
                let Block::Paragraph { children, .. } = ordinary_name else {
                    panic!("wanted the ordinary name control: {source}");
                };
                assert_eq!(inline_plain_text(children), visible, "{source}");
                assert_eq!(
                    inline_plain_text(cell_inlines_in(table_owner, place)),
                    expected,
                    "{source}"
                );
            }
        }
    }
}

struct Targets(Vec<LinkTarget>);

impl<'a> Visit<'a> for Targets {
    fn visit_inline(&mut self, node: &'a Inline) {
        if let Inline::Link { target, .. } = node {
            self.0.push(target.clone());
        }
        mant_ir::visit::walk_inline(self, node);
    }
}

fn assert_cell(doc: &Document, place: &str, expected: &str, email: bool, source: &str) {
    let nodes = cell_inlines(doc, place);
    assert_eq!(inline_plain_text(nodes), expected, "{source}");
    let mut links = Targets(Vec::new());
    for node in nodes {
        links.visit_inline(node);
    }
    let expected_targets = if email {
        vec![LinkTarget::Email {
            address: expected.into(),
        }]
    } else {
        Vec::new()
    };
    assert_eq!(links.0, expected_targets, "{source}");
    links.0.clear();
    links.visit_document(doc);
    assert_eq!(links.0, expected_targets, "{source}");
}

#[test]
fn source_recovery_preserves_words_and_email_identity_in_every_owner() {
    // Fixed roff_parsetext() and tbl_cdata() ran all 32 exact sources in
    // five native profiles. The compatibility enhancement uses the same
    // authored T{} evidence in a HEAD term as it does in a table block.
    for place in PLACEMENTS {
        for (mac, plain, enhanced, email) in [
            (
                ".Mt a@example.test",
                "a@example.test",
                "a@example.test",
                true,
            ),
            (".Fl Fl help", "Fl help", "--help", false),
            (".No AA Ns No BB", "AA Ns No BB", "AABB", false),
            ("PLAIN", "PLAIN", "PLAIN", false),
        ] {
            for source_block in [false, true] {
                let cell = if source_block {
                    format!("T{{\n{mac}\nT}}")
                } else {
                    plain.into()
                };
                let table = format!(".TS\nl.\n{cell}\n.TE\n");
                let source = format!("{REVIEW_HEADER}{}", placed(&table, place));
                let query = lower(&source);
                let expected = if source_block { enhanced } else { plain };
                assert_cell(
                    query.document.as_ref().unwrap(),
                    place,
                    expected,
                    email && source_block,
                    &source,
                );
                let markdown = mant_codec::encode::render_markdown(&query);
                let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
                let text = mant_render::render_query_text(&readback);
                assert_eq!(text.matches(expected).count(), 1, "{source}\n{markdown}");
                assert_eq!(text.matches("OWNER").count(), 1, "{source}\n{markdown}");
                if place == "head" && email && source_block {
                    let mut readback_links = Targets(Vec::new());
                    readback_links.visit_document(readback.document.as_ref().unwrap());
                    assert_eq!(
                        readback_links.0,
                        [LinkTarget::Email {
                            address: expected.into()
                        }]
                    );
                }
                // An owned AST without the original source has the native
                // operand stream only, including escaped or opaque macros.
                let report = libmandoc_rs::Parser::default()
                    .parse_bytes("fallback.1", source.as_bytes())
                    .unwrap();
                let native =
                    mant_codec::lower_mandoc_document(std::path::Path::new("fallback.1"), &report);
                assert_cell(&native, place, plain, false, &source);
            }
        }
    }
}

#[test]
fn redefined_table_macros_never_recover_as_builtin_links() {
    // All four exact CVS trees retain REDEFINED in the original T{} cell;
    // the authored spelling `.Mt` cannot override this native witness.
    let table = ".TS\nl.\nT{\n.Mt a@example.test\nT}\n.TE\n";
    for place in PLACEMENTS {
        let source = format!(
            "{REVIEW_HEADER}.de Mt\nREDEFINED\n..\n{}",
            placed(table, place)
        );
        let query = lower(&source);
        assert_cell(
            query.document.as_ref().unwrap(),
            place,
            "REDEFINED",
            false,
            &source,
        );
    }
}

#[test]
fn table_name_recovery_uses_only_the_real_page_default() {
    // post_nm() inserts meta.name as NODE_NOSRC for an implicit Nm, even
    // under Pq. The pinned tbl parser emits only the raw operands, with
    // the expected UNSUPP diagnostic. Same-source owner comparisons
    // verify the bounded ManT enhancement, not a native extension.
    let alternative_page = REVIEW_HEADER.replace(".Nm probe\n", ".Nm actual-tool\n");
    let no_name = REVIEW_HEADER.replace(".Sh NAME\n.Nm probe\n.Nd exercise descriptors\n", "");
    for (header, name) in [
        (REVIEW_HEADER, Some("probe")),
        (alternative_page.as_str(), Some("actual-tool")),
        (no_name.as_str(), None),
    ] {
        for place in PLACEMENTS {
            for (fragment, native, enhanced, link) in [
                (".Nm", "", name.unwrap_or(""), false),
                (".Nm EXPLICIT", "EXPLICIT", "EXPLICIT", false),
                (".Pq Nm", "Nm", name.unwrap_or("Nm"), false),
                (".Pq Nm EXPLICIT", "Nm EXPLICIT", "EXPLICIT", false),
                (
                    ".Nm\n.Lk https://example.test LINK",
                    "https://example.test LINK",
                    name.unwrap_or(""),
                    true,
                ),
            ] {
                let table = format!(".TS\nl.\nT{{\n{fragment}\nT}}\n.TE\n");
                let source = format!(
                    "{header}{}",
                    placed(&table, place).replace("OWNER\n", "ENDING\n")
                ) + ".Sh EXAMPLES\nTAIL\n";
                let query = lower(&source);
                let body = mant_render::render_query_text(&query);
                assert!(!body.contains("table-fragment"), "{source}\n{body}");
                assert_eq!(body.matches("ENDING").count(), 1, "{source}");
                if name.is_none() {
                    if fragment.contains("EXPLICIT") {
                        assert!(body.contains("EXPLICIT"), "{source}");
                    }
                    continue;
                }
                let document = query.document.as_ref().unwrap();
                let nodes = cell_inlines(document, place);
                let projected = inline_plain_text(nodes);
                if link {
                    assert!(projected.starts_with(enhanced), "{source}\n{projected}");
                    let mut links = Targets(Vec::new());
                    links.visit_document(document);
                    assert_eq!(
                        links.0,
                        [LinkTarget::External {
                            uri: "https://example.test".into()
                        }],
                        "{source}"
                    );
                } else if fragment.starts_with(".Pq") {
                    assert_eq!(projected, format!("({enhanced})"), "{source}");
                } else {
                    assert_eq!(projected, enhanced, "{source}");
                }
                let report = libmandoc_rs::Parser::default()
                    .parse_bytes("fallback.1", source.as_bytes())
                    .unwrap();
                let raw =
                    mant_codec::lower_mandoc_document(std::path::Path::new("fallback.1"), &report);
                if !native.is_empty() {
                    let raw_text = inline_plain_text(cell_inlines(&raw, place));
                    let words = raw_text.split_whitespace().collect::<Vec<_>>().join(" ");
                    assert_eq!(words, native, "{source}\n{raw_text}");
                }
            }
        }
    }
}
