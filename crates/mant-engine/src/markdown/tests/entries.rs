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
    let Block::DefinitionList { items, .. } = &mut document.blocks[0] else {
        panic!("definition")
    };
    items[0].identity.as_mut().unwrap().value_domain =
        Some(mant_ir::ValueDomain::Choices { exhaustive: true });
    let findings = mant_ir::validate_document(&document);
    assert!(
        findings
            .iter()
            .any(|finding| finding.code.as_deref() == Some("ir.invalid-entry-choices"))
    );
    assert!(mant_ir::is_semantic_completeness_diagnostic(
        "ir.invalid-entry-choices"
    ));
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
    assert_eq!(entry.aliases, ["-o", "--output"]);
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
fn turns_explicit_option_lists_into_addressable_definitions() {
    let document = parse_document(
        "\
# Tool

## Options

- `-h`, `--help`: Show help.
- `--color=WHEN` — Set the colour mode.
",
        None,
    );

    let options = &document.sections[0];
    let Block::DefinitionList { items, .. } = &options.blocks[0] else {
        panic!("explicit option list should become a semantic definition list");
    };
    assert_eq!(
        items[0].identity.as_ref().expect("option identity").names,
        ["-h", "--help"]
    );
    assert_eq!(
        items[1].identity.as_ref().expect("option identity").names,
        ["--color"]
    );
    assert!(matches!(
        &items[0].terms[0][0],
        Inline::Anchor { id, .. } if id == "option-h"
    ));

    let outline = build_outline_with_detail(
        &ResolvedContent {
            address: None,
            label: "tool.md".to_owned(),
            document: Some(document),
            tldr: None,
        },
        OutlineDetail::Entries,
    )
    .expect("Markdown document has an outline");
    let OutlineNode::DocumentSection { children, .. } = &outline.nodes[0] else {
        panic!("options should be a top-level document section");
    };
    assert!(matches!(
        &children[0],
        OutlineNode::DocumentEntry { aliases, .. } if aliases == &["-h", "--help"]
    ));
}

#[test]
fn declared_entries_cover_windows_options_commands_and_environment_variables() {
    let parsed = parse_markdown(
        "# tool\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/query`: Query tasks.\n- `/?`: Display help.\n- `/S COMPUTER`: Select a remote computer.\n- `/server:NAME`: Select a server.\n- `/reg:32`, `/reg:64`: Select registry views.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`: Read values.\n- `winget install`: Install a package.\n\n### query\n\nBehavioral details.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=insensitive -->\n- `PATH`, `$env:PATH`: Control executable discovery.\n- `$LASTEXITCODE`: Hold the last native exit code.\n- `%ProgramFiles(x86)%`: Locate 32-bit programs.\n- `${Env:ProgramData}`: Locate shared application data.\n- `RUST_LOG=debug`: Select a log filter.\n",
        Some("tool.md".to_owned()),
    )
    .expect("declared semantic entries");
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.shadowed-selector")
            && diagnostic.message.contains("semantic selector 'query'")
    }));

    let Block::DefinitionList {
        items: option_items,
        ..
    } = &parsed.document.sections[0].blocks[0]
    else {
        panic!("declared options should become definitions");
    };
    let identities = option_items
        .iter()
        .map(|item| item.identity.as_ref().expect("semantic identity"))
        .collect::<Vec<_>>();
    assert_eq!(identities[0].names, ["/query"]);
    assert_eq!(identities[1].id, "option-help");
    assert_eq!(identities[2].names, ["/S"]);
    assert_eq!(identities[3].names, ["/server"]);
    assert_eq!(identities[4].names, ["/reg:32", "/reg:64"]);
    assert!(identities.iter().all(|identity| {
        identity.role == DefinitionRole::Option && identity.case == DefinitionCase::Insensitive
    }));

    let query = ResolvedContent {
        address: None,
        label: "tool".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    let explanation = select_explanation(&query, "/QUERY").expect("case-insensitive option");
    assert!(matches!(
        explanation.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| identity.names == ["/query"])
    ));
    assert!(matches!(
        select_explanation(&query, "query"),
        Err(ProjectionError::ExplanationRequiresEntry { .. })
    ));
    let command = select_explanation(&query, "QUERY")
        .expect("a differently cased alias does not equal the case-sensitive section ID");
    assert!(matches!(
        command.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| identity.role == DefinitionRole::Command)
    ));
    for selector in ["3", "environment"] {
        assert!(matches!(
            select_explanation(&query, selector),
            Err(ProjectionError::ExplanationRequiresEntry { .. })
        ));
    }
    let environment = select_explanation(&query, "path").expect("environment alias");
    assert!(matches!(
        environment.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| identity.role == DefinitionRole::EnvironmentVariable)
    ));
    for selector in [
        "ProgramFiles(x86)",
        "%ProgramFiles(x86)%",
        "ProgramData",
        "${Env:ProgramData}",
        "RUST_LOG",
    ] {
        assert!(
            select_explanation(&query, selector).is_ok(),
            "environment selector {selector}"
        );
    }
}

