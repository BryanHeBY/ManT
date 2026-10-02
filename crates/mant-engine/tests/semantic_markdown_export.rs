//! Semantic export is an opt-in subset, not a lossless Markdown serializer.
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{
    Document, EntryFacts, ListItem,
    visit::{self, Visit},
};
use mant_loader::{load_markdown_text, load_roff_bytes};

#[test]
fn no_fill_function_target_survives_addressable_markdown_export() {
    // Exact source checked with pinned CVS -Ttree/-Thtml: post_tg attaches
    // the target to Fo HEAD, and mdoc_html.c puts id=call on its visible Fn.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n.Tg call\n.Fo call\n.Fa first\n.Fa second\n.Fc\n.fi\n";
    let query = load_roff_bytes(source).unwrap();
    let markdown = render_markdown_with_options(
        &query,
        MarkdownOptions {
            preserve_anchors: true,
            preserve_semantics: false,
            native_text: false,
        },
    );
    assert_eq!(
        markdown.matches("<a id=\"call\"></a>").count(),
        1,
        "{markdown}"
    );
    assert!(
        markdown.contains("<a id=\"call\"></a>\n\n```"),
        "{markdown}"
    );
    assert!(markdown.contains("call(\nfirst,\nsecond)"), "{markdown}");
}

#[test]
fn literal_display_target_survives_addressable_markdown_export() {
    // Exact input checked with pinned CVS -Thtml. mdoc_html.c renders the
    // attached Tg destination on the visible No inside the literal pre.
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Tg inner\n.No inner\n.Ed\n";
    let query = load_roff_bytes(source).unwrap();
    let markdown = render_markdown_with_options(
        &query,
        MarkdownOptions {
            preserve_anchors: true,
            preserve_semantics: false,
            native_text: false,
        },
    );
    assert_eq!(
        markdown.matches("<a id=\"inner\"></a>").count(),
        1,
        "{markdown}"
    );
    assert!(
        markdown.contains("<a id=\"inner\"></a>\n\n```"),
        "{markdown}"
    );
    assert!(markdown.contains("```\ninner\n```"), "{markdown}");
}

fn facts(doc: &Document) -> Vec<EntryFacts> {
    struct Entries(Vec<EntryFacts>);
    impl<'a> Visit<'a> for Entries {
        fn visit_list_item(&mut self, item: &'a ListItem) {
            if let Some(facts) = &item.entry {
                self.0.push(facts.clone());
            }
            visit::walk_list_item(self, item);
        }
    }
    let mut result = Entries(Vec::new());
    result.visit_document(doc);
    result.0
}

#[test]
fn ordinary_declared_entries_reimport_names_groups_relations_and_domains() {
    let source = r#"# Probe

<!-- mant:entries role=option case=sensitive -->
- `-h`, `--help`: Help. <!-- mant:entry {"id":"help","aliasGroups":[["-h","--help"]]} -->
- `--assist`: More help. <!-- mant:entry {"id":"assist","aliasOf":"help"} -->
- `--mode MODE`: Mode. <!-- mant:entry {"id":"mode"} -->

  <!-- mant:domain choices=exhaustive -->

  <!-- mant:entries role=value case=sensitive -->
  - `auto`: Automatic.
  - `manual`: Manual.

- `--key KEY`: Remote keys.

  <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->
"#;
    let query = load_markdown_text(source, None).unwrap();
    assert_eq!(query.document.as_ref().unwrap().diagnostics.len(), 0);
    let original = facts(query.document.as_ref().unwrap());
    for preserve_anchors in [false, true] {
        let markdown = render_markdown_with_options(
            &query,
            MarkdownOptions {
                preserve_anchors,
                preserve_semantics: true,
                native_text: false,
            },
        );
        assert!(markdown.contains("mant:entry"), "{markdown}");
        let reparsed = load_markdown_text(&markdown, None).unwrap();
        let document = reparsed.document.unwrap();
        assert!(
            document.diagnostics.is_empty(),
            "{:?}\n{markdown}",
            document.diagnostics
        );
        let roundtrip = facts(&document);
        assert_eq!(roundtrip.len(), original.len());
        for (a, b) in original.iter().zip(roundtrip) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.names, b.names);
            assert_eq!(a.alias_groups, b.alias_groups);
            assert_eq!(a.alias_of, b.alias_of);
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.case, b.case);
            // Relationship source spans belong to the new source, not the exporter.
            match (&a.value_domain, &b.value_domain) {
                (
                    Some(mant_ir::ValueDomain::EntrySet {
                        reference: ar,
                        entry_kinds: ak,
                        ..
                    }),
                    Some(mant_ir::ValueDomain::EntrySet {
                        reference: br,
                        entry_kinds: bk,
                        ..
                    }),
                ) => {
                    assert_eq!(ar, br);
                    assert_eq!(ak, bk);
                }
                (a, b) => assert_eq!(a, b),
            }
        }
    }
    assert!(
        !render_markdown_with_options(&query, MarkdownOptions::default()).contains("mant:entry")
    );
}

