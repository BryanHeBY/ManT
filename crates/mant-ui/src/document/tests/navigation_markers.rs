//! Zero-width first roots preserve existing UI marker cells and navigation.
use super::*;

const EMPTY_URI: &str = "https://example.test/empty";
const BODY_URI: &str = "https://example.test/body";

#[derive(Clone, Copy, Debug)]
enum Prefix {
    Empty,
    Text,
    Anchor,
    External,
    Styled,
    Manual,
    StyledManual,
    Spaces,
    Hard,
    Completed,
}

fn link(target: mant_ir::LinkTarget, children: Vec<Inline>) -> Inline {
    Inline::Link {
        target,
        children,
        title: None,
    }
}

impl Prefix {
    fn nodes(self) -> Vec<Inline> {
        let target = if matches!(self, Self::Manual | Self::StyledManual) {
            mant_ir::LinkTarget::Manual {
                name: "destination".into(),
                manual_section: Some("1".into()),
            }
        } else {
            mant_ir::LinkTarget::External {
                uri: EMPTY_URI.into(),
            }
        };
        match self {
            Self::Empty => vec![],
            Self::Text => vec![Inline::Text {
                value: String::new(),
            }],
            Self::Anchor => vec![Inline::anchor_with_aliases(
                "prefix",
                vec!["Prefix.Exact".into()],
            )],
            Self::External | Self::Manual => vec![link(target, vec![])],
            Self::Styled | Self::StyledManual => vec![Inline::Strong {
                children: vec![link(target, vec![])],
            }],
            Self::Spaces => vec![Inline::Text { value: "  ".into() }],
            Self::Hard => vec![Inline::LineBreak {}],
            Self::Completed => vec![Inline::LineBreak {}, Inline::LineBreak {}],
        }
    }
}

fn first(prefix: Prefix, spacing: u16) -> Block {
    Block::Paragraph {
        children: prefix.nodes(),
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint {
            indent_columns: 3,
            spacing_before_lines: spacing,
            ..Default::default()
        },
        source: None,
    }
}

fn body(literal: bool) -> Block {
    let children = vec![link(
        mant_ir::LinkTarget::External {
            uri: BODY_URI.into(),
        },
        vec![Inline::Text {
            value: "BODY".into(),
        }],
    )];
    if literal {
        Block::Preformatted {
            children,
            inline_layout: mant_ir::InlineLayout::default(),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        }
    } else {
        Block::Paragraph {
            children,
            inline_layout: mant_ir::InlineLayout::default(),
            layout: LayoutHint::default(),
            source: None,
        }
    }
}