#[test]
fn declared_entries_expose_every_protocol_semantic_role() {
    let parsed = parse_markdown(
        "# tool\n\n## Markers\n\n<!-- mant:entries role=marker case=sensitive -->\n- `--`: End option parsing.\n\n## Operands\n\n<!-- mant:entries role=operand case=sensitive -->\n- `FILE`: Select an input.\n\n## Configuration\n\n<!-- mant:entries role=configuration-key case=insensitive -->\n- `AuthorizedKeysFile`: Select key paths.\n\n## Values\n\n<!-- mant:entries role=value case=insensitive -->\n- `always`: Select a policy.\n\n## Terms\n\n<!-- mant:entries role=term case=sensitive -->\n- `exit status`: Describe a result.\n",
        Some("roles.md".to_owned()),
    )
    .expect("all declared semantic roles");
    assert!(parsed.document.diagnostics.is_empty());

    let expected = [
        (DefinitionRole::Marker, "--"),
        (DefinitionRole::Operand, "FILE"),
        (DefinitionRole::ConfigurationKey, "AuthorizedKeysFile"),
        (DefinitionRole::Value, "always"),
        (DefinitionRole::Term, "exit status"),
    ];
    for (section, (role, name)) in parsed.document.sections.iter().zip(expected) {
        let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
            panic!("declared {role:?} list should become definitions");
        };
        let identity = items[0].identity.as_ref().expect("semantic identity");
        assert_eq!(identity.role, role);
        assert_eq!(identity.names, [name]);
    }

    let content = ResolvedContent {
        address: None,
        label: "roles".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    for selector in ["--", "FILE", "authorizedkeysfile", "ALWAYS", "exit status"] {
        assert!(
            select_explanation(&content, selector).is_ok(),
            "semantic selector {selector}"
        );
    }
}

#[test]
fn declared_non_option_code_spans_are_atomic_names() {
    let parsed = parse_markdown(
        "# tool\n\n## Terms\n\n<!-- mant:entries role=term case=sensitive -->\n- `Send, Env`: Preserve punctuation.\n- `A | B`: Preserve a grammar expression.\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- `alpha|beta`: Preserve a literal command.\n- `cd`, `chdir`: Expose explicit aliases.\n",
        Some("atomic.md".to_owned()),
    )
    .expect("atomic semantic entry names");
    assert!(parsed.document.diagnostics.is_empty());

    let identities = parsed
        .document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items),
            _ => None,
        })
        .flatten()
        .map(|item| item.identity.as_ref().expect("semantic identity"))
        .collect::<Vec<_>>();
    assert_eq!(identities[0].names, ["Send, Env"]);
    assert_eq!(identities[1].names, ["A | B"]);
    assert_eq!(identities[2].names, ["alpha|beta"]);
    assert_eq!(identities[3].names, ["cd", "chdir"]);

    let content = ResolvedContent {
        address: None,
        label: "atomic.md".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    for selector in ["Send, Env", "A | B", "alpha|beta", "cd", "chdir"] {
        assert!(
            select_explanation(&content, selector).is_ok(),
            "semantic selector {selector}"
        );
    }
    for truncated in ["Send", "Env", "A", "B", "alpha", "beta"] {
        assert!(
            select_explanation(&content, truncated).is_err(),
            "truncated selector {truncated} must not resolve"
        );
    }
}

