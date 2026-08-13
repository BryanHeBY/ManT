//! Contract-focused tests for Markdown lowering and source preservation.

use mant_ir::{
    Block, DefinitionCase, DefinitionRole, Inline, ListKind, SourceFormat, TableAlignment,
    TldrOrigin,
};
use mant_protocol::{
    ExcerptSelection, OutlineDetail, OutlineNode, SearchCase, SearchNode, SearchQuery, SearchScope,
    SearchSyntax,
};

use crate::{
    ProjectionError, ResolvedContent, build_outline_with_detail, search_query, select_excerpt,
    select_explanation,
};

use super::{parse_document, parse_markdown};

#[test]
fn lowers_root_content_headings_inlines_lists_tables_and_code() {
    let markdown = "\
Intro with **bold**, *emphasis*, `code`, and [docs](https://example.test).

# Tool

See [the top](#tool) and [options](#options).\\
Next line.

## Options

- first
  - nested
- second

1. one
2. two

| Name | Meaning |
| :--- | ---: |
| a | alpha |

```rust
fn main() {}
```

---
";
    let document = parse_document(markdown, Some("/docs/tool.md".to_owned()));

    assert_eq!(document.source.format, SourceFormat::Markdown);
    assert_eq!(document.meta.title.as_deref(), Some("Tool"));
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(document.sections.len(), 1);
    assert_eq!(document.sections[0].id, "options");

    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("intro is a paragraph");
    };
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. } if uri == "https://example.test")
    ));

    assert!(matches!(
        &document.blocks[1],
        Block::Paragraph { children, .. }
            if children.iter().any(|inline| matches!(
                inline,
                Inline::Link { target: mant_ir::LinkTarget::Section { id: target }, .. } if target == "options"
            )) && children.iter().any(|inline| matches!(
                inline,
                Inline::Link { target: mant_ir::LinkTarget::Section { id: target }, .. } if target == "document-overview"
            )) && children.iter().any(|inline| matches!(inline, Inline::LineBreak))
    ));

    let options = &document.sections[0];
    assert!(matches!(
        &options.blocks[0],
        Block::List { kind: ListKind::Bullet, items, .. }
            if items.len() == 2
                && matches!(&items[0].blocks[1], Block::List { kind: ListKind::Bullet, .. })
    ));
    assert!(matches!(
        &options.blocks[1],
        Block::List {
            kind: ListKind::Ordered,
            start: Some(1),
            ..
        }
    ));
    assert!(matches!(
        &options.blocks[2],
        Block::Table { rows, .. }
            if rows.len() == 2
                && rows[0].cells[0].alignment == Some(TableAlignment::Left)
                && rows[0].cells[1].alignment == Some(TableAlignment::Right)
    ));
    assert!(matches!(
        &options.blocks[3],
        Block::Preformatted {
            language: Some(language),
            children,
            layout,
            ..
        }
            if language == "rust"
                && matches!(&children[0], Inline::Text { value } if value == "fn main() {}")
                && layout.indent_columns == 0
                && layout.spacing_before_lines == 1
    ));
    assert!(matches!(
        &options.blocks[1],
        Block::List { layout, .. } if layout.spacing_before_lines == 1
    ));
    assert!(matches!(
        &options.blocks[2],
        Block::Table { layout, .. } if layout.spacing_before_lines == 1
    ));
    assert!(matches!(&options.blocks[4], Block::ThematicBreak { .. }));
    assert!(document.diagnostics.is_empty());
}

#[test]
fn lowers_hierarchical_markdown_links_into_same_source_document_references() {
    let document = parse_document(
        "[Start](Start-Process.md) [Guide](about_Profiles.markdown#examples) [Nested](../other.md)\n",
        Some("/docs/current.md".to_owned()),
    );
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("links are a paragraph");
    };

    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Document { name, fragment: None }, .. } if name == "Start-Process"
    )));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Document { name, fragment: Some(fragment) }, .. }
            if name == "about_Profiles" && fragment == "examples"
    )));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Document { name, fragment: None }, .. } if name == "../other"
    )));
}