#[test]
fn unsupported_partial_or_native_owners_keep_content_without_invented_relations() {
    for source in [
        "<!-- mant:entries role=option case=sensitive -->\n- `--good`: Good.\n- `--bad`\n",
        "- `--implicit`: An inferred option.\n",
    ] {
        let query = load_markdown_text(source, None).unwrap();
        let markdown = render_markdown_with_options(
            &query,
            MarkdownOptions {
                preserve_semantics: true,
                ..Default::default()
            },
        );
        assert!(!markdown.contains("mant:entry"));
        assert!(markdown.contains("--"));
    }
    let query =
        mant_loader::load_roff_bytes(b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -h, --help\nHelp.\n")
            .unwrap();
    let markdown = render_markdown_with_options(
        &query,
        MarkdownOptions {
            preserve_semantics: true,
            ..Default::default()
        },
    );
    assert!(!markdown.contains("aliasGroups"));
    assert!(markdown.contains("Help."));
}

struct Metadata {
    groups: Vec<Vec<String>>,
    id: String,
}
impl visit::VisitMut for Metadata {
    fn visit_list_item_mut(&mut self, item: &mut ListItem) {
        if let Some(entry) = &mut item.entry {
            entry.alias_groups.clone_from(&self.groups);
            entry.id = self.id.clone().into();
        }
        visit::walk_list_item_mut(self, item);
    }
}

/// Public IR producers need not obey Markdown's authoring-size limits.
#[test]
fn metadata_representation_limits_are_shared_by_import_and_export() {
    use mant_ir::visit::VisitMut;

    // Count limits, ID limits, and payload bytes are independent constraints.
    for (group_count, members, name_padding, id_length, supported) in [
        (32, 2, 0, 4, true),
        (1, 32, 0, 4, true),
        (33, 2, 0, 4, false),
        (1, 33, 0, 4, false),
        (1, 34, 0, 4, false),
        (1, 2, 0, 512, true),
        (1, 2, 0, 513, false),
        (32, 2, 140, 4, false),
    ] {
        let names: Vec<_> = (0..group_count * members)
            .map(|i| format!("--alias{i}{}", "x".repeat(name_padding)))
            .collect();
        let head = names
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let source =
            format!("<!-- mant:entries role=option case=sensitive -->\n- {head}: Kept body.\n");
        let mut query = load_markdown_text(&source, None).unwrap();
        let document = query.document.as_mut().unwrap();
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        Metadata {
            groups: names.chunks(members).map(<[String]>::to_vec).collect(),
            id: "i".repeat(id_length),
        }
        .visit_document_mut(document);
        assert_eq!(mant_ir::validate_document(document).len(), 0);
        let original = facts(document);
        assert_eq!(original.len(), 1);
        let json = serde_json::json!({
            "id": original[0].id,
            "aliasGroups": original[0].alias_groups,
        });
        let authored = format!("{} <!-- mant:entry {json} -->\n", source.trim_end());
        let imported = load_markdown_text(&authored, None)
            .unwrap()
            .document
            .unwrap();
        assert_eq!(
            imported.diagnostics.is_empty(),
            supported,
            "authoring and export limits disagree: {:?}",
            imported.diagnostics
        );
        for preserve_anchors in [false, true] {
            let markdown = render_markdown_with_options(
                &query,
                MarkdownOptions {
                    preserve_anchors,
                    preserve_semantics: true,
                    native_text: false,
                },
            );
            assert_eq!(
                markdown.contains("mant:entry"),
                supported,
                "groups={group_count}, members={members}, id={id_length}, padding={name_padding}"
            );
            if supported {
                let document = load_markdown_text(&markdown, None)
                    .unwrap()
                    .document
                    .unwrap();
                assert!(
                    document.diagnostics.is_empty(),
                    "{:?}",
                    document.diagnostics
                );
                assert_eq!(mant_ir::validate_document(&document).len(), 0);
                assert_eq!(facts(&document), original);
            } else {
                let ordinary = render_markdown_with_options(
                    &query,
                    MarkdownOptions {
                        preserve_anchors,
                        preserve_semantics: false,
                        native_text: false,
                    },
                );
                assert_eq!(markdown, ordinary);
                assert!(!markdown.contains("mant:entries"));
                assert!(markdown.contains("Kept body."));
                for name in &names {
                    assert!(markdown.contains(name));
                }
            }
        }
    }
}
