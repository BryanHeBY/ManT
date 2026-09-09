//! Existing regressions grouped by entries behavior; expected values remain independent.
use super::*;

#[test]
fn declared_choice_domains_require_values_and_preserve_exhaustiveness() {
    for (policy, exhaustive) in [("exhaustive", true), ("open", false)] {
        for newline in ["\n", "\r\n"] {
            let source = format!("# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n-\n  `--color WHEN`: Color policy.\n\n  <!-- mant:domain choices={policy} -->\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n  - `never`: Disabled.\n").replace('\n', newline);
            let parsed = parse_markdown(&source, None).unwrap();
            assert!(
                parsed.document.diagnostics.is_empty(),
                "{:?}",
                parsed.document.diagnostics
            );
            let index = mant_ir::SemanticIndex::build(&parsed.document);
            assert_eq!(
                index.root()[0].value_domain,
                Some(mant_ir::ValueDomain::Choices { exhaustive })
            );
            assert_eq!(index.root()[0].children.len(), 2);
        }
    }
}

#[test]
fn invalid_choices_do_not_turn_into_an_exhaustive_claim() {
    for (declaration, children) in [
        ("choices=exhaustive", ""),
        (
            "choices=exhaustive",
            "\n  <!-- mant:entries role=command case=sensitive -->\n  - `auto`: A command, not a value.\n",
        ),
        ("choices=yes", ""),
        ("choices=open choices=exhaustive", ""),
        ("choices=exhaustive entries=values.md roles=value", ""),
        ("choices=exhaustive roles=value", ""),
    ] {
        let source = format!(
            "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--color WHEN`: Color policy.\n\n  <!-- mant:domain {declaration} -->\n{children}"
        );
        let parsed = parse_markdown(&source, None).unwrap();
        assert!(
            parsed
                .document
                .diagnostics
                .iter()
                .any(|finding| finding.code.as_deref() == Some("markdown.semantic-value-domain")),
            "{source}"
        );
        let index = mant_ir::SemanticIndex::build(&parsed.document);
        assert_eq!(index.root().len(), 1);
        assert_eq!(index.root()[0].value_domain, None);
    }
}

#[test]
fn ambiguous_choice_claims_leave_only_independent_open_child_inference() {
    let parsed = parse_markdown("# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--color WHEN`: Color policy.\n\n  <!-- mant:domain choices=exhaustive -->\n  <!-- mant:domain entries=other.md roles=value -->\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n", None).unwrap();
    assert!(
        parsed
            .document
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_deref() == Some("markdown.semantic-value-domain"))
    );
    assert_eq!(
        mant_ir::SemanticIndex::build(&parsed.document).root()[0].value_domain,
        Some(mant_ir::ValueDomain::Choices { exhaustive: false })
    );
}

#[test]
fn shared_ir_validation_rejects_a_producer_choices_claim_without_values() {
    let mut document = parse_markdown("# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--color WHEN`: Color policy.\n", None).unwrap().document;
    let Block::List { items, .. } = &mut document.blocks[0] else {
        panic!("definition")
    };
    items[0].entry.as_mut().unwrap().value_domain =
        Some(mant_ir::ValueDomain::Choices { exhaustive: true });
    let findings = mant_ir::validate_document(&document);
    assert!(
        findings
            .iter()
            .any(|finding| finding.code.as_deref() == Some("ir.invalid-entry-choices"))
    );
    assert!(!mant_ir::semantics_complete(&findings));
}

#[test]
fn declared_forms_are_separate_from_alias_groups() {
    let parsed = parse_markdown("# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o FILE`, `--output FILE` | `--output=FILE`: Write output.\n", None).unwrap();
    assert!(
        parsed.document.diagnostics.is_empty(),
        "{:?}",
        parsed.document.diagnostics
    );
    let index = mant_ir::SemanticIndex::build(&parsed.document);
    let entry = &index.root()[0];
    assert_eq!(entry.names, ["-o", "--output"]);
    assert_eq!(entry.forms, ["-o FILE, --output FILE", "--output=FILE"]);
    for term in [
        "`-o` | : Empty.",
        "`-o` || `--output`: Empty.",
        "`-o` | | `--output`: Empty.",
    ] {
        let source =
            format!("# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- {term}\n");
        let parsed = parse_markdown(&source, None).unwrap();
        assert!(!parsed.document.diagnostics.is_empty(), "{term}");
        assert!(
            mant_ir::SemanticIndex::build(&parsed.document)
                .root()
                .is_empty(),
            "{term}"
        );
    }
}

