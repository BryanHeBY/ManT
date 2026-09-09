//! Markdown import combined with public query and presentation contracts.
use super::*;

#[test]
fn thematic_rule_rendered_gaps_match_root_section_and_nested_list_sources() {
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
            let query = crate::load_markdown_text(&source, None).unwrap();
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
        let query = crate::load_markdown_text(&source, None).unwrap();
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