#[test]
fn declared_dotted_dash_options_preserve_their_exact_names() {
    let parsed = parse_markdown(
        "# tool\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `-ca.cert`: Retrieve a CA certificate.\n- `-ca.chain`: Retrieve a CA chain.\n- `--foo.bar`: Use a dotted long option.\n- `--config.file=FILE`: Read a configuration file.\n- `--output.name <PATH>`: Write to a path.\n",
        Some("dot-option.md".to_owned()),
    )
    .expect("dotted semantic options");
    assert!(parsed.document.diagnostics.is_empty());

    let Block::DefinitionList { items, .. } = &parsed.document.sections[0].blocks[0] else {
        panic!("declared options should become definitions");
    };
    assert_eq!(
        items
            .iter()
            .map(|item| {
                item.identity
                    .as_ref()
                    .expect("semantic identity")
                    .names
                    .clone()
            })
            .collect::<Vec<_>>(),
        [
            vec!["-ca.cert".to_owned()],
            vec!["-ca.chain".to_owned()],
            vec!["--foo.bar".to_owned()],
            vec!["--config.file".to_owned()],
            vec!["--output.name".to_owned()],
        ]
    );

    let query = ResolvedContent {
        address: None,
        label: "dot-option.md".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    for selector in [
        "-ca.cert",
        "-ca.chain",
        "--foo.bar",
        "--config.file",
        "--output.name",
    ] {
        let explanation = select_explanation(&query, selector).expect("exact dotted selector");
        assert!(matches!(
            explanation.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { entry, .. }]
                if entry.identity.as_ref().is_some_and(|identity| identity.names == [selector])
        ));
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
fn declared_variables_keep_shell_and_powershell_automatic_names() {
    let parsed = parse_markdown(
        "# Shell\n\n## Variables\n\n<!-- mant:entries role=variable case=insensitive -->\n- `$?`: Last success state.\n- `$$`: Current process identifier.\n- `$^`: First pipeline input.\n- `$_`: Current pipeline item.\n- `$null`: Null value.\n- `$LASTEXITCODE`: Native exit status.\n- `$PSVersionTable`: PowerShell version data.\n- `$PROFILE`: Profile paths.\n- `$PATH`: Ordinary shell variable.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=insensitive -->\n- `$env:PATH`: Process executable path.\n",
        Some("shell.md".to_owned()),
    )
    .expect("variable semantic entries");
    assert!(parsed.document.diagnostics.is_empty());

    let query = ResolvedContent {
        address: None,
        label: "shell".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    let outline =
        build_outline_with_detail(&query, OutlineDetail::Entries).expect("variable entry outline");
    let OutlineNode::DocumentSection { children, .. } = &outline.nodes[0] else {
        panic!("variables section");
    };
    assert_eq!(children.len(), 9);
    assert!(children.iter().all(|entry| matches!(
        entry,
        OutlineNode::DocumentEntry {
            entry_kind: EntryKind::Variable,
            ..
        }
    )));
    assert!(matches!(
        &children[0],
        OutlineNode::DocumentEntry { id, aliases, .. }
            if id == "variable-question-mark" && aliases == &["$?"]
    ));
    for selector in ["$?", "$$", "$^", "$_", "$lastexitcode", "$PSVersionTable"] {
        let explanation = select_explanation(&query, selector).expect("variable selector");
        assert!(matches!(
            explanation.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { entry, .. }]
                if entry.identity.as_ref().is_some_and(|identity| identity.role == DefinitionRole::Variable)
        ));
    }
    assert!(matches!(
        select_explanation(&query, "$env:PATH")
            .expect("environment variable selector")
            .selections
            .as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| identity.role == DefinitionRole::EnvironmentVariable)
    ));
    assert!(matches!(
        select_explanation(&query, "$PATH")
            .expect("ordinary variable selector")
            .selections
            .as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| identity.role == DefinitionRole::Variable)
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
fn exact_aliases_win_before_normalized_option_shorthands() {
    let parsed = parse_markdown(
        "# Tool\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `?`: Display positional help.\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/?`, `-?`: Display option help.\n",
        Some("help-spellings.md".to_owned()),
    )
    .expect("help spelling fixture");
    assert!(parsed.document.diagnostics.is_empty());
    let query = ResolvedContent {
        address: None,
        label: "help-spellings.md".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };

    let command = select_explanation(&query, "?").expect("exact command spelling");
    assert!(matches!(
        command.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| {
                identity.role == DefinitionRole::Command && identity.names == ["?"]
            })
    ));
    let command_node = select_excerpt(&query, &["?".to_owned()]).expect("exact command node");
    assert!(matches!(
        command_node.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| {
                identity.role == DefinitionRole::Command && identity.names == ["?"]
            })
    ));
    for selector in ["/?", "-?"] {
        let option = select_explanation(&query, selector).expect("exact option spelling");
        assert!(matches!(
            option.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { entry, .. }]
                if entry.identity.as_ref().is_some_and(|identity| {
                    identity.role == DefinitionRole::Option
                        && identity.names == ["/?", "-?"]
                })
        ));
    }
}

