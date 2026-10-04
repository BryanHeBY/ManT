use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Context {
    Root,
    Section,
    List,
    Definition,
    Nested,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Position {
    Prefix,
    Middle,
    Tail,
    TailFollow,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Zero {
    Paragraph,
    Link,
    LiteralNone,
    LiteralEmpty,
    NestedCarrier,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Gap {
    Vertical,
    Block,
    Item,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Physical {
    Paragraph,
    Opaque,
    InlineEquation,
    Fence,
    List,
    Rule,
}

pub(super) const fn source(line: u32) -> SourceSpan {
    SourceSpan {
        byte_range: None,
        line,
        column: 1,
        end_line: Some(line),
        end_column: Some(20),
    }
}

pub(super) fn prose(children: Vec<Inline>, line: u32) -> Block {
    Block::Paragraph {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: Some(source(line)),
    }
}

fn literal(children: Vec<Inline>, language: &str, line: u32) -> Block {
    Block::Preformatted {
        children,
        inline_layout: InlineLayout::default(),
        language: Some(language.into()),
        layout: LayoutHint::default(),
        source: Some(source(line)),
    }
}

fn facts(root: EntryInlineRoot, named: bool) -> EntryFacts {
    EntryFacts {
        id: OWNER.into(),
        kind: EntryKind::Term,
        case: NameCase::Sensitive,
        names: if named { vec![NAME.into()] } else { vec![] },
        forms: vec![EntryForm {
            parts: vec![EntryContentSlice {
                root: root.clone(),
                path: vec![],
                bytes: None,
            }],
        }],
        name_bindings: if named {
            vec![EntryNameBinding {
                name: 0,
                evidence: EntryNameEvidence::Declared,
                occurrences: vec![EntryForm {
                    parts: vec![EntryContentSlice {
                        root,
                        path: vec![0],
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

fn head() -> Vec<Inline> {
    vec![
        text(NAME),
        Inline::Anchor {
            id: "contribution-anchor".into(),
            fragment_aliases: vec!["Contribution.Anchor".into()],
            owner_source: Some(source(10)),
        },
    ]
}

fn definition(terms: Vec<Inline>, description: Vec<Block>, entry: Option<EntryFacts>) -> Block {
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![terms.into()],
            description,
            head_body_relation: HeadBodyRelation::Separate,
            layout: DefinitionLayout::default(),
            source: Some(source(10)),
            entry,
        }],
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn list(mut blocks: Vec<Block>, named: bool) -> Block {
    if named {
        blocks.insert(0, prose(head(), 10));
    }
    Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            blocks,
            layout: ListItemLayout::default(),
            source: Some(source(10)),
            entry: named.then(|| facts(EntryInlineRoot::Block { index: 0 }, true)),
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn link() -> Block {
    prose(
        vec![Inline::Link {
            target: LinkTarget::External { uri: URI.into() },
            title: None,
            children: vec![],
        }],
        12,
    )
}

pub(super) fn zero(kind: Zero) -> Block {
    match kind {
        Zero::Paragraph => prose(vec![], 12),
        Zero::Link => link(),
        Zero::LiteralNone => literal(vec![], "empty", 12),
        Zero::LiteralEmpty => literal(vec![text("")], "empty", 12),
        Zero::NestedCarrier => definition(vec![], vec![link()], None),
    }
}

pub(super) fn named_navigation() -> Block {
    definition(
        vec![],
        vec![link()],
        Some(facts(EntryInlineRoot::Term { index: 0 }, false)),
    )
}

pub(super) fn spaced_carrier(gap: Gap) -> Block {
    let mut node = definition(vec![], vec![link()], None);
    let Block::DefinitionList { items, .. } = &mut node else {
        unreachable!()
    };
    match gap {
        Gap::Vertical => items[0].description.insert(
            0,
            Block::VerticalSpace {
                lines: 1,
                source: Some(source(11)),
            },
        ),
        Gap::Block => {
            geometry::block_layout_mut(&mut items[0].description[0])
                .unwrap()
                .spacing_before_lines = 1;
        }
        Gap::Item => items[0].layout.spacing_before_lines = Some(1),
    }
    node
}

impl Physical {
    pub(super) fn blocks(self) -> Vec<Block> {
        let node = match self {
            Self::Paragraph => prose(vec![text(BODY)], 20),
            Self::Opaque => Block::Unsupported {
                name: Some("Opaque".into()),
                text: BODY.into(),
                layout: LayoutHint::default(),
                source: Some(source(20)),
            },
            Self::InlineEquation => Block::Equation {
                value: BODY.into(),
                expression: None,
                display: false,
                layout: LayoutHint::default(),
                source: Some(source(20)),
            },
            Self::Fence => literal(vec![text("Contribution中\nLiteralEnd")], "contract", 20),
            Self::List => list(vec![prose(vec![text(BODY)], 20)], false),
            Self::Rule => {
                return vec![
                    Block::ThematicBreak {
                        source: Some(source(20)),
                    },
                    prose(vec![text(BODY)], 21),
                ];
            }
        };
        vec![node]
    }

    pub(super) const fn phrase(self) -> &'static str {
        match self {
            Self::Opaque => "Opaque: Contribution中",
            Self::InlineEquation => "Equation: Contribution中",
            _ => BODY,
        }
    }
}

pub(super) fn sequence(kind: Zero, position: Position) -> Vec<Block> {
    let before = prose(vec![text(BEFORE)], 19);
    let body = prose(vec![text(BODY)], 20);
    let after = prose(vec![text(AFTER)], 21);
    match position {
        Position::Prefix => vec![zero(kind), body, after],
        Position::Middle => vec![before, zero(kind), body, after],
        Position::Tail => vec![before, body, zero(kind)],
        Position::TailFollow => vec![before, body, zero(kind), after],
    }
}

pub(super) fn install(context: Context, blocks: Vec<Block>) -> ResolvedContent {
    let mut value = mant_loader::load_markdown_text("# Probe\n\n## OPTIONS\n", None).unwrap();
    let document = value.document.as_mut().unwrap();
    match context {
        Context::Root => {
            document.sections.clear();
            document.blocks = blocks;
        }
        Context::Section => document.sections[0].blocks = blocks,
        Context::List => document.sections[0].blocks = vec![list(blocks, true)],
        Context::Definition | Context::Nested => {
            let owner = definition(
                head(),
                blocks,
                Some(facts(EntryInlineRoot::Term { index: 0 }, true)),
            );
            document.sections[0].blocks = vec![if matches!(context, Context::Nested) {
                list(vec![owner], false)
            } else {
                owner
            }];
        }
    }
    value
}

pub(super) fn set_relation(value: &mut ResolvedContent, relation: HeadBodyRelation) {
    struct Set(HeadBodyRelation);
    impl visit::VisitMut for Set {
        fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
            if item.entry.is_some() {
                item.head_body_relation = self.0;
            }
            visit::walk_definition_item_mut(self, item);
        }
    }
    visit::VisitMut::visit_document_mut(&mut Set(relation), value.document.as_mut().unwrap());
}

pub(super) fn baseline(value: &ResolvedContent) -> ResolvedContent {
    struct Remove;
    impl visit::VisitMut for Remove {
        fn visit_block_mut(&mut self, block: &mut Block) {
            if let Block::Preformatted {
                children,
                layout,
                source,
                ..
            } = block
                && children.is_empty()
            {
                // A no-row literal has no authored payload. Retire only that
                // contribution, retaining this owner slot and boundary.
                *block = Block::Paragraph {
                    children: vec![],
                    inline_layout: InlineLayout::default(),
                    layout: *layout,
                    source: *source,
                };
            }
            visit::walk_block_mut(self, block);
        }
        fn visit_inline_mut(&mut self, node: &mut Inline) {
            if matches!(node, Inline::Link { children, .. } if children.is_empty()) {
                *node = Inline::Strong { children: vec![] };
            } else {
                visit::walk_inline_mut(self, node);
            }
        }
    }
    let mut baseline = value.clone();
    visit::VisitMut::visit_document_mut(&mut Remove, baseline.document.as_mut().unwrap());
    baseline
}
