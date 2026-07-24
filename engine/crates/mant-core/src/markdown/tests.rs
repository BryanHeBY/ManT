//! Contract-focused tests for Markdown lowering and source preservation.

use mant_ast::{
    Block, Inline, ListKind, OutlineDetail, OutlineNode, QueryBundle, QuerySchema, SectionRole,
    SourceFormat, TableAlignment,
};

use crate::build_outline_with_detail;

use super::parse_markdown;

#[test]
fn lowers_root_content_headings_inlines_lists_tables_and_code() {
    let markdown = "\
Intro with **bold**, *emphasis*, `code`, and [docs](https://example.test).

# Tool

See [options](#options).  
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
    let document = parse_markdown(markdown, Some("/docs/tool.md".to_owned()));

    assert_eq!(document.source.format, SourceFormat::Markdown);
    assert_eq!(document.meta.title.as_deref(), Some("Tool"));
    assert_eq!(document.blocks.len(), 1);
    assert_eq!(document.sections.len(), 1);
    assert_eq!(document.sections[0].id, "tool");
    assert_eq!(document.sections[0].children[0].id, "options");

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

    let tool = &document.sections[0];
    assert!(matches!(
        &tool.blocks[0],
        Block::Paragraph { children, .. }
            if children.iter().any(|inline| matches!(
                inline,
                Inline::SectionReference { target, .. } if target == "options"
            )) && children.iter().any(|inline| matches!(inline, Inline::LineBreak))
    ));

    let options = &tool.children[0];
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
        Block::Preformatted { language: Some(language), children, .. }
            if language == "rust"
                && matches!(&children[0], Inline::Text { value } if value == "fn main() {}\n")
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
    let document = parse_markdown(markdown, None);
    let blocks = &document.sections[0].blocks;

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
fn assigns_unique_heading_ids_and_marks_embedded_quick_references() {
    let document = parse_markdown(
        "# Demo\n\n## TLDR Quick Reference\n\n## Same\n\n## Same\n",
        None,
    );

    assert_eq!(
        document.sections[0].children[0].role,
        Some(SectionRole::QuickReference)
    );
    assert_eq!(document.sections[0].children[1].id, "same");
    assert_eq!(document.sections[0].children[2].id, "same-2");
}

#[test]
fn turns_explicit_option_lists_into_addressable_definitions() {
    let document = parse_markdown(
        "\
# Tool

## Options

- `-h`, `--help`: Show help.
- `--color=WHEN` — Set the colour mode.
",
        None,
    );

    let options = &document.sections[0].children[0];
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
            schema: QuerySchema::V3,
            label: "tool.md".to_owned(),
            document: Some(document),
            tldr: None,
        },
        OutlineDetail::Options,
    )
    .expect("Markdown document has an outline");
    let OutlineNode::DocumentSection { children, .. } = &outline.nodes[0] else {
        panic!("top-level heading should remain a section");
    };
    let OutlineNode::DocumentSection { children, .. } = &children[0] else {
        panic!("options should remain a subsection");
    };
    assert!(matches!(
        &children[0],
        OutlineNode::DocumentEntry { names, .. } if names == &["-h", "--help"]
    ));
}
