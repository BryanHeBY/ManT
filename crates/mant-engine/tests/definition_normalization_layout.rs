//! Source-level rendering contracts paired with codec-only normalization tests.

use mant_engine::query_roff_bytes;
use mant_ir::{
    DefinitionItem, Document, inline_plain_text,
    visit::{self, Visit},
};
use mant_render::render_query_text;

fn row<'a>(text: &'a str, token: &str) -> &'a str {
    text.lines()
        .find(|line| line.trim() == token)
        .unwrap_or_else(|| panic!("missing standalone {token:?}: {text}"))
}

fn blank_rows_before(text: &str, token: &str) -> usize {
    let rows = text.lines().collect::<Vec<_>>();
    let index = rows.iter().position(|line| line.trim() == token).unwrap();
    rows[..index]
        .iter()
        .rev()
        .take_while(|line| line.is_empty())
        .count()
}

fn definition<'a>(document: &'a Document, term: &str) -> &'a DefinitionItem {
    struct Definitions<'a>(Vec<&'a DefinitionItem>);
    impl<'a> Visit<'a> for Definitions<'a> {
        fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
            self.0.push(item);
            visit::walk_definition_item(self, item);
        }
    }
    let mut definitions = Definitions(Vec::new());
    definitions.visit_document(document);
    definitions
        .0
        .into_iter()
        .find(|item| {
            item.terms
                .iter()
                .any(|nodes| inline_plain_text(nodes) == term)
        })
        .unwrap_or_else(|| panic!("missing definition {term:?}: {document:?}"))
}

#[test]
fn spaced_relative_continuations_preserve_rows_columns_and_next_owner() {
    for origin in [0, 7] {
        for label in ["-a", "--long-option", "界", "e\u{301}"] {
            let source = format!(
                ".TH PROBE 1\n.SH OPTIONS\n.PD 0\n.RS {origin}\n.IP \"{label}\" 4\nInitial.\n.RS 4\n.sp 1\n.sp 3\nFIRST_CONTINUATION\n.RE\n.RS 8\n.sp 2\n.IP --nested 4\nNESTED_BODY\n.RE\n.RS 4\n.sp 1\nLAST_CONTINUATION\n.RE\n.sp 2\n.IP --next 4\nOUTSIDE_BODY\n.RE\n"
            );
            let content = query_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&content);
            for token in ["FIRST_CONTINUATION", "LAST_CONTINUATION"] {
                assert_eq!(
                    row(&text, token),
                    format!("{}{token}", " ".repeat(origin + 4)),
                    "{source}\n{text}"
                );
            }
            assert_eq!(blank_rows_before(&text, "FIRST_CONTINUATION"), 4, "{text}");
            assert_eq!(blank_rows_before(&text, "LAST_CONTINUATION"), 1, "{text}");
            let document = content.document.as_ref().unwrap();
            let owner = definition(document, label);
            let body = serde_json::to_string(&owner.description).unwrap();
            for token in ["FIRST_CONTINUATION", "NESTED_BODY", "LAST_CONTINUATION"] {
                assert!(body.contains(token), "{body}");
            }
            assert!(!body.contains("OUTSIDE_BODY"), "{body}");
            assert_eq!(owner.layout.inline_term, !label.starts_with("--"));
            assert!(
                serde_json::to_string(&definition(document, "--next").description)
                    .unwrap()
                    .contains("OUTSIDE_BODY")
            );
        }
    }
}

#[test]
fn hanging_paragraph_heads_keep_explicit_space_and_separate_body_rows() {
    for spacing in [0, 1, 3] {
        for head in ["--x", "--long-option-name"] {
            let source = format!(
                ".TH PROBE 1\n.SH OPTIONS\n.PD 0\n.PP\n.B {head}\n.sp {spacing}\n.RS 4\nDESCRIPTION_BODY\n.RE\n.PP\nOutside prose.\n"
            );
            let content = query_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&content);
            assert_eq!(row(&text, head), head, "{source}\n{text}");
            assert_eq!(
                row(&text, "DESCRIPTION_BODY"),
                "    DESCRIPTION_BODY",
                "{source}\n{text}"
            );
            assert_eq!(
                blank_rows_before(&text, "DESCRIPTION_BODY"),
                spacing,
                "{source}\n{text}"
            );
            assert!(
                !definition(content.document.as_ref().unwrap(), head)
                    .layout
                    .inline_term
            );
        }
    }
}

#[test]
fn hanging_heads_keep_source_offsets_without_promoting_run_in_layout() {
    for origin in [0_i32, 2, 5, -2] {
        for offset in [2, 4, 12] {
            for head in ["--x", "--long-option-name"] {
                let source = format!(
                    ".TH PROBE 1\n.SH OPTIONS\n.PD 0\n.RS {origin}\n.PP\n.B {head}\n.RS {offset}\nDESCRIPTION_BODY\n.RE\n.RE\n"
                );
                let content = query_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&content);
                assert_eq!(
                    row(&text, head),
                    format!(
                        "{}{head}",
                        " ".repeat(usize::try_from(origin.max(0)).unwrap())
                    ),
                    "{source}\n{text}"
                );
                assert_eq!(
                    row(&text, "DESCRIPTION_BODY"),
                    format!(
                        "{}DESCRIPTION_BODY",
                        " ".repeat(usize::try_from((origin + offset).max(0)).unwrap())
                    ),
                    "{source}\n{text}"
                );
                assert_eq!(
                    blank_rows_before(&text, "DESCRIPTION_BODY"),
                    0,
                    "{source}\n{text}"
                );
                assert!(
                    !definition(content.document.as_ref().unwrap(), head)
                        .layout
                        .inline_term
                );
            }
        }
    }
}