#[test]
fn declared_options_reject_arbitrary_bang_prefixed_terms() {
    let parsed = parse_markdown(
        "# tool\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `!reloadEnvironment`: Invalid negation.\n",
        Some("invalid-negated-option.md".to_owned()),
    )
    .expect("invalid entry remains a readable document");
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.unsupported-option-prefix")
    }));
    assert!(matches!(
        &parsed.document.sections[0].blocks[0],
        Block::List {
            kind: ListKind::Bullet,
            ..
        }
    ));
}

#[test]
fn variable_declarations_reject_environment_provider_names_per_item() {
    let parsed = parse_markdown(
        "# Shell\n\n<!-- mant:entries role=variable case=insensitive -->\n- `$good`: Ordinary variable.\n- `$env:PATH`: Environment provider variable.\n",
        None,
    )
    .expect("invalid variable remains visible");
    assert!(matches!(parsed.document.blocks[0], Block::List { .. }));
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.invalid-entry-name")
            && diagnostic.message.contains("$env:PATH")
            && diagnostic.source.is_some_and(|source| source.line == 5)
    }));
}

#[test]
fn malformed_declared_entry_lists_remain_visible_and_report_the_list_location() {
    let parsed = parse_markdown(
        "# tool\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `--good`: Valid.\n- ordinary prose\n",
        None,
    )
    .expect("rejected declarations are recoverable");

    assert!(matches!(
        parsed.document.sections[0].blocks[0],
        Block::List { .. }
    ));
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.missing-leading-code")
            && diagnostic.source.is_some_and(|source| source.line == 7)
    }));
}

#[test]
fn declared_entry_grammar_accepts_blank_lines_delimiters_and_colon_conventions() {
    let parsed = parse_markdown(
        "<!-- mant:entries role=option case=insensitive -->\n\n- `/server:NAME`: Uppercase placeholder.\n- `/target:<HOST>` — Angle-bracket placeholder.\n- `/server:name` – Lowercase fixed value.\n- `/mode:auto`: Alphabetic fixed value.\n\n# Details\n",
        None,
    )
    .expect("declared root entries");
    assert!(parsed.document.diagnostics.is_empty());
    let Block::List { items, .. } = &parsed.document.blocks[0] else {
        panic!("the next non-empty root list should become semantic entries");
    };
    assert_eq!(
        items
            .iter()
            .map(|item| {
                item.entry
                    .as_ref()
                    .expect("semantic identity")
                    .names
                    .clone()
            })
            .collect::<Vec<_>>(),
        [
            vec!["/server".to_owned()],
            vec!["/target".to_owned()],
            vec!["/server:name".to_owned()],
            vec!["/mode:auto".to_owned()],
        ]
    );
}

#[test]
fn semantic_directives_are_independent_of_markdown_line_endings() {
    let source = "# Tool\n\n<!-- mant:entries role=environment-variable case=sensitive -->\n- `MANT_MANPATH`: Select roots.\n\n  <!-- mant:domain entries=manual/5/manpath roles=environment-variable -->\n";
    for newline in ["\n", "\r\n"] {
        let source = source.replace('\n', newline);
        let parsed = parse_markdown(&source, None).expect("semantic directives parse");
        assert!(
            parsed.document.diagnostics.is_empty(),
            "{newline:?}: {:?}",
            parsed.document.diagnostics
        );
        let semantic_index = mant_ir::SemanticIndex::build(&parsed.document);
        let [entry] = semantic_index.root() else {
            panic!("{newline:?}: one semantic entry expected");
        };
        assert_eq!(entry.names, ["MANT_MANPATH"]);
        assert!(matches!(
            entry.value_domain,
            Some(mant_ir::ValueDomain::EntrySet {
                reference: mant_ir::DocumentReference::Manual {
                    ref name,
                    manual_section: Some(ref section),
                },
                ..
            }) if name == "manpath" && section == "5"
        ));
    }
}