#[test]
fn the_same_alias_in_different_roles_is_ambiguous() {
    let parsed = parse_markdown(
        "# Tool\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- `PATH`: Run a command.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=sensitive -->\n- `PATH`: Configure discovery.\n",
        None,
    )
    .expect("cross-role alias fixture");
    let query = ResolvedContent {
        address: None,
        label: "tool".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };

    let error = select_explanation(&query, "PATH").expect_err("cross-role alias is ambiguous");
    let ProjectionError::AmbiguousSelector { candidates, .. } = error else {
        panic!("expected structured ambiguity");
    };
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect::<Vec<_>>(),
        ["command-path", "environment-path"]
    );
}

#[test]
fn exact_entry_id_takes_precedence_over_another_entry_alias() {
    let parsed = parse_markdown(
        "# Tool\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- `query`: Query data.\n- `command-query`: A command whose alias resembles an ID.\n",
        None,
    )
    .expect("entry ID precedence fixture");
    let query = ResolvedContent {
        address: None,
        label: "tool".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };

    let explanation = select_explanation(&query, "command-query").expect("exact entry ID");
    assert!(matches!(
        explanation.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| {
                identity.id == "command-query" && identity.names == ["query"]
            })
    ));
}

#[test]
fn declared_case_policy_preserves_distinct_sensitive_aliases() {
    let parsed = parse_markdown(
        "# Tool\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `-p`: Lowercase mode.\n- `-P`: Uppercase mode.\n",
        None,
    )
    .expect("case-sensitive entries");
    let query = ResolvedContent {
        address: None,
        label: "tool".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };

    for (selector, expected) in [("p", "-p"), ("P", "-P")] {
        let explanation = select_explanation(&query, selector).expect("case-sensitive alias");
        assert!(matches!(
            explanation.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { entry, .. }]
                if entry.identity.as_ref().is_some_and(|identity| identity.names == [expected])
        ));
    }
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
    let Block::DefinitionList { items, .. } = &parsed.document.blocks[0] else {
        panic!("the next non-empty root list should become semantic entries");
    };
    assert_eq!(
        items
            .iter()
            .map(|item| {
                item.identity
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
fn declared_option_entries_cover_windows_native_token_families() {
    fn collect_names(nodes: &[OutlineNode], output: &mut Vec<String>) {
        for node in nodes {
            match node {
                OutlineNode::DocumentEntry {
                    aliases: entry_names,
                    ..
                } => output.extend(entry_names.iter().cloned()),
                OutlineNode::DocumentRoot { children, .. }
                | OutlineNode::DocumentSection { children, .. } => collect_names(children, output),
                OutlineNode::Tldr { .. } => {}
            }
        }
    }

    let parsed = parse_markdown(
        "# Native options\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `type= TYPE`: Select a type.\n- `start= MODE`: Select a start mode.\n- `board=N`: Select a board.\n- `PORTX=PORTY`: Map ports.\n- `//B`: Select batch mode.\n- `//E:ENGINE`: Select an engine.\n- `//?`: Display host help.\n- `+r`: Set an attribute.\n- `+shared`: Share a printer.\n- `+N`: Select a line.\n- `/+N`: Select an offset.\n- `/driver.exclude`: Exclude drivers.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `start`: Start processing.\n",
        Some("native.md".to_owned()),
    )
    .expect("Windows-native semantic entries");
    assert!(parsed.document.diagnostics.is_empty());

    let query = ResolvedContent {
        address: None,
        label: "native.md".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    let outline = build_outline_with_detail(&query, OutlineDetail::Entries)
        .expect("Windows-native entry outline");
    let mut names = Vec::new();
    collect_names(&outline.nodes, &mut names);
    assert_eq!(
        names,
        [
            "type=",
            "start=",
            "board=",
            "PORTX=",
            "//B",
            "//E",
            "//?",
            "+r",
            "+shared",
            "+N",
            "/+N",
            "/driver.exclude",
            "start",
        ]
    );

    for selector in ["START=", "//b", "//e", "/DRIVER.EXCLUDE", "+R", "/+n"] {
        select_explanation(&query, selector).expect("case-insensitive Windows entry selector");
    }
    let option = select_explanation(&query, "start=").expect("equals-bearing option selector");
    let command = select_explanation(&query, "start").expect("command selector");
    assert!(matches!(
        option.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| {
                identity.id == "option-start" && identity.role == DefinitionRole::Option
            })
    ));
    assert!(matches!(
        command.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { entry, .. }]
            if entry.identity.as_ref().is_some_and(|identity| {
                identity.id == "command-start" && identity.role == DefinitionRole::Command
            })
    ));
}

#[test]
fn rejected_declared_entries_report_each_term_reason_and_item_location() {
    let parsed = parse_markdown(
        "# tool\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `--good`: Valid.\n- `/driver..exclude`: Empty dotted segment.\n- `type= lowercase`: Lowercase placeholder.\n- `--bad@name`: Unsupported punctuation.\n",
        None,
    )
    .expect("rejected declaration diagnostics");

    assert!(matches!(
        parsed.document.sections[0].blocks[0],
        Block::List { .. }
    ));
    assert_eq!(parsed.document.diagnostics.len(), 3);
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.invalid-option-name")
            && diagnostic.message.contains("/driver..exclude")
            && diagnostic.source.is_some_and(|source| source.line == 7)
    }));
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.invalid-placeholder")
            && diagnostic.message.contains("type= lowercase")
            && diagnostic.source.is_some_and(|source| source.line == 8)
    }));
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.invalid-option-name")
            && diagnostic.message.contains("--bad@name")
            && diagnostic.source.is_some_and(|source| source.line == 9)
    }));

    let outline = build_outline_with_detail(
        &ResolvedContent {
            address: None,
            label: "tool.md".to_owned(),
            document: Some(parsed.document),
            tldr: None,
        },
        OutlineDetail::Entries,
    )
    .expect("incomplete semantic outline");
    assert!(!outline.semantics_complete);
    assert_eq!(outline.diagnostics.len(), 3);
}

