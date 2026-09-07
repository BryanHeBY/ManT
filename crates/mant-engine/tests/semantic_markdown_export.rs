//! Semantic export is an opt-in subset, not a lossless Markdown serializer.
use mant_engine::{MarkdownOptions, query_markdown_text, render_markdown_with_options};
use mant_ir::{
    Document, EntryFacts, ListItem,
    visit::{self, Visit},
};

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
    let query = query_markdown_text(source, None).unwrap();
    assert!(query.document.as_ref().unwrap().diagnostics.is_empty());
    let original = facts(query.document.as_ref().unwrap());
    for preserve_anchors in [false, true] {
        let markdown = render_markdown_with_options(
            &query,
            MarkdownOptions {
                preserve_anchors,
                preserve_semantics: true,
            },
        );
        assert!(markdown.contains("mant:entry"), "{markdown}");
        let reparsed = query_markdown_text(&markdown, None).unwrap();
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
            assert_eq!(a.role, b.role);
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
        let query = query_markdown_text(source, None).unwrap();
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
        mant_engine::query_roff_bytes(b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -h, --help\nHelp.\n")
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