#[test]
fn indented_code_does_not_activate_semantic_entry_directives() {
    let parsed = parse_markdown(
        "# Tool\n\n    <!-- mant:entries role=command case=sensitive -->\n    - `not-a-command`: code\n",
        None,
    )
    .expect("indented code remains ordinary Markdown");
    assert!(parsed.document.diagnostics.is_empty());
    assert!(matches!(
        parsed.document.blocks.as_slice(),
        [Block::Preformatted { .. }]
    ));
}

#[test]
fn declared_entry_description_requires_a_leading_paragraph_delimiter() {
    let parsed = parse_markdown(
        "# Tool\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`\n\n  Query data in a following paragraph.\n",
        None,
    )
    .expect("invalid declared entry remains recoverable");
    assert!(matches!(parsed.document.blocks[0], Block::List { .. }));
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.missing-description")
            && diagnostic.message.contains("query")
    }));
}

#[test]
fn entry_domains_attach_to_multiline_nested_items_with_crlf() {
    let source = "# Query\n\n<!-- mant:entries role=command case=insensitive -->\n-\n  `query`: Dispatch a query family.\n\n  <!-- mant:entries role=option case=insensitive -->\n  -\n    `--user`: Select users.\n\n    <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->\n";
    for newline in ["\n", "\r\n"] {
        let parsed = parse_markdown(&source.replace('\n', newline), None)
            .expect("nested multiline entry domains parse");
        assert!(
            parsed.document.diagnostics.is_empty(),
            "{newline:?}: {:?}",
            parsed.document.diagnostics
        );
        let semantic_index = mant_ir::SemanticIndex::build(&parsed.document);
        let [parent] = semantic_index.root() else {
            panic!("one parent entry expected");
        };
        let [child] = parent.children.as_slice() else {
            panic!("one nested entry expected");
        };
        assert!(matches!(
            child.value_domain,
            Some(mant_ir::ValueDomain::EntrySet {
                reference: mant_ir::DocumentReference::Manual {
                    ref name,
                    manual_section: Some(ref section),
                },
                source: Some(_),
                ..
            }) if name == "ssh_config" && section == "5"
        ));
    }
}

#[test]
fn entry_domains_on_nested_items_remain_independent() {
    let parsed = parse_markdown(
        "# Query\n\n<!-- mant:entries role=command case=sensitive -->\n- `query`: Dispatch queries.\n\n  <!-- mant:domain entries=query-values.md roles=value -->\n\n  <!-- mant:entries role=option case=sensitive -->\n  - `--user`: Select users.\n\n    <!-- mant:domain entries=user-values.md roles=value -->\n",
        None,
    )
    .expect("nested independent domains parse");
    assert!(parsed.document.diagnostics.is_empty());

    let semantic_index = mant_ir::SemanticIndex::build(&parsed.document);
    let [parent] = semantic_index.root() else {
        panic!("one parent entry expected");
    };
    let [child] = parent.children.as_slice() else {
        panic!("one child entry expected");
    };
    assert!(matches!(
        parent.value_domain,
        Some(mant_ir::ValueDomain::EntrySet {
            reference: mant_ir::DocumentReference::Document { ref name, .. },
            ..
        }) if name == "query-values"
    ));
    assert!(matches!(
        child.value_domain,
        Some(mant_ir::ValueDomain::EntrySet {
            reference: mant_ir::DocumentReference::Document { ref name, .. },
            ..
        }) if name == "user-values"
    ));
}

#[test]
fn similarly_prefixed_html_is_not_a_semantic_domain_directive() {
    let parsed = parse_markdown(
        "# Tool\n\n<!-- mant:entries role=command case=sensitive -->\n- `run`: Execute.\n\n  <!-- mant:domainentries=other.md roles=command -->\n",
        None,
    )
    .expect("ordinary HTML remains recoverable");
    assert!(parsed.document.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_deref() != Some("markdown.semantic-value-domain")
    }));
    let semantic_index = mant_ir::SemanticIndex::build(&parsed.document);
    let [entry] = semantic_index.root() else {
        panic!("one semantic entry expected");
    };
    assert!(entry.value_domain.is_none());
}