#[test]
fn declared_entry_directive_does_not_skip_an_intervening_construct() {
    let parsed = parse_markdown(
        "# Tool\n\n<!-- mant:entries role=option case=insensitive -->\n## Options\n\n- `/query`: Query data.\n",
        None,
    )
    .expect("invalid directive placement remains recoverable");
    assert!(matches!(
        parsed.document.sections[0].blocks[0],
        Block::List { .. }
    ));
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry-list")
            && diagnostic.message.contains("immediately precede")
    }));

    let outline = build_outline_with_detail(
        &ResolvedContent {
            address: None,
            label: "tool.md".to_owned(),
            document: Some(parsed.document),
            tldr: None,
        },
        OutlineDetail::Entries,
    )
    .expect("recoverable incomplete semantic outline");
    assert!(!outline.semantics_complete);
}

#[test]
fn declared_entries_preserve_roles_at_arbitrary_list_depth() {
    let parsed = parse_markdown(
        "# Tool\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`: Dispatch queries.\n\n  <!-- mant:entries role=command case=insensitive -->\n  - `query user`: Inspect users.\n\n    <!-- mant:entries role=option case=insensitive -->\n    - `/server:NAME`: Select a server.\n\n      <!-- mant:entries role=value case=insensitive -->\n      - `local`: Use the local server.\n",
        None,
    )
    .expect("deep semantic entry declarations");
    assert!(parsed.document.diagnostics.is_empty());

    let outline = build_outline_with_detail(
        &ResolvedContent {
            address: None,
            label: "tool.md".to_owned(),
            document: Some(parsed.document),
            tldr: None,
        },
        OutlineDetail::Entries,
    )
    .expect("deep semantic outline");
    let OutlineNode::DocumentSection { children, .. } = &outline.nodes[0] else {
        panic!("commands should be a section");
    };
    let OutlineNode::DocumentEntry {
        entry_kind: EntryKind::Command,
        case: DefinitionCase::Insensitive,
        children,
        ..
    } = &children[0]
    else {
        panic!("query should be a command");
    };
    let OutlineNode::DocumentEntry {
        entry_kind: EntryKind::Command,
        children,
        ..
    } = &children[0]
    else {
        panic!("query user should be a nested command");
    };
    let OutlineNode::DocumentEntry {
        entry_kind: EntryKind::Parameter { .. },
        children,
        ..
    } = &children[0]
    else {
        panic!("/server should be a nested parameter");
    };
    assert!(matches!(
        children.as_slice(),
        [OutlineNode::DocumentEntry {
            entry_kind: EntryKind::Value,
            aliases,
            ..
        }] if aliases == &["local"]
    ));
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
        assert_eq!(entry.aliases, ["MANT_MANPATH"]);
        assert!(matches!(
            entry.value_domain,
            Some(mant_ir::ValueDomain::EntrySet {
                reference: mant_ir::SemanticDocumentReference::Manual {
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
fn entry_domain_directives_resolve_cross_document_value_spaces() {
    let parsed = parse_markdown(
        "# SSH\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a configuration key.\n\n  <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->\n",
        None,
    )
    .expect("entry domain parses");
    assert!(parsed.document.diagnostics.is_empty());
    let index = mant_ir::SemanticIndex::build(&parsed.document);
    let [entry] = index.root() else {
        panic!("one semantic option expected");
    };
    assert!(matches!(
        entry.value_domain,
        Some(mant_ir::ValueDomain::EntrySet {
            reference: mant_ir::SemanticDocumentReference::Manual {
                ref name,
                manual_section: Some(ref section),
            },
            ref entry_kinds,
            ..
        }) if name == "ssh_config"
            && section == "5"
            && entry_kinds == &[EntryKind::ConfigurationKey]
    ));

    let outline = build_outline_projection(
        &ResolvedContent {
            label: "ssh(1)".to_owned(),
            address: Some(DocumentAddress::Manual {
                name: "ssh".to_owned(),
                manual_section: "1".to_owned(),
            }),
            document: Some(parsed.document),
            tldr: None,
        },
        EntryProjection::All,
        None,
    )
    .expect("entry domain outline");
    let OutlineNode::DocumentRoot { children, .. } = &outline.nodes[0] else {
        panic!("document title leaves entries in root content");
    };
    assert!(matches!(
        children.as_slice(),
        [OutlineNode::DocumentEntry {
            value_domain: Some(value_domain),
            ..
        }] if matches!(value_domain.as_ref(), mant_protocol::EntryValueDomain::EntrySet {
                reference: mant_ir::SemanticDocumentReference::Manual { name, manual_section: Some(section) },
                address: Some(DocumentAddress::Manual { name: address_name, manual_section: address_section }),
                entry_kinds,
            } if name == "ssh_config"
            && section == "5"
            && address_name == "ssh_config"
            && address_section == "5"
            && entry_kinds == &[EntryKind::ConfigurationKey])
    ));
    assert!(
        render_outline_text(&outline).contains("values: configuration key in manual/5/ssh_config")
    );
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
                reference: mant_ir::SemanticDocumentReference::Manual {
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
fn duplicate_entry_domains_are_ambiguous_instead_of_last_wins() {
    for second_reference in ["first.md", "second.md"] {
        let parsed = parse_markdown(
            &format!(
                "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a key.\n\n  <!-- mant:domain entries=first.md roles=configuration-key -->\n  <!-- mant:domain entries={second_reference} roles=configuration-key -->\n"
            ),
            None,
        )
        .expect("duplicate domains remain recoverable");

        assert_eq!(
            parsed
                .document
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.code.as_deref() == Some("markdown.semantic-value-domain")
                        && diagnostic.message.contains("more than one")
                })
                .count(),
            1
        );
        let semantic_index = mant_ir::SemanticIndex::build(&parsed.document);
        let [entry] = semantic_index.root() else {
            panic!("one semantic entry expected");
        };
        assert!(entry.value_domain.is_none());

        let outline = build_outline_with_detail(
            &ResolvedContent {
                label: "tool".to_owned(),
                address: None,
                document: Some(parsed.document),
                tldr: None,
            },
            OutlineDetail::Entries,
        )
        .expect("incomplete outline");
        assert!(!outline.semantics_complete);
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
            reference: mant_ir::SemanticDocumentReference::Document { ref name, .. },
            ..
        }) if name == "query-values"
    ));
    assert!(matches!(
        child.value_domain,
        Some(mant_ir::ValueDomain::EntrySet {
            reference: mant_ir::SemanticDocumentReference::Document { ref name, .. },
            ..
        }) if name == "user-values"
    ));
}

#[test]
fn malformed_entry_domains_remain_visible_and_incomplete() {
    for directive in [
        "<!-- mant:domain entries=manual/5/ssh_config roles=configuration-key,configuration-key -->",
        "<!-- mant:domain entries=manual/qgroup/ssh_config roles=configuration-key -->",
    ] {
        let parsed = parse_markdown(
            &format!(
                "# SSH\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a key.\n\n  {directive}\n"
            ),
            None,
        )
        .expect("invalid domain remains recoverable");
        assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("markdown.semantic-value-domain")
        }));
        let outline = build_outline_with_detail(
            &ResolvedContent {
                label: "ssh".to_owned(),
                address: None,
                document: Some(parsed.document),
                tldr: None,
            },
            OutlineDetail::Entries,
        )
        .expect("incomplete outline");
        assert!(!outline.semantics_complete);
    }
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

#[test]
fn misplaced_entry_domain_is_visible_as_incomplete_semantics() {
    let parsed = parse_markdown(
        "# Tool\n\n<!-- mant:domain entries=other.md roles=command -->\n\nProse.\n",
        None,
    )
    .expect("misplaced domain remains recoverable");
    assert!(
        parsed.document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("markdown.semantic-value-domain")
        }),
        "diagnostics: {:?}",
        parsed.document.diagnostics
    );
    let outline = build_outline_with_detail(
        &ResolvedContent {
            label: "tool".to_owned(),
            address: None,
            document: Some(parsed.document),
            tldr: None,
        },
        OutlineDetail::Entries,
    )
    .expect("incomplete outline remains available");
    assert!(!outline.semantics_complete);
}