#[test]
fn preserves_unsupported_constructs_as_exact_source_with_diagnostics() {
    let markdown = "\
# Unsupported

> quoted **text**

- [x] finished

Text with ~~strike~~, ![alt](image.png), <kbd>raw</kbd>, and $math$.

[^note]: footnote body
";
    let document = parse_document(markdown, None);
    assert!(document.sections.is_empty());
    let blocks = &document.blocks;

    assert!(matches!(
        &blocks[0],
        Block::Unsupported { name: Some(name), text, .. }
            if name == "block quote" && text == "> quoted **text**\n"
    ));
    assert!(matches!(
        &blocks[1],
        Block::Unsupported { name: Some(name), text, .. }
            if name == "task list" && text == "- [x] finished\n\n"
    ));
    let Block::Paragraph { children, .. } = &blocks[2] else {
        panic!("mixed unsupported inline syntax remains in its paragraph");
    };
    let visible = children
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text { value } => Some(value.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(visible.contains("~~strike~~"));
    assert!(visible.contains("![alt](image.png)"));
    assert!(visible.contains("<kbd>raw</kbd>"));
    assert!(visible.contains("$math$"));
    assert!(matches!(
        &blocks[3],
        Block::Unsupported { name: Some(name), text, .. }
            if name == "footnote definition" && text.contains("[^note]: footnote body")
    ));
    assert!(document.diagnostics.len() >= 7);
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.source.is_some())
    );
}

#[test]
fn separates_a_leading_tldr_directive_from_the_document_ir() {
    let parsed = parse_markdown(
        "\
<!-- mant:tldr:start -->
# demo

> A demonstration command.

- Show command help:

`demo --help`
<!-- mant:tldr:end -->

# Demo

Document introduction.

## Same

## Same
",
        None,
    )
    .expect("embedded tldr");

    let tldr = parsed.tldr.expect("quick reference");
    assert_eq!(tldr.title, "demo");
    assert_eq!(tldr.description, ["A demonstration command."]);
    assert_eq!(tldr.examples[0].description, "Show command help");
    assert_eq!(tldr.examples[0].command, "demo --help");
    assert_eq!(tldr.origin, TldrOrigin::Embedded);

    assert_eq!(parsed.document.meta.title.as_deref(), Some("Demo"));
    assert!(matches!(
        parsed.document.blocks.as_slice(),
        [Block::Paragraph { children, source, .. }]
            if matches!(children.as_slice(), [Inline::Text { value }] if value == "Document introduction.")
                && source.is_some_and(|span| span.line == 13)
    ));
    assert_eq!(parsed.document.sections[0].id, "same");
    assert_eq!(parsed.document.sections[1].id, "same-2");
}

#[test]
fn a_reference_to_a_duplicated_heading_resolves_to_the_first_section() {
    let markdown = "\
# Guide

## Options

See [more options](#options).

## Options

Duplicate heading.
";
    let document = parse_document(markdown, None);

    assert_eq!(document.sections[0].id, "options");
    assert_eq!(document.sections[1].id, "options-2");

    // The bare `#options` anchor renders on the first section, so an ambiguous
    // link must resolve there rather than to the later disambiguated duplicate.
    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("first Options section holds the reference paragraph");
    };
    assert!(
        children.iter().any(|inline| matches!(
            inline,
            Inline::Link { target: mant_ir::LinkTarget::Section { id: target }, .. } if target == "options"
        )),
        "a #options link must resolve to the first section, not options-2"
    );
}

