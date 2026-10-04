use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Navigation {
    External,
    Manual,
    Multiple,
    Adjacent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Head {
    Empty,
    Word,
    Hard,
    Anchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Body {
    Paragraph,
    Literal,
    List,
    Ordered,
    Table,
    Equation,
    Thematic,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Case {
    pub(super) navigation: Navigation,
    pub(super) head: Head,
    pub(super) body: Body,
    pub(super) relation: HeadBodyRelation,
    pub(super) spacing: u16,
}

pub(super) const fn source(line: u32) -> SourceSpan {
    SourceSpan {
        byte_range: None,
        line,
        column: 1,
        end_line: Some(line),
        end_column: Some(10),
    }
}

fn empty_link(target: LinkTarget) -> Inline {
    Inline::Link {
        target,
        title: None,
        children: vec![],
    }
}

fn external(uri: &str) -> LinkTarget {
    LinkTarget::External { uri: uri.into() }
}

fn manual() -> LinkTarget {
    LinkTarget::Manual {
        name: "navigation-target".into(),
        manual_section: Some("1".into()),
    }
}

impl Navigation {
    pub(super) fn targets(self) -> Vec<LinkTarget> {
        match self {
            Self::External => vec![external(EXTERNAL_A)],
            Self::Manual => vec![manual()],
            Self::Multiple | Self::Adjacent => {
                vec![external(EXTERNAL_A), external(EXTERNAL_B), manual()]
            }
        }
    }

    pub(super) fn paths(self) -> Vec<Vec<u32>> {
        match self {
            Self::External | Self::Manual => vec![vec![0]],
            Self::Multiple => vec![vec![0], vec![1], vec![2]],
            Self::Adjacent => vec![vec![1], vec![2, 0], vec![3]],
        }
    }

    pub(super) fn has_anchors(self) -> bool {
        matches!(self, Self::Adjacent)
    }

    fn anchors(self) -> Vec<Inline> {
        if self.has_anchors() {
            vec![
                Inline::anchor_with_aliases("nav-before", vec!["Nav.Before".into()]),
                Inline::anchor("nav-after"),
            ]
        } else {
            vec![]
        }
    }

    fn nodes(self) -> Vec<Inline> {
        let mut links = self.targets().into_iter().map(empty_link);
        if matches!(self, Self::Adjacent) {
            vec![
                Inline::anchor_with_aliases("nav-before", vec!["Nav.Before".into()]),
                links.next().unwrap(),
                Inline::Strong {
                    children: vec![links.next().unwrap()],
                },
                links.next().unwrap(),
                Inline::anchor("nav-after"),
            ]
        } else {
            links.collect()
        }
    }
}

impl Head {
    pub(super) const fn has_word(self) -> bool {
        matches!(self, Self::Word | Self::Hard)
    }

    fn nodes(self) -> Vec<Inline> {
        let mut nodes = if self.has_word() {
            vec![Inline::Strong {
                children: vec![text(NAME)],
            }]
        } else {
            vec![]
        };
        if self == Self::Hard {
            nodes.push(Inline::LineBreak {});
        } else if self == Self::Anchor {
            nodes.push(Inline::anchor("head-only"));
        }
        nodes
    }
}

fn prose(value: &str, line: u32) -> Block {
    Block::Paragraph {
        children: vec![text(value)],
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: Some(source(line)),
    }
}

fn table_cell(value: &str) -> TableCell {
    TableCell {
        kind: TableCellKind::Text,
        blocks: vec![prose(value, 20)],
        break_after: false,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

impl Body {
    pub(super) const fn last_word(self) -> &'static str {
        match self {
            Self::Literal => "LiteralTail",
            Self::Table => "CellEnd",
            Self::Paragraph | Self::List | Self::Ordered | Self::Equation | Self::Thematic => BODY,
        }
    }

    fn block(self) -> Block {
        match self {
            Self::Paragraph => prose(BODY, 20),
            Self::Literal => Block::Preformatted {
                children: vec![text("Body中\nLiteralTail")],
                inline_layout: InlineLayout::default(),
                language: Some("txt".into()),
                layout: LayoutHint::default(),
                source: Some(source(20)),
            },
            Self::List | Self::Ordered => Block::List {
                kind: if self == Self::Ordered {
                    ListKind::Ordered { start: Some(2) }
                } else {
                    ListKind::Bullet
                },
                compact: true,
                items: vec![ListItem {
                    blocks: vec![prose(BODY, 20)],
                    layout: ListItemLayout::default(),
                    source: None,
                    entry: None,
                }],
                layout: LayoutHint::default(),
                source: Some(source(20)),
            },
            Self::Table => Block::Table {
                rows: vec![TableRow {
                    kind: TableRowKind::Data,
                    cells: vec![table_cell(BODY), table_cell("CellEnd")],
                }],
                column_preferences: ColumnPreferences::default(),
                layout: LayoutHint::default(),
                source: Some(source(20)),
            },
            Self::Equation => Block::Equation {
                value: BODY.into(),
                expression: None,
                display: true,
                layout: LayoutHint::default(),
                source: Some(source(20)),
            },
            Self::Thematic => Block::ThematicBreak {
                source: Some(source(20)),
            },
        }
    }
}

fn facts(head: Head) -> EntryFacts {
    EntryFacts {
        id: "navigation-owner".into(),
        kind: EntryKind::Term,
        case: NameCase::Sensitive,
        names: if head.has_word() {
            vec![NAME.into()]
        } else {
            vec![]
        },
        forms: vec![EntryForm::term(0)],
        name_bindings: if head.has_word() {
            vec![EntryNameBinding {
                name: 0,
                evidence: EntryNameEvidence::Declared,
                occurrences: vec![EntryForm {
                    parts: vec![EntryContentSlice {
                        root: EntryInlineRoot::Term { index: 0 },
                        path: vec![0, 0],
                        bytes: Some(0..NAME.len()),
                    }],
                }],
            }]
        } else {
            vec![]
        },
        alias_groups: vec![],
        alias_of: None,
        value_domain: None,
    }
}

pub(super) fn specimen(case: Case, navigation: bool) -> ResolvedContent {
    let mut value = mant_loader::load_markdown_text("# Probe\n\n## OPTIONS\n", None).unwrap();
    let mut description = vec![];
    if navigation || case.navigation.has_anchors() {
        description.push(Block::Paragraph {
            // The no-link baseline retains authored anchors. Their frozen
            // HTML readback policy is compared verbatim, never filtered.
            children: if navigation {
                case.navigation.nodes()
            } else {
                case.navigation.anchors()
            },
            inline_layout: InlineLayout {
                row_hints: vec![RowLayoutHint {
                    row: 0,
                    indent_columns: 4,
                }],
            },
            layout: LayoutHint::default(),
            source: Some(source(12)),
        });
    }
    description.push(Block::VerticalSpace {
        lines: case.spacing,
        source: Some(source(15)),
    });
    description.push(case.body.block());
    if case.body == Body::Thematic {
        description.push(prose(BODY, 21));
    }
    value.document.as_mut().unwrap().sections[0].blocks = vec![
        Block::DefinitionList {
            items: vec![DefinitionItem {
                terms: vec![case.head.nodes().into()],
                description,
                head_body_relation: case.relation,
                layout: DefinitionLayout::default(),
                entry: Some(facts(case.head)),
                source: Some(source(10)),
            }],
            declaration_groups: vec![],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        },
        prose(TAIL, 30),
    ];
    value
}
