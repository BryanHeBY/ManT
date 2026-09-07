//! Existing regressions grouped by tables behavior; expected values remain independent.
use super::*;

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
