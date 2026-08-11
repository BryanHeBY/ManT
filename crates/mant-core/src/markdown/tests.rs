//! Contract-focused tests for Markdown lowering and source preservation.

use mant_ast::{
    Block, DefinitionCase, DefinitionRole, ExcerptSelection, Inline, ListKind, OutlineDetail,
    OutlineNode, QueryBundle, QuerySchema, SearchCase, SearchNode, SearchQuery, SearchScope,
    SearchSyntax, SourceFormat, TableAlignment, TldrOrigin,
};

use crate::{
    ProjectionError, build_outline_with_detail, search_query, select_excerpt, select_explanation,
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
        |inline| matches!(inline, Inline::ExternalLink { uri, .. } if uri == "https://example.test")
    ));

    assert!(matches!(
        &document.blocks[1],
        Block::Paragraph { children, .. }
            if children.iter().any(|inline| matches!(
                inline,
                Inline::SectionReference { target, .. } if target == "options"
            )) && children.iter().any(|inline| matches!(
                inline,
                Inline::SectionReference { target, .. } if target == "document-overview"
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
fn separates_a_leading_tldr_directive_from_the_document_ast() {
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
            Inline::SectionReference { target, .. } if target == "options"
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

    let query = QueryBundle {
        schema: QuerySchema::V5,
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
            Inline::SectionReference { target, .. } => Some(target.as_str()),
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

    let query = QueryBundle {
        schema: QuerySchema::V5,
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
        &QueryBundle {
            schema: QuerySchema::V5,
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

    let query = QueryBundle {
        schema: QuerySchema::V5,
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
fn duplicate_entry_aliases_require_a_stable_path_or_id() {
    let parsed = parse_markdown(
        "# tool\n\n## Query\n\n<!-- mant:entries role=option case=insensitive -->\n- `/f`: Force query.\n\n## Delete\n\n<!-- mant:entries role=option case=insensitive -->\n- `/F`: Force deletion.\n",
        None,
    )
    .expect("duplicate entries remain valid input");
    let query = QueryBundle {
        schema: QuerySchema::V5,
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
fn the_same_alias_in_different_roles_is_ambiguous() {
    let parsed = parse_markdown(
        "# Tool\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- `PATH`: Run a command.\n\n## Environment\n\n<!-- mant:entries role=environment-variable case=sensitive -->\n- `PATH`: Configure discovery.\n",
        None,
    )
    .expect("cross-role alias fixture");
    let query = QueryBundle {
        schema: QuerySchema::V5,
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
    let query = QueryBundle {
        schema: QuerySchema::V5,
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
    let query = QueryBundle {
        schema: QuerySchema::V5,
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
        diagnostic.code.as_deref() == Some("markdown.semantic-entry-list")
            && diagnostic.source.is_some_and(|source| source.line == 6)
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
        diagnostic.code.as_deref() == Some("markdown.semantic-entry-list")
            && diagnostic.message.contains("invalid item")
    }));
}
