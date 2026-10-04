//! A navigation-only first paragraph cannot add content after a list marker.
use super::*;

#[derive(Clone, Copy, Debug)]
enum Prefix {
    Empty,
    Text,
    Anchor,
    External,
    Styled,
    Spaces,
    Hard,
    Completed,
}

impl Prefix {
    fn nodes(self) -> Vec<Inline> {
        let link = Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.test/empty".into(),
            },
            title: None,
            children: vec![],
        };
        match self {
            Self::Empty => vec![],
            Self::Text => vec![Inline::Text {
                value: String::new(),
            }],
            Self::Anchor => vec![Inline::anchor("prefix")],
            Self::External => vec![link],
            Self::Styled => vec![Inline::Strong {
                children: vec![link],
            }],
            Self::Spaces => vec![Inline::Text { value: "  ".into() }],
            Self::Hard => vec![Inline::LineBreak {}],
            Self::Completed => vec![Inline::LineBreak {}, Inline::LineBreak {}],
        }
    }
}

fn first(prefix: Prefix, spacing: u16) -> Block {
    let mut block = paragraph("", 3);
    let Block::Paragraph {
        children, layout, ..
    } = &mut block
    else {
        unreachable!()
    };
    *children = prefix.nodes();
    layout.spacing_before_lines = spacing;
    block
}

fn body(literal: bool) -> Block {
    if literal {
        Block::Preformatted {
            children: vec![Inline::Text {
                value: "BODY".into(),
            }],
            inline_layout: mant_ir::InlineLayout::default(),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        }
    } else {
        paragraph("BODY", 0)
    }
}

fn list(kind: ListKind, blocks: Vec<Block>, item_spacing: Option<u16>) -> Block {
    Block::List {
        kind,
        compact: true,
        items: vec![ListItem {
            blocks,
            layout: mant_ir::ListItemLayout {
                spacing_before_lines: item_spacing,
            },
            source: None,
            entry: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn render(block: &Block) -> String {
    let wire = serde_json::to_string(block).unwrap();
    let restored: Block = serde_json::from_str(&wire).unwrap();
    assert_eq!(&restored, block);
    super::super::super::plain_renderer().render_blocks(
        &[paragraph("BEFORE", 0), restored, paragraph("AFTER", 0)],
        0,
    )
}

#[test]
fn empty_first_roots_keep_the_original_bare_marker_and_body_rows() {
    for (kind, marker, indent) in [
        (ListKind::Bullet, "•", "  "),
        (ListKind::Ordered { start: Some(2) }, "2.", "   "),
    ] {
        for literal in [false, true] {
            for spacing in [0, 1] {
                let expected = format!(
                    "BEFORE\n{}{marker}\n{indent}BODY\nAFTER",
                    "\n".repeat(usize::from(spacing))
                );
                let baseline = render(&list(
                    kind,
                    vec![first(Prefix::Empty, spacing), body(literal)],
                    None,
                ));
                assert_eq!(baseline, expected);
                for prefix in [
                    Prefix::Empty,
                    Prefix::Text,
                    Prefix::Anchor,
                    Prefix::External,
                    Prefix::Styled,
                ] {
                    let actual = render(&list(
                        kind,
                        vec![first(prefix, spacing), body(literal)],
                        None,
                    ));
                    assert_eq!(actual, expected, "{kind:?}/{prefix:?}/{literal}/{spacing}");
                    assert_eq!(actual, baseline);
                }
                if literal {
                    // The original non-paragraph route already emits a bare
                    // marker. A zero-width prefix must preserve that contract.
                    assert_eq!(
                        render(&list(kind, vec![body(true)], Some(spacing))),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn author_spaces_hard_rows_and_positive_spacing_keep_their_marker_content() {
    for (kind, marker, indent) in [
        (ListKind::Bullet, "•", "  "),
        (ListKind::Ordered { start: Some(2) }, "2.", "   "),
    ] {
        for literal in [false, true] {
            for spacing in [0, 1] {
                for (prefix, marker_tail, hard_row) in [
                    (Prefix::Spaces, "      ", ""),
                    (Prefix::Hard, "    ", ""),
                    (Prefix::Completed, "    ", "\n"),
                ] {
                    let actual = render(&list(
                        kind,
                        vec![first(prefix, spacing), body(literal)],
                        None,
                    ));
                    let expected = format!(
                        "BEFORE\n{}{marker}{marker_tail}\n{hard_row}{indent}BODY\nAFTER",
                        "\n".repeat(usize::from(spacing))
                    );
                    assert_eq!(actual, expected, "{kind:?}/{prefix:?}/{literal}/{spacing}");
                }
            }
        }
    }
}