#[test]
fn linked_code_terms_define_entry_document_destinations() {
    let parsed = parse_markdown(
        "# Tools\n\n<!-- mant:entries role=command case=insensitive -->\n- [`winget`](winget.exe.md#install), [`w`](winget.exe.md#install): Windows package manager.\n- `plain`: See [details](plain.md).\n",
        None,
    )
    .expect("linked entry terms parse");
    assert!(parsed.document.diagnostics.is_empty());

    let index = mant_ir::SemanticIndex::build(&parsed.document);
    let entries = index.root();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].aliases, ["winget", "w"]);
    assert!(matches!(
        entries[0].document_targets.as_slice(),
        [mant_ir::SemanticDocumentTarget {
            label: first_label,
            reference: mant_ir::SemanticDocumentReference::Document {
                name: first_name,
                fragment: Some(first_fragment),
            },
        }, mant_ir::SemanticDocumentTarget {
            label: second_label,
            reference: mant_ir::SemanticDocumentReference::Document {
                name: second_name,
                fragment: Some(second_fragment),
            },
        }] if first_label == "winget"
            && second_label == "w"
            && first_name == "winget.exe"
            && second_name == "winget.exe"
            && first_fragment == "install"
            && second_fragment == "install"
    ));
    assert!(entries[1].document_targets.is_empty());

    let outline = build_outline_projection(
        &ResolvedContent {
            label: "tools".to_owned(),
            address: Some(DocumentAddress::Markdown {
                path: "indexes/tools".to_owned(),
                origin: MarkdownOrigin::Documents,
            }),
            document: Some(parsed.document),
            tldr: None,
        },
        EntryProjection::All,
        None,
    )
    .expect("linked entry outline");
    let OutlineNode::DocumentRoot { children, .. } = &outline.nodes[0] else {
        panic!("document title leaves entries in root content");
    };
    assert!(matches!(
        children.as_slice(),
        [OutlineNode::DocumentEntry { document_targets, .. }, OutlineNode::DocumentEntry { .. }]
            if matches!(
                document_targets.as_slice(),
                [mant_protocol::EntryDocumentTarget {
                    label: first_label,
                    reference: mant_ir::SemanticDocumentReference::Document { name: first_name, fragment: Some(first_fragment) },
                    address: Some(DocumentAddress::Markdown { path: first_path, origin: MarkdownOrigin::Documents }),
                }, mant_protocol::EntryDocumentTarget {
                    label: second_label,
                    reference: mant_ir::SemanticDocumentReference::Document { name: second_name, fragment: Some(second_fragment) },
                    address: Some(DocumentAddress::Markdown { path, origin: MarkdownOrigin::Documents }),
                }] if first_label == "winget"
                    && second_label == "w"
                    && first_name == "winget.exe"
                    && second_name == "winget.exe"
                    && first_fragment == "install"
                    && second_fragment == "install"
                    && first_path == "indexes/winget.exe"
                    && path == "indexes/winget.exe"
            )
    ));
    assert_eq!(
        outline.address,
        Some(DocumentAddress::Markdown {
            path: "indexes/tools".to_owned(),
            origin: MarkdownOrigin::Documents,
        })
    );
    assert!(render_outline_text(&outline).contains(
        "documents: winget → documents/indexes/winget.exe#install, w → documents/indexes/winget.exe#install"
    ));
}