fn content(kind: ListKind, blocks: Vec<Block>, spacing: Option<u16>) -> ResolvedContent {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![
        paragraph("BEFORE"),
        Block::List {
            kind,
            compact: true,
            items: vec![ListItem {
                blocks,
                layout: mant_ir::ListItemLayout {
                    spacing_before_lines: spacing,
                },
                source: None,
                entry: None,
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        paragraph("AFTER"),
    ];
    assert_eq!(mant_ir::validate_document(document).len(), 0);
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
    let restored: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into();
    assert_eq!(restored, content);
    restored
}

fn copied_rows(rendered: &RenderedDocument) -> Vec<String> {
    (0..rendered.row_count)
        .map(|row| {
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition { row, column: 0 },
                focus: TextPosition { row, column: 79 },
            })
        })
        .collect()
}

fn assert_body(rendered: &RenderedDocument, row: usize, column: usize) {
    let hits = rendered.search("BODY");
    let [hit] = hits.as_slice() else {
        panic!("one BODY: {hits:?}")
    };
    assert_eq!(
        (hit.row, hit.start_column, hit.end_column),
        (row, column, column + 4)
    );
    let target = LinkTarget::External(ExternalUri::parse(BODY_URI).unwrap());
    assert_eq!(rendered.link_target_at(row, column), Some(&target));
    assert_eq!(rendered.link_target_at(row, column + 4), None);
    assert_eq!(
        rendered.links.len(),
        1,
        "empty labels cannot invent a hit region"
    );
    assert_eq!(rendered.search("AFTER")[0].row, row + 1);
}

fn assert_navigation(
    content: &ResolvedContent,
    view: &DocumentView,
    rendered: &RenderedDocument,
    prefix: Prefix,
    marker_row: usize,
) {
    if matches!(prefix, Prefix::Anchor) {
        for id in ["prefix", "Prefix.Exact"] {
            assert_eq!(rendered.anchor_row(id), Some(marker_row));
        }
    }
    let manual = matches!(prefix, Prefix::Manual | Prefix::StyledManual);
    assert_eq!(view.references.len(), usize::from(manual));
    if manual {
        let reference = &view.references[0];
        assert_eq!(rendered.anchor_row(&reference.id), Some(marker_row));
        let document = content.document.as_ref().unwrap();
        let root = reference.location.inline_content(document).unwrap();
        assert_eq!(mant_ir::inline_plain_text(root.content), "");
        let Inline::Link {
            children, target, ..
        } = reference.location.resolve_link(document).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(children, &Vec::<Inline>::new());
        assert_eq!(
            target,
            &mant_ir::LinkTarget::Manual {
                name: "destination".into(),
                manual_section: Some("1".into())
            }
        );
    }
}

fn expected_rows(
    marker: &str,
    indent: usize,
    spacing: u16,
    suffix: &str,
    blank: bool,
) -> Vec<String> {
    let mut rows = vec!["BEFORE".into()];
    rows.extend((0..spacing).map(|_| String::new()));
    rows.push(format!("{marker}{suffix}"));
    if blank {
        rows.push(String::new());
    }
    rows.push(format!("{}BODY", " ".repeat(indent)));
    rows.push("AFTER".into());
    rows
}

#[test]
fn navigation_only_roots_match_existing_ui_marker_rows_after_json() {
    for (kind, marker, indent) in [
        (ListKind::Bullet, "• ", 2),
        (ListKind::Ordered { start: Some(2) }, "2. ", 3),
    ] {
        for literal in [false, true] {
            for spacing in [0, 1] {
                let expected = expected_rows(marker, indent, spacing, "", false);
                let baseline = content(
                    kind,
                    vec![first(Prefix::Empty, spacing), body(literal)],
                    None,
                );
                assert_eq!(
                    copied_rows(&DocumentView::new(&baseline).render(80)),
                    expected
                );
                for prefix in [
                    Prefix::Empty,
                    Prefix::Text,
                    Prefix::Anchor,
                    Prefix::External,
                    Prefix::Styled,
                    Prefix::Manual,
                    Prefix::StyledManual,
                ] {
                    let content = content(kind, vec![first(prefix, spacing), body(literal)], None);
                    let view = DocumentView::new(&content);
                    let rendered = view.render(80);
                    assert_eq!(
                        copied_rows(&rendered),
                        expected,
                        "{kind:?}/{prefix:?}/{literal}/{spacing}"
                    );
                    assert_eq!(
                        rendered.text.lines[usize::from(spacing) + 1].to_string(),
                        marker
                    );
                    assert_body(&rendered, usize::from(spacing) + 2, indent);
                    assert_navigation(&content, &view, &rendered, prefix, usize::from(spacing) + 1);
                }
                if literal {
                    // UI preserves its established marker separator cell;
                    // removing navigation must not migrate that format policy.
                    let bare = content(kind, vec![body(true)], Some(spacing));
                    assert_eq!(copied_rows(&DocumentView::new(&bare).render(80)), expected);
                }
            }
        }
    }
}

#[test]
fn authored_whitespace_hard_rows_and_completed_spacing_remain_copyable() {
    for (kind, marker, indent) in [
        (ListKind::Bullet, "• ", 2),
        (ListKind::Ordered { start: Some(2) }, "2. ", 3),
    ] {
        for literal in [false, true] {
            for spacing in [0, 1] {
                for (prefix, suffix, blank) in [
                    (Prefix::Spaces, "     ", false),
                    (Prefix::Hard, "   ", false),
                    (Prefix::Completed, "   ", true),
                ] {
                    let content = content(kind, vec![first(prefix, spacing), body(literal)], None);
                    let rendered = DocumentView::new(&content).render(80);
                    assert_eq!(
                        copied_rows(&rendered),
                        expected_rows(marker, indent, spacing, suffix, blank),
                        "{kind:?}/{prefix:?}/{literal}/{spacing}"
                    );
                    assert_body(
                        &rendered,
                        usize::from(spacing) + 2 + usize::from(blank),
                        indent,
                    );
                    if matches!(prefix, Prefix::Spaces) {
                        let row = usize::from(spacing) + 1;
                        assert_eq!(
                            rendered.selected_text(RenderedSelection {
                                anchor: TextPosition {
                                    row,
                                    column: indent + 3
                                },
                                focus: TextPosition {
                                    row,
                                    column: indent + 4
                                },
                            }),
                            "  "
                        );
                    }
                }
            }
        }
    }
}
