//! Effective BODY roots retain original prefix navigation and physical rows.

use super::super::*;
use super::fixtures::{
    cell, copy_cells, json_content, literal_nodes, paragraph_nodes, table, text,
};
use mant_ir::{ColumnPreferences, DefinitionLayout, HeadBodyRelation, InlineLayout, RowLayoutHint};

#[derive(Clone, Copy, Debug)]
enum Prefix {
    Paragraph,
    EmptyText,
    Hint,
    Literal,
    LiteralAnchor,
    EmptyReference,
    ZeroGap,
    Spaces,
    LiteralText,
    ParagraphBreak,
    LiteralBreak,
    Gap,
    Spacing,
    List,
    Table,
}

fn prefix_anchor() -> Inline {
    Inline::anchor_with_aliases("prefix", vec!["PREFIX.Exact".into(), "prefix.alias".into()])
}

fn reference(children: Vec<Inline>) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::Manual {
            name: "destination".into(),
            manual_section: Some("1".into()),
        },
        title: None,
        children,
    }
}

fn hinted_paragraph(children: Vec<Inline>) -> Block {
    let mut block = paragraph_nodes(children);
    if let Block::Paragraph { inline_layout, .. } = &mut block {
        inline_layout.row_hints = vec![RowLayoutHint {
            row: 0,
            indent_columns: 4,
        }];
    }
    block
}

impl Prefix {
    fn blocks(self) -> Vec<Block> {
        let block = match self {
            Self::Paragraph => paragraph_nodes(vec![]),
            Self::EmptyText => paragraph_nodes(vec![text("")]),
            Self::Hint => hinted_paragraph(vec![]),
            Self::Literal => literal_nodes(vec![]),
            Self::LiteralAnchor => literal_nodes(vec![Inline::anchor("literal-prefix")]),
            Self::EmptyReference => hinted_paragraph(vec![reference(vec![])]),
            Self::ZeroGap => Block::VerticalSpace {
                lines: 0,
                source: None,
            },
            Self::Spaces => paragraph_nodes(vec![text("  ")]),
            Self::LiteralText => literal_nodes(vec![text("")]),
            Self::ParagraphBreak => paragraph_nodes(vec![Inline::LineBreak {}]),
            Self::LiteralBreak => literal_nodes(vec![Inline::LineBreak {}]),
            Self::Gap => Block::VerticalSpace {
                lines: 1,
                source: None,
            },
            Self::Spacing => {
                let mut block = paragraph_nodes(vec![]);
                if let Block::Paragraph { layout, .. } = &mut block {
                    layout.spacing_before_lines = 1;
                }
                block
            }
            Self::List => Block::List {
                kind: ListKind::Plain,
                compact: true,
                items: vec![],
                layout: LayoutHint::default(),
                source: None,
            },
            Self::Table => table(vec![], ColumnPreferences::default(), 0),
        };
        vec![paragraph_nodes(vec![prefix_anchor()]), block]
    }

    const fn transparent(self) -> bool {
        matches!(
            self,
            Self::Paragraph
                | Self::EmptyText
                | Self::Hint
                | Self::Literal
                | Self::LiteralAnchor
                | Self::EmptyReference
                | Self::ZeroGap
        )
    }

    const fn can_share(self) -> bool {
        !matches!(self, Self::Gap | Self::Spacing | Self::List | Self::Table)
    }

    fn body_row(self, heads: usize, shared: bool) -> usize {
        if self.transparent() {
            return if shared { heads - 1 } else { heads };
        }
        match self {
            Self::Gap | Self::Spacing | Self::Table => heads + 1,
            Self::List => heads,
            Self::LiteralBreak => heads + if shared { 1 } else { 2 },
            _ => heads + usize::from(!shared),
        }
    }