#[test]
fn declared_negated_dash_options_preserve_their_executable_spelling() {
    let parsed = parse_markdown(
        "# tool\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `!--reloadEnvironment`: Disable environment reload.\n- `!--profile=NAME`: Negate one named profile option.\n",
        Some("negated-option.md".to_owned()),
    )
    .expect("negated dash semantic options");
    assert!(parsed.document.diagnostics.is_empty());

    let Block::DefinitionList { items, .. } = &parsed.document.sections[0].blocks[0] else {
        panic!("declared options should become definitions");
    };
    assert_eq!(
        items
            .iter()
            .map(|item| item
                .identity
                .as_ref()
                .expect("option identity")
                .names
                .clone())
            .collect::<Vec<_>>(),
        [vec!["!--reloadEnvironment"], vec!["!--profile"]]
    );

    let content = ResolvedContent {
        address: None,
        label: "negated-option".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    assert!(select_explanation(&content, "!--reloadEnvironment").is_ok());
    assert!(select_explanation(&content, "!--profile").is_ok());
}

#[test]
fn duplicate_entry_aliases_require_a_stable_path_or_id() {
    let parsed = parse_markdown(
        "# tool\n\n## Query\n\n<!-- mant:entries role=option case=insensitive -->\n- `/f`: Force query.\n\n## Delete\n\n<!-- mant:entries role=option case=insensitive -->\n- `/F`: Force deletion.\n",
        None,
    )
    .expect("duplicate entries remain valid input");
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.ambiguous-selector")
            && diagnostic.message.contains("1/e1 (option-f-")
            && diagnostic.message.contains("2/e1 (option-f-")
    }));
    let query = ResolvedContent {
        address: None,
        label: "tool".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };

    let error = select_explanation(&query, "/F").expect_err("bare alias must be ambiguous");
    let ProjectionError::AmbiguousSelector { candidates, .. } = error else {
        panic!("expected a structured ambiguity");
    };
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.path.as_str())
            .collect::<Vec<_>>(),
        ["1/e1", "2/e1"]
    );
    assert_eq!(
        select_explanation(&query, "2/e1")
            .expect("qualified path")
            .selections
            .len(),
        1
    );
}
