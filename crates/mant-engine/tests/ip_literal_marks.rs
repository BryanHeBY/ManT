//! IP renders an authored tag, not an inferred disposable punctuation mark.
use mant_ir::{
    DefinitionItem, Document,
    visit::{self, Visit},
};
use mant_protocol::{ExplanationOptions, ExplanationQuery};

fn definitions(document: &Document) -> Vec<&DefinitionItem> {
    struct Items<'a>(Vec<&'a DefinitionItem>);
    impl<'a> Visit<'a> for Items<'a> {
        fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
            self.0.push(item);
            visit::walk_definition_item(self, item);
        }
    }
    let mut result = Items(Vec::new());
    result.visit_document(document);
    result.0
}

#[test]
fn styled_punctuation_keys_keep_labels_body_and_explanation() {
    for (mark, expected) in [
        ("#", "#"),
        ("=", "="),
        (r"\e", "\\"),
        ("*", "*"),
        ("^", "^"),
        ("$", "$"),
        ("o", "o"),
        ("-", "-"),
        ("+", "+"),
    ] {
        let source = format!(".TH KEYS 1\n.SH COMMANDS\n.IP \"\\fB{mark}\\fP\" 10\nKEY_BODY\n");
        let bundle = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let items = definitions(bundle.document.as_ref().unwrap());
        assert_eq!(items.len(), 1);
        assert_eq!(mant_ir::inline_plain_text(&items[0].terms[0]), expected);
        let entry = items[0].entry.as_ref().expect("explicit styled key");
        assert_eq!(entry.kind, mant_ir::EntryKind::Term);
        assert_eq!(entry.names, [expected]);
        let rendered = mant_render::render_query_text(&bundle);
        assert!(
            rendered
                .lines()
                .any(|line| line.trim_start().starts_with(&format!("{expected} "))),
            "{rendered}"
        );
        assert!(rendered.contains("KEY_BODY"));
        let explanation = mant_query::explain_query(
            &bundle,
            &ExplanationQuery {
                entry: expected.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert!(
            explanation
                .evidence
                .iter()
                .any(|evidence| evidence.source == items[0].source),
            "{expected}: {explanation:?}"
        );
    }
}

#[test]
fn unstyled_marks_are_presentation_and_styled_keys_are_not_option_values() {
    for mark in ["*", "o", "-", "+", "#", "=", "^", "$"] {
        for styled in [false, true] {
            let tag = if styled {
                format!(r"\fB{mark}\fP")
            } else {
                mark.into()
            };
            let source = format!(
                ".TH KEYS 1\n.SH OPTIONS\n.TP\n.B --mode\nMode description.\n.RS\n.IP \"{tag}\" 4\nMARK_BODY\n.RE\n"
            );
            let bundle = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let items = definitions(bundle.document.as_ref().unwrap());
            let item = items
                .iter()
                .find(|item| {
                    item.terms
                        .iter()
                        .any(|term| mant_ir::inline_plain_text(term).trim() == mark)
                })
                .expect("authored tag retained");
            if styled {
                let entry = item.entry.as_ref().expect("styled key");
                assert_eq!(entry.kind, mant_ir::EntryKind::Term);
                assert_eq!(entry.names, [mark]);
            } else {
                assert!(item.entry.is_none(), "{mark}: {:?}", item.entry);
            }
            assert!(
                items.iter().all(|item| item
                    .entry
                    .as_ref()
                    .is_none_or(|entry| entry.kind != mant_ir::EntryKind::Value)),
                "no fabricated accepted value"
            );
            assert!(mant_render::render_query_text(&bundle).contains("MARK_BODY"));
        }
    }
}

#[test]
fn licensed_gcc_and_rsync_marks_do_not_add_semantic_nodes() {
    for (fixture, mark, expected) in [
        ("archlinux/rsync.1.zst", "o", 157),
        ("fedora44/gcc.1.zst", "*", 102),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/roff/real")
            .join(fixture);
        let document = mant_loader::parse_manual_source(&path).unwrap();
        let items = definitions(&document);
        let marked = items
            .iter()
            .filter(|item| {
                item.terms.len() == 1 && mant_ir::inline_plain_text(&item.terms[0]).trim() == mark
            })
            .collect::<Vec<_>>();
        assert_eq!(marked.len(), expected, "{fixture}");
        assert!(
            marked.iter().all(|item| item.entry.is_none()),
            "{fixture}: marks must not become Term or Value entries"
        );
    }
}

#[test]
fn licensed_posix_sh_editor_keys_retain_their_exact_source_owners() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/archlinux/sh.1p.gz");
    let document = mant_loader::parse_manual_source(&path).unwrap();
    let bundle = mant_ir::ResolvedContent {
        label: "sh".into(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let items = definitions(bundle.document.as_ref().unwrap());
    for (line, key) in [
        (668, "#"),
        (675, "="),
        (704, "\\"),
        (728, "*"),
        (882, "^"),
        (886, "$"),
    ] {
        let item = items
            .iter()
            .find(|item| item.source.is_some_and(|source| source.line == line))
            .unwrap_or_else(|| panic!("missing sh source owner at line {line}"));
        assert_eq!(mant_ir::inline_plain_text(&item.terms[0]), key);
        let explanation = mant_query::explain_query(
            &bundle,
            &ExplanationQuery {
                entry: key.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert!(
            explanation
                .evidence
                .iter()
                .any(|evidence| evidence.source == item.source),
            "{key}: {explanation:?}"
        );
    }
}