    fn anchor_row(self, heads: usize, shared: bool) -> usize {
        if self.transparent() || matches!(self, Self::Gap | Self::Spacing | Self::List) {
            self.body_row(heads, shared)
        } else {
            heads - usize::from(shared)
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Container {
    Root,
    Plain,
    Table,
    Nested,
}

fn wrap(block: Block, container: Container) -> Block {
    let preferences = || ColumnPreferences {
        widths: vec![64],
        ..Default::default()
    };
    match container {
        Container::Root => block,
        Container::Plain => Block::List {
            kind: ListKind::Plain,
            compact: true,
            items: vec![ListItem {
                blocks: vec![block],
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        Container::Table => table(vec![cell(vec![block], false)], preferences(), 0),
        Container::Nested => table(
            vec![cell(
                vec![table(vec![cell(vec![block], false)], preferences(), 0)],
                false,
            )],
            preferences(),
            0,
        ),
    }
}

fn definition(prefix: Prefix, heads: usize, relation: HeadBodyRelation, literal: bool) -> Block {
    let mut terms = Vec::new();
    if heads == 2 {
        terms.push(vec![text("A")].into());
    }
    terms.push(vec![Inline::anchor("last-head"), text("X")].into());
    let mut description = prefix.blocks();
    let nodes = vec![Inline::anchor("actual-body"), reference(vec![text("Y")])];
    let mut body = if literal {
        literal_nodes(nodes)
    } else {
        paragraph_nodes(nodes)
    };
    match &mut body {
        Block::Paragraph { inline_layout, .. } | Block::Preformatted { inline_layout, .. } => {
            *inline_layout = InlineLayout {
                row_hints: vec![RowLayoutHint {
                    row: 0,
                    indent_columns: 2,
                }],
            };
        }
        _ => unreachable!(),
    }
    description.push(body);
    Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            terms,
            description,
            head_body_relation: relation,
            layout: DefinitionLayout::default(),
            entry: None,
            source: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn hit(rendered: &RenderedDocument, word: &str) -> RenderedSearchMatch {
    let matches = rendered.search(word);
    assert_eq!(matches.len(), 1, "{word}: {:?}", rendered.text);
    let hit = matches[0].clone();
    assert_eq!(
        copy_cells(rendered, hit.row, hit.start_column, hit.end_column - 1),
        word
    );
    hit
}

fn assert_body(
    rendered: &RenderedDocument,
    prefix: Prefix,
    heads: usize,
    relation: HeadBodyRelation,
) -> RenderedSearchMatch {
    let shared = relation != HeadBodyRelation::Separate && prefix.can_share();
    let body = hit(rendered, "Y");
    let row = prefix.body_row(heads, shared);
    let column = if shared && prefix.transparent() && relation == HeadBodyRelation::joined() {
        1
    } else {
        6
    };
    assert_eq!(
        (body.row, body.start_column),
        (row, column),
        "{prefix:?}/{heads}/{relation:?}"
    );
    assert_eq!(
        rendered.row_count,
        row + 2,
        "{prefix:?}/{heads}/{relation:?}"
    );
    assert_eq!(hit(rendered, "END").row, row + 1);
    let expected = if shared && prefix.transparent() {
        if relation == HeadBodyRelation::joined() {
            "XY"
        } else {
            "X   Y"
        }
    } else {
        "    Y"
    };
    assert_eq!(
        copy_cells(rendered, row, 0, 79),
        expected,
        "{prefix:?}/{heads}/{relation:?}"
    );
    assert_eq!(rendered.anchor_row("actual-body"), Some(row));
    assert_eq!(rendered.anchor_row("last-head"), Some(heads - 1));
    for id in ["prefix", "PREFIX.Exact", "prefix.alias"] {
        assert_eq!(
            rendered.anchor_row(id),
            Some(prefix.anchor_row(heads, shared)),
            "{prefix:?}/{heads}/{relation:?}/{id}"
        );
    }
    assert_eq!(hit(rendered, "X").row, heads - 1);
    if heads == 2 {
        assert_eq!(hit(rendered, "A").row, 0);
    }
    body
}

fn assert_prefix_rows(
    rendered: &RenderedDocument,
    prefix: Prefix,
    heads: usize,
    relation: HeadBodyRelation,
    body_row: usize,
) {
    let shared = relation != HeadBodyRelation::Separate && prefix.can_share();
    for row in heads..body_row {
        if !matches!(prefix, Prefix::Spaces) {
            assert_eq!(
                copy_cells(rendered, row, 0, 79),
                "",
                "{prefix:?}/{heads}/{relation:?}/{row}"
            );
        }
    }
    if matches!(prefix, Prefix::Spaces) {
        let column = if shared && relation == HeadBodyRelation::joined() {
            1
        } else {
            4
        };
        assert_eq!(
            copy_cells(
                rendered,
                prefix.anchor_row(heads, shared),
                column,
                column + 1
            ),
            "  "
        );
    }
    if matches!(prefix, Prefix::LiteralAnchor) {
        assert_eq!(rendered.anchor_row("literal-prefix"), Some(body_row));
    }
}

fn assert_references(
    view: &DocumentView,
    rendered: &RenderedDocument,
    content: &ResolvedContent,
    prefix: Prefix,
    body: &RenderedSearchMatch,
) {
    let document = content.document.as_ref().unwrap();
    let reference = view
        .references
        .iter()
        .find(|reference| reference.label == "Y")
        .unwrap();
    assert_eq!(rendered.anchor_row(&reference.id), Some(body.row));
    let owner = reference.location.inline_content(document).unwrap();
    assert_eq!(mant_ir::inline_plain_text(owner.content), "Y");
    assert_eq!(owner.layout.row_indent(0), 2);
    assert!(reference.location.resolve_link(document).is_some());
    assert_eq!(
        view.references.len(),
        1 + usize::from(matches!(prefix, Prefix::EmptyReference))
    );
    assert_eq!(rendered.links.len(), 1);
    assert_eq!(
        rendered.link_target_at(body.row, body.start_column),
        Some(&LinkTarget::Document {
            address: mant_ir::DocumentAddress::Manual {
                name: "destination".into(),
                manual_section: "1".into()
            },
            fragment: None,
        })
    );
    assert_eq!(rendered.link_target_at(body.row, body.end_column), None);
    if matches!(prefix, Prefix::EmptyReference) {
        let empty = view
            .references
            .iter()
            .find(|reference| reference.label != "Y")
            .unwrap();
        let root = empty.location.inline_content(document).unwrap();
        assert_eq!(mant_ir::inline_plain_text(root.content), "");
        assert_eq!(root.layout.row_indent(0), 4);
        assert_eq!(rendered.anchor_row(&empty.id), Some(body.row));
        assert!(empty.location.resolve_link(document).is_some());
    }
}

#[test]
fn effective_body_prefixes_preserve_shared_words_rows_and_source_navigation() {
    for prefix in [
        Prefix::Paragraph,
        Prefix::EmptyText,
        Prefix::Hint,
        Prefix::Literal,
        Prefix::LiteralAnchor,
        Prefix::EmptyReference,
        Prefix::ZeroGap,
        Prefix::Spaces,
        Prefix::LiteralText,
        Prefix::ParagraphBreak,
        Prefix::LiteralBreak,
        Prefix::Gap,
        Prefix::Spacing,
        Prefix::List,
        Prefix::Table,
    ] {
        for heads in [1, 2] {
            for relation in [
                HeadBodyRelation::joined(),
                HeadBodyRelation::separated(),
                HeadBodyRelation::Separate,
            ] {
                for container in [
                    Container::Root,
                    Container::Plain,
                    Container::Table,
                    Container::Nested,
                ] {
                    for literal in [false, true] {
                        let content = json_content(vec![
                            wrap(definition(prefix, heads, relation, literal), container),
                            paragraph("END"),
                        ]);
                        let view = DocumentView::new(&content);
                        let rendered = view.render(80);
                        let body = assert_body(&rendered, prefix, heads, relation);
                        assert_prefix_rows(&rendered, prefix, heads, relation, body.row);
                        assert_references(&view, &rendered, &content, prefix, &body);
                    }
                }
            }
        }
    }
}
