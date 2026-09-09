//! Markdown source coordinates, fragment decoding and list topology.
use super::*;

#[test]
fn thematic_rule_source_gaps_survive_root_section_and_nested_list_lowering() {
    for prefix in [
        "",
        "## Details\n\n",
        "- Container\n\n",
        "- Outer\n\n  - Inner\n\n",
    ] {
        let indent = match prefix {
            "- Container\n\n" => "  ",
            "- Outer\n\n  - Inner\n\n" => "    ",
            _ => "",
        };
        for blank in [false, true] {
            let separator = if blank { "\n\n" } else { "\n" };
            let body = ["BEFORE", "***", "AFTER"]
                .map(|line| format!("{indent}{line}"))
                .join(separator);
            let source = format!("{prefix}{body}\n");
            let query = crate::query_markdown_text(&source, None).unwrap();
            let document = query.document.as_ref().unwrap();
            let mut blocks = if document.sections.is_empty() {
                document.blocks.as_slice()
            } else {
                document.sections[0].blocks.as_slice()
            };
            while let Some(Block::List { items, .. }) = blocks.first() {
                blocks = &items[0].blocks;
                if let Some(index) = blocks
                    .iter()
                    .position(|block| matches!(block, Block::List { .. }))
                {
                    blocks = &blocks[index..];
                }
            }
            let rule = blocks
                .iter()
                .position(|block| matches!(block, Block::ThematicBreak { .. }))
                .unwrap();
            assert_eq!(
                matches!(blocks[rule - 1], Block::VerticalSpace { lines: 1, .. }),
                blank,
                "{source}"
            );
            let Block::Paragraph { layout, .. } = &blocks[rule + 1] else {
                panic!("rule must retain its following paragraph: {source}");
            };
            assert_eq!(layout.spacing_before_lines, u16::from(blank), "{source}");
            let text = crate::render_query_text(&query);
            let lines = text.lines().collect::<Vec<_>>();
            let before = lines
                .iter()
                .position(|line| line.contains("BEFORE"))
                .unwrap();
            let rule = lines.iter().position(|line| line.trim() == "---").unwrap();
            let after = lines
                .iter()
                .position(|line| line.contains("AFTER"))
                .unwrap();
            assert_eq!(rule - before, 1 + usize::from(blank), "{source}\n{text}");
            assert_eq!(after - rule, 1 + usize::from(blank), "{source}\n{text}");

            let mut repeated = document.clone();
            super::super::layout::normalize_markdown_layout(
                &super::super::source::MarkdownSource::new(&source),
                &mut repeated.blocks,
                &mut repeated.sections,
            );
            assert_eq!(
                &repeated, document,
                "normalization must not duplicate gaps: {source}"
            );
        }
    }
}

#[test]
fn annotations_preserve_the_original_event_tree_and_every_visible_delimiter() {
    use mant_ir::visit::{VisitMut, walk_list_item_mut};
    struct EraseFacts;
    impl VisitMut for EraseFacts {
        fn visit_list_item_mut(&mut self, item: &mut mant_ir::ListItem) {
            item.entry = None;
            walk_list_item_mut(self, item);
        }
    }
    for newline in ["\n", "\r\n", "\r"] {
        for body in [
            "<!-- mant:entries role=option case=sensitive -->\n- `-h`, `--help`: Help.  \n  Next line.\n- `--color WHEN` — Color.\n",
            "<!-- mant:entries role=command case=sensitive -->\n7. [`get`](get.md) / `fetch` | `get all`: Get **body**.\n\n8. `put`: Put.\n\n   <!-- mant:entries role=option case=sensitive -->\n   - `-f`: Child.\n",
            "<!-- mant:entries role=command case=insensitive -->\n- `good`: Valid.\n- This remains visible prose.\n- `other`: Also valid.\n",
            "<!-- mant:entries role=option case=sensitive -->\n- `--mode MODE`: Choose.\n\n  <!-- mant:domain choices=exhaustive -->\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n  - `manual`\n",
            "<!-- mant:entries role=option case=sensitive -->\n4. `-h`, `--help`: Help.  \n   Hard break. <!-- mant:entry {\"id\":\"help\",\"aliasGroups\":[[\"-h\",\"--help\"]]} -->\n\n5. `--more` — More. <!-- mant:entry {\"id\":\"more\",\"aliasOf\":\"help\"} -->\n",
            "<!-- mant:entries role=option case=sensitive -->\n- `--help`: Help. <!-- mant:entry {\"unknown\":true} -->\n\n  <!-- mant:entries role=command case=sensitive -->\n  - `go` | `go all`: [Details](other.md). <!-- mant:entry {\"id\":\"go\"} -->\n",
            "<!-- mant:entries role=option case=sensitive -->\n- `--mode MODE`: Choose.\n\n  <!-- mant:domain choices=exhaustive -->\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n  Additional modes:\n\n  1. Ordinary container.\n\n     - Nested container.\n\n       <!-- mant:entries role=invalid case=sensitive -->\n       - `manual`: Manual mode.\n",
        ] {
            let source = body.replace('\n', newline);
            let mut diagnostics = Vec::new();
            let prepared =
                super::super::directives::PreparedMarkdown::new(&source, &mut diagnostics);
            let source_map = super::super::source::MarkdownSource::new(&source);
            let raw = super::super::lower_document_structure(prepared.events, &source_map);
            let mut annotated = raw.root_blocks.clone();
            let mut declarations = prepared.declarations;
            super::super::entries::normalize_entry_lists(
                &mut annotated,
                &mut declarations,
                &mut diagnostics,
            );
            for block in &mut annotated {
                EraseFacts.visit_block_mut(block);
            }
            assert_eq!(annotated, raw.root_blocks, "{source:?}");
            let parsed = parse_markdown(&source, None).unwrap();
            assert!(
                mant_ir::validate_document(&parsed.document).is_empty(),
                "{source:?}: {:?}",
                parsed.document.diagnostics
            );
            let copied: mant_ir::Document =
                serde_json::from_str(&serde_json::to_string(&parsed.document).unwrap()).unwrap();
            assert_eq!(
                mant_ir::SemanticIndex::build(&copied),
                mant_ir::SemanticIndex::build(&parsed.document)
            );
        }
    }
}

#[test]
fn declared_items_fail_independently_and_bind_only_visible_name_occurrences() {
    let parsed = parse_markdown("<!-- mant:entries role=option case=sensitive -->\n3. `-a, --all`: All.\n4. Invalid prose.\n5. `--last`: Last.\n", None).unwrap();
    let Block::List { kind, items, .. } = &parsed.document.blocks[0] else {
        panic!("ordinary list")
    };
    assert_eq!(*kind, mant_ir::ListKind::Ordered { start: Some(3) });
    assert!(items[1].entry.is_none());
    assert!(items[2].entry.is_some());
    assert!(!parsed.document.diagnostics.is_empty());
    let facts = items[0].entry.as_ref().unwrap();
    assert_eq!(facts.names, ["-a", "--all"]);
    assert_eq!(facts.name_bindings.len(), 2);
    assert!(
        facts
            .name_bindings
            .iter()
            .all(|binding| binding.occurrences.len() == 1)
    );
    let query = ResolvedContent {
        address: None,
        label: "mixed".into(),
        document: Some(parsed.document),
        tldr: None,
    };
    assert!(
        !build_outline_with_detail(&query, OutlineDetail::Entries)
            .unwrap()
            .semantics_complete
    );
    assert!(crate::semantic_test_read::semantic_excerpt(&query, &["--last"]).is_ok());
}

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
                assert_eq!(entries.last().unwrap().names, ["auto"]);
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
