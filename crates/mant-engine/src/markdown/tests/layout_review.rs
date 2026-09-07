//! Source-topology regressions from the layout/semantic-entry review.
use super::*;

#[test]
fn literal_anchor_code_is_searchable_and_cannot_steal_entry_ownership() {
    for heading in ["", "## Section\n\n"] {
        for literal in [
            "```html\n<a id=\"option-alpha\"></a>\nSENTINEL\n```",
            "`<a id=\"option-alpha\"></a>` SENTINEL",
        ] {
            let source = format!(
                "# Tool\n\n{heading}{literal}\n\nA separate paragraph.\n\n<!-- mant:entries role=option case=sensitive -->\n- `--alpha`: Alpha description.\n\n{literal}\n"
            );
            let document = parse_markdown(&source, None).unwrap().document;
            let query = ResolvedContent {
                label: "tool".into(),
                address: None,
                document: Some(document),
                tldr: None,
            };
            for scope in [SearchScope::Visible, SearchScope::Markdown] {
                for pattern in ["<a", "SENTINEL", "separate"] {
                    let result = search_query(
                        &query,
                        &SearchQuery {
                            pattern: pattern.into(),
                            syntax: SearchSyntax::Literal,
                            case: SearchCase::Sensitive,
                            scope,
                            word: false,
                            context_lines: 1,
                            offset: 0,
                            limit: 100,
                        },
                    )
                    .unwrap();
                    assert!(result.total > 0, "{source}: {pattern}: {scope:?}");
                    for hit in result.matches {
                        assert!(
                            !matches!(hit.outline.node, OutlineNodeReference::DocumentEntry { .. }),
                            "{pattern}: {hit:?}"
                        );
                        assert!(
                            hit.occurrences
                                .iter()
                                .any(|occurrence| occurrence.matched_text.contains(pattern))
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn environment_assignments_retain_punctuation_in_values() {
    for form in ["FOO=one,two", "FOO=one|two"] {
        let parsed = parse_markdown(&format!("# Tool\n\n<!-- mant:entries role=environment-variable case=sensitive -->\n- `{form}`: Set values.\n"), None).unwrap();
        let index = mant_ir::SemanticIndex::build(&parsed.document);
        assert_eq!(index.root()[0].aliases, ["FOO"]);
        assert_eq!(index.root()[0].forms, [form]);
    }
}

#[test]
fn decoded_fragments_are_exact_and_not_normalized_a_second_time() {
    use mant_ir::visit::{Visit, walk_inline};
    struct Targets(Vec<String>);
    impl<'a> Visit<'a> for Targets {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Link {
                target: mant_ir::LinkTarget::Section { id },
                ..
            } = inline
            {
                self.0.push(id.to_string());
            }
            walk_inline(self, inline);
        }
    }
    let source = "# Tool\n\n## First {#foo}\n\nFIRST\n\n## Second {##foo}\n\nSECOND\n\n## Third {###foo}\n\nTHIRD\n\n## Percent {#%23foo}\n\n[one](#foo) [two](#%23foo) [direct](##foo) [three](#%23%23foo) [percent](#%2523foo)\n";
    let parsed = parse_markdown(source, None).unwrap();
    assert!(
        parsed.document.diagnostics.is_empty(),
        "{:?}",
        parsed.document.diagnostics
    );
    let mut targets = Targets(Vec::new());
    targets.visit_document(&parsed.document);
    assert_eq!(targets.0, ["foo", "second", "second", "third", "percent"]);
    for fragment in ["%20foo", "foo%20", "FOO", "missing"] {
        let parsed = parse_markdown(
            &format!("# Tool\n\n## First {{#foo}}\n\n[bad](#{fragment})\n"),
            None,
        )
        .unwrap();
        assert!(
            parsed
                .document
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("ir.dangling-section-link")),
            "{fragment}"
        );
    }
}

#[test]
fn removing_directives_never_merges_independent_lists_or_roles() {
    for newline in ["\n", "\r\n"] {
        for first in [
            "<!-- mant:entries role=command case=sensitive -->\n- `run`: Run.",
            "- ordinary",
        ] {
            for marker in ["-", "*"] {
                let source = format!("# Tool\n\n{first}\n\n<!-- mant:entries role=value case=insensitive -->\n{marker} `auto`: Automatic.\n").replace('\n', newline);
                let parsed = parse_markdown(&source, None).unwrap();
                assert!(
                    parsed.document.diagnostics.is_empty(),
                    "{source}: {:?}",
                    parsed.document.diagnostics
                );
                assert_eq!(parsed.document.blocks.len(), 2);
                let index = mant_ir::SemanticIndex::build(&parsed.document);
                let entries = index.root();
                assert_eq!(entries.last().unwrap().kind, EntryKind::Value);
                assert_eq!(entries.last().unwrap().aliases, ["auto"]);
            }
        }
        let source = "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--color WHEN`: Color.\n\n  <!-- mant:domain choices=exhaustive -->\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n  <!-- mant:entries role=command case=sensitive -->\n  - `run`: A command.\n".replace('\n', newline);
        let parsed = parse_markdown(&source, None).unwrap();
        let index = mant_ir::SemanticIndex::build(&parsed.document);
        let entry = &index.root()[0];
        assert_eq!(entry.children[0].kind, EntryKind::Value);
        assert_eq!(entry.children[1].kind, EntryKind::Command);
        assert!(!matches!(
            entry.value_domain,
            Some(mant_ir::ValueDomain::Choices { exhaustive: true })
        ));
        assert!(
            parsed
                .document
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("markdown.semantic-value-domain"))
        );
    }
}
