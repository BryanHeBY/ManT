//! Accepted row layout precedes every final UTF-8 owner interval.
use super::*;
use mant_ir::{EntryFacts, EntryKind, ListItem, ListKind, NameCase};

fn owned(block: Block, id: &str) -> Block {
    Block::List {
        kind: ListKind::Plain,
        compact: true,
        items: vec![ListItem {
            blocks: vec![block],
            layout: mant_ir::ListItemLayout::default(),
            entry: Some(EntryFacts {
                id: id.into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: Vec::new(),
                forms: Vec::new(),
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                value_domain: None,
            }),
            source: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn normalized_source_rows_map_both_addressable_owners_after_growth_or_shrinkage() {
    for (raw, hints, first) in [
        ("FIRST\r字\nZ", vec![hint(1, 3, 0)], "FIRST\n字\n   Z"),
        ("FIRST\r\n字\nZ", vec![hint(1, 3, 0)], "FIRST\n   字\nZ"),
        (
            "FIRST````\r字\n😀e\u{301}",
            vec![hint(0, 1, 2), hint(1, 2, -1)],
            "   FIRST````\n 字\n 😀e\u{301}",
        ),
    ] {
        let block = table(vec![
            owned(
                literal(
                    vec![Inline::anchor("row-owner"), text(raw)],
                    InlineLayout { row_hints: hints },
                ),
                "first",
            ),
            owned(
                Block::Paragraph {
                    children: vec![text("SECOND")],
                    inline_layout: InlineLayout::default(),
                    layout: LayoutHint::default(),
                    source: None,
                },
                "second",
            ),
        ]);
        let mut query = parse_content("# Row owners\n", None).unwrap();
        query.document.as_mut().unwrap().blocks = vec![block.clone()];
        assert_eq!(
            mant_ir::validate_document(query.document.as_ref().unwrap()),
            []
        );
        let original = query.clone();
        for preserve_anchors in [false, true] {
            let options = MarkdownOptions {
                preserve_anchors,
                ..MarkdownOptions::default()
            };
            let artifact = render_addressable_markdown_with_options(&query, options);
            assert_eq!(
                artifact.text(),
                render_markdown_with_options(&query, options)
            );
            let mut count = 0;
            for node in artifact.nodes() {
                let crate::encode::MarkdownNode::DocumentEntry { owner, .. } = node.node() else {
                    continue;
                };
                let id = owner.facts().unwrap().id.as_str();
                let expected = if id == "first" {
                    first
                } else {
                    assert_eq!(id, "second");
                    "SECOND"
                };
                let range = node.range();
                assert!(artifact.text().is_char_boundary(range.start));
                assert!(artifact.text().is_char_boundary(range.end));
                assert_eq!(artifact.text().get(range), Some(expected));
                count += 1;
            }
            assert_eq!(count, 2);
            assert_eq!(query, original);
        }
        assert_projection(&block, &format!("{first}\nSECOND"));
    }
}