#[test]
fn a_heading_slug_colliding_with_a_disambiguated_duplicate_stays_unique() {
    // `# Foo 2` slugs to `foo-2`, the same id a second `# Foo` produces by
    // disambiguation. Every section must still own a distinct id, or search
    // ownership silently misattributes between the collision pair.
    let markdown = "\
# Foo

Alpha.

# Foo

Beta.

# Foo 2

Gamma.
";
    let document = parse_document(markdown, None);

    let ids: Vec<&str> = document
        .sections
        .iter()
        .map(|section| section.id.as_str())
        .collect();
    assert_eq!(ids, ["foo-2", "foo-2-2"]);

    let query = ResolvedContent {
        address: None,
        label: "collision".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let result = search_query(
        &query,
        &SearchQuery {
            pattern: "Gamma".to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .expect("search colliding section");
    assert_eq!(result.total, 1);
    assert!(matches!(
        &result.matches[0].node,
        SearchNode::DocumentSection { path, id, .. }
            if path == "2" && id == "foo-2-2"
    ));
}

#[test]
fn a_leading_byte_order_mark_hides_neither_the_directive_nor_the_title() {
    let parsed = parse_markdown(
        "\u{feff}<!-- mant:tldr:start -->\n# demo\n\n> Saved by a Windows editor.\n\n- Run:\n\n`demo`\n<!-- mant:tldr:end -->\n\n# Demo\n\nBody.\n",
        None,
    )
    .expect("embedded tldr behind a BOM");
    assert_eq!(parsed.tldr.expect("quick reference").title, "demo");
    assert_eq!(parsed.document.meta.title.as_deref(), Some("Demo"));

    let plain = parse_markdown("\u{feff}# Demo\n\nBody.\n", None).expect("plain document");
    assert!(plain.tldr.is_none());
    assert_eq!(plain.document.meta.title.as_deref(), Some("Demo"));
}

#[test]
fn terminal_control_characters_are_masked_with_a_diagnostic() {
    let parsed = parse_markdown(
        "# Demo\n\nx\u{1b}]0;EVIL\u{7}y \u{1b}[31mred\u{1b}[0m z\u{8}\u{8}\n",
        None,
    )
    .expect("document with control characters");
    let document = &parsed.document;

    assert!(
        document
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("markdown.control-characters"))
    );
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("prose survives sanitizing");
    };
    let Inline::Text { value } = &children[0] else {
        panic!("text inline survives sanitizing");
    };
    assert!(!value.contains('\u{1b}') && !value.contains('\u{8}') && !value.contains('\u{7}'));
    assert!(value.contains("red") && value.contains('z'));
}

#[test]
fn leaves_an_ordinary_tldr_heading_in_the_manual() {
    let document = parse_document(
        "\
# Demo

## Synopsis

Normal manual content.

## TLDR

- This late heading is ordinary content:

`demo --help`
",
        None,
    );

    assert_eq!(document.sections[1].title, "TLDR");
    assert_eq!(
        document.sections[1].id, "tldr-section",
        "an ordinary TLDR heading must not shadow the reserved tldr selector"
    );
}

#[test]
fn reserved_selectors_never_shadow_section_ids() {
    let document = parse_document(
        "# Demo\n\n## root\n\nA.\n\n## document-overview\n\nB.\n\n## 1\n\nC.\n",
        None,
    );

    let ids: Vec<&str> = document
        .sections
        .iter()
        .map(|section| section.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["root-section", "document-overview-section", "1-section"]
    );
}

#[test]
fn explicit_heading_ids_cannot_shadow_paths_and_remain_link_targets() {
    let document = parse_document(
        "\
# Demo

See [entry owner](#1/o1), [path owner](#3.1), and [explicit root](#root).

## Entry owner {#1/o1}

- `--help`: Show help.

## Path owner {#3.1}

Path owner body.

## Parent

### Child

Child body.

## Explicit root {#root}

Root body.
",
        None,
    );

    assert_eq!(document.sections[0].id, "1/o1-section");
    assert_eq!(document.sections[1].id, "3.1-section");
    assert_eq!(document.sections[2].children[0].id, "child");
    assert_eq!(document.sections[3].id, "root-section");
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("document preface contains source links");
    };
    let targets = children
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link {
                target: mant_ir::LinkTarget::Section { id: target },
                ..
            } => Some(target.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        ["1/o1-section", "3.1-section", "root-section"],
        "renamed explicit IDs remain valid Markdown link aliases"
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("markdown.unresolved-reference"))
    );

    let query = ResolvedContent {
        address: None,
        label: "demo.md".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let entry = select_excerpt(&query, &["1/o1".to_owned()]).expect("entry path");
    assert!(matches!(
        entry.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { title, .. }] if title.contains("--help")
    ));
    let child = select_excerpt(&query, &["3.1".to_owned()]).expect("child path");
    assert!(matches!(
        child.selections.as_slice(),
        [ExcerptSelection::DocumentSection { title, .. }] if title == "Child"
    ));
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
        Inline::Anchor { id } if id == "option-h"
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
        OutlineNode::DocumentEntry { names, .. } if names == &["-h", "--help"]
    ));
}

