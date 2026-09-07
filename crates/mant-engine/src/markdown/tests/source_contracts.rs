//! Markdown source coordinates, fragment decoding and list topology.
use super::*;

#[test]
fn entry_search_sources_point_into_original_bytes_for_each_line_ending() {
    for newline in ["\n", "\r\n", "\r"] {
        let source = "# Tool\n\n## Commands\n\n`<a id=\"command-run\"></a>` OUTSIDE\n\n<!-- mant:entries role=command case=sensitive -->\n- `run`: OWNEDPAYLOAD\n".replace('\n', newline);
        let query = crate::query_markdown_text(&source, None).unwrap();
        let found = search_query(
            &query,
            &SearchQuery {
                pattern: "OWNEDPAYLOAD".into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope: SearchScope::Visible,
                word: false,
                context_lines: 1,
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(found.matches.len(), 1);
        let hit = &found.matches[0];
        assert!(
            matches!(&hit.outline.node, OutlineNodeReference::DocumentEntry { names, .. } if names == &["run"])
        );
        let span = hit.node_source.unwrap();
        assert_eq!(span.line, 8);
        let bytes = span.byte_range.unwrap();
        let original = &source[bytes.start.get() as usize..bytes.end.get() as usize];
        assert!(original.contains("`run`: OWNEDPAYLOAD"), "{original:?}");
        assert!(!original.contains("OUTSIDE"));
    }
}

#[test]
fn list_tightness_comes_from_direct_parser_items_not_source_substrings() {
    use mant_ir::visit::{Visit, walk_block};
    struct Lists(Vec<bool>);
    impl<'a> Visit<'a> for Lists {
        fn visit_block(&mut self, block: &'a Block) {
            if let Block::List { compact, .. } | Block::DefinitionList { compact, .. } = block {
                self.0.push(*compact);
            }
            walk_block(self, block);
        }
    }
    for newline in ["\n", "\r\n", "\r"] {
        for blank in ["", "  ", "\t"] {
            for (body, expected) in [
                ("- first\n- second".into(), vec![true]),
                (format!("- first\n{blank}\n- second"), vec![false]),
                (format!("1. first\n{blank}\n2. second"), vec![false]),
                (
                    format!(
                        "<!-- mant:entries role=command case=sensitive -->\n- `first`: First.\n{blank}\n- `second`: Second."
                    ),
                    vec![false],
                ),
                (
                    format!(
                        "- outer\n  - nested first\n{blank}\n  - nested second\n- outer second"
                    ),
                    vec![true, false],
                ),
                (
                    format!(
                        "- outer\n  - nested first\n  - nested second\n{blank}\n- outer second"
                    ),
                    vec![false, true],
                ),
            ] {
                let source = format!("# Tool\n\n{body}\n").replace('\n', newline);
                let parsed = parse_markdown(&source, None).unwrap();
                assert!(
                    parsed.document.diagnostics.is_empty(),
                    "{source:?}: {:?}",
                    parsed.document.diagnostics
                );
                let mut lists = Lists(Vec::new());
                lists.visit_document(&parsed.document);
                assert_eq!(lists.0, expected, "{source:?}");
            }
            let source = format!("# Tool\n\nfirst\n{blank}\nsecond\n").replace('\n', newline);
            let parsed = parse_markdown(&source, None).unwrap();
            let [
                _,
                Block::Paragraph {
                    layout,
                    source: Some(span),
                    ..
                },
            ] = parsed.document.blocks.as_slice()
            else {
                panic!("{:?}", parsed.document)
            };
            assert_eq!(span.line, 5, "{source:?}");
            assert!(layout.spacing_before_lines > 0, "{source:?}: {layout:?}");
        }
    }
}

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