#[test]
fn declared_entries_cover_windows_options_commands_and_environment_variables() {
    let parsed = parse_markdown(
        "# tool\n\n## Options\n\n<!-- mant:entries role=option case=insensitive -->\n- `/query`: Query tasks.\n- `/?`: Display help.\n- `/S COMPUTER`: Select a remote computer.\n- `/server:NAME`: Select a server.\n- `/reg:32`, `/reg:64`: Select registry views.\n\n## Commands\n\n<!-- mant:entries role=command case=insensitive -->\n- `query`: Read values.\n- `winget install`: Install a package.\n\n### query\n\nBehavioral details.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=insensitive -->\n- `PATH`, `$env:PATH`: Control executable discovery.\n- `$LASTEXITCODE`: Hold the last native exit code.\n",
        Some("tool.md".to_owned()),
    )
    .expect("declared semantic entries");
    assert!(parsed.document.diagnostics.is_empty());

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
    for selector in ["query", "QUERY"] {
        let command = select_explanation(&query, selector)
            .expect("command alias beats a same-named section ID");
        assert!(matches!(
            command.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { entry, .. }]
                if entry.identity.as_ref().is_some_and(|identity| identity.role == DefinitionRole::Command)
        ));
    }
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
            role: DefinitionRole::Variable,
            ..
        }
    )));
    assert!(matches!(
        &children[0],
        OutlineNode::DocumentEntry { id, names, .. }
            if id == "variable-question-mark" && names == &["$?"]
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
fn duplicate_entry_aliases_require_a_stable_path_or_id() {
    let parsed = parse_markdown(
        "# tool\n\n## Query\n\n<!-- mant:entries role=option case=insensitive -->\n- `/f`: Force query.\n\n## Delete\n\n<!-- mant:entries role=option case=insensitive -->\n- `/F`: Force deletion.\n",
        None,
    )
    .expect("duplicate entries remain valid input");
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.ambiguous-selector")
            && diagnostic.message.contains("1/o1 (option-f)")
            && diagnostic.message.contains("2/o1 (option-f-2)")
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
        ["1/o1", "2/o1"]
    );
    assert_eq!(
        select_explanation(&query, "2/o1")
            .expect("qualified path")
            .selections
            .len(),
        1
    );
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
fn normalized_shorthand_collisions_are_reported_before_selection() {
    let parsed = parse_markdown(
        "# Tool\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `-help`: Short help spelling.\n- `--help`: Long help spelling.\n",
        Some("shorthand-collision.md".to_owned()),
    )
    .expect("shorthand collision fixture");
    assert!(parsed.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("markdown.semantic-entry.ambiguous-selector")
            && diagnostic.message.contains("semantic selector 'help'")
            && diagnostic.message.contains("normalized shorthand")
            && diagnostic.message.contains("1/o1 (option-help)")
            && diagnostic.message.contains("1/o2 (option-help-2)")
    }));
    let query = ResolvedContent {
        address: None,
        label: "shorthand-collision.md".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    for selector in ["-help", "--help"] {
        assert!(select_explanation(&query, selector).is_ok());
    }
    assert!(matches!(
        select_explanation(&query, "help"),
        Err(ProjectionError::AmbiguousSelector { .. })
    ));
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
fn declared_fixed_attached_values_keep_their_official_identity() {
    let parsed = parse_markdown(
        "# Tool\n\n## Options\n\n<!-- mant:entries role=option case=insensitive attached=fixed -->\n- `/F`: Extended scan.\n- `/F:Y`: Extended scan and cleanup.\n- `/server:<NAME>`: Select a server.\n- `perf=default`: Select the default policy.\n",
        None,
    )
    .expect("fixed attached option values");
    assert!(parsed.document.diagnostics.is_empty());
    let query = ResolvedContent {
        address: None,
        label: "tool.md".to_owned(),
        document: Some(parsed.document),
        tldr: None,
    };
    let outline = build_outline_with_detail(&query, OutlineDetail::Entries)
        .expect("fixed attached value outline");
    let OutlineNode::DocumentSection { children, .. } = &outline.nodes[0] else {
        panic!("options section");
    };
    assert!(matches!(
        children.as_slice(),
        [
            OutlineNode::DocumentEntry { id: first_id, title: first_title, names: first_names, .. },
            OutlineNode::DocumentEntry { id: fixed_id, title: fixed_title, names: fixed_names, .. },
            OutlineNode::DocumentEntry { names: placeholder_names, .. },
            OutlineNode::DocumentEntry { id: equals_id, title: equals_title, names: equals_names, .. },
        ] if first_id == "option-f"
            && first_title == "/F"
            && first_names == &["/F"]
            && fixed_id == "option-f-y"
            && fixed_title == "/F:Y"
            && fixed_names == &["/F:Y"]
            && placeholder_names == &["/server"]
            && equals_id == "option-perf-default"
            && equals_title == "perf=default"
            && equals_names == &["perf=default"]
    ));
    for selector in ["/F", "/F:Y", "/f:y", "perf=default"] {
        select_explanation(&query, selector).expect("fixed attached value selector");
    }
}

#[test]
fn declared_option_entries_cover_windows_native_token_families() {
    fn collect_names(nodes: &[OutlineNode], output: &mut Vec<String>) {
        for node in nodes {
            match node {
                OutlineNode::DocumentEntry {
                    names: entry_names, ..
                } => output.extend(entry_names.iter().cloned()),
                OutlineNode::DocumentSection { children, .. } => collect_names(children, output),
                OutlineNode::Tldr { .. } | OutlineNode::DocumentRoot { .. } => {}
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
    assert!(!outline.entries_complete);
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
