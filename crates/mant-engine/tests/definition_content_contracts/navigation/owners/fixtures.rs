//! Source-neutral named-child and hard-tail fixtures.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Child {
    List,
    Definition,
    EmptyDefinition,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Surface {
    Paragraph,
    Literal,
    LiteralFirst,
    NestedList,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Position {
    Prefix,
    Tail,
}

impl Position {
    pub(super) const fn child_index(self) -> usize {
        match self {
            Self::Prefix => 1,
            Self::Tail => 0,
        }
    }

    pub(super) const fn navigation_index(self) -> u32 {
        match self {
            Self::Prefix => 0,
            Self::Tail => 1,
        }
    }
}

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: Some(source(25)),
    }
}

fn ordinary_list(blocks: Vec<Block>, entry: Option<EntryFacts>) -> Block {
    Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            blocks,
            layout: ListItemLayout::default(),
            source: Some(source(25)),
            entry,
        }],
        layout: LayoutHint::default(),
        source: Some(source(25)),
    }
}

fn child_head() -> Vec<Inline> {
    vec![
        Inline::Code {
            value: CHILD.into(),
        },
        Inline::Anchor {
            id: "child-place".into(),
            fragment_aliases: vec!["Child.Place".into()],
            owner_source: Some(source(25)),
        },
    ]
}

fn child_facts(root: EntryInlineRoot) -> EntryFacts {
    EntryFacts {
        id: CHILD_ID.into(),
        kind: EntryKind::Term,
        case: NameCase::Sensitive,
        names: vec![CHILD.into()],
        forms: vec![EntryForm {
            parts: vec![EntryContentSlice {
                root: root.clone(),
                path: vec![],
                bytes: None,
            }],
        }],
        name_bindings: vec![EntryNameBinding {
            name: 0,
            evidence: EntryNameEvidence::Declared,
            occurrences: vec![EntryForm {
                parts: vec![EntryContentSlice {
                    root,
                    path: vec![0],
                    bytes: Some(0..CHILD.len()),
                }],
            }],
        }],
        alias_groups: vec![],
        alias_of: None,
        value_domain: None,
    }
}

fn payload(surface: Surface) -> Block {
    match surface {
        Surface::Paragraph => paragraph(vec![text(PAYLOAD)]),
        Surface::Literal | Surface::LiteralFirst => Block::Preformatted {
            children: vec![text("Payload終\nLiteralEnd")],
            inline_layout: InlineLayout::default(),
            language: Some("txt".into()),
            layout: LayoutHint::default(),
            source: Some(source(26)),
        },
        Surface::NestedList => ordinary_list(vec![paragraph(vec![text(PAYLOAD)])], None),
    }
}

fn child_block(child: Child, surface: Surface) -> Block {
    if matches!(child, Child::List) {
        let blocks = if matches!(surface, Surface::LiteralFirst) {
            let mut children = child_head();
            children.push(text("\nPayload終\nLiteralEnd"));
            vec![Block::Preformatted {
                children,
                inline_layout: InlineLayout::default(),
                language: Some("txt".into()),
                layout: LayoutHint::default(),
                source: Some(source(25)),
            }]
        } else {
            vec![paragraph(child_head()), payload(surface)]
        };
        return ordinary_list(
            blocks,
            Some(child_facts(EntryInlineRoot::Block { index: 0 })),
        );
    }
    let empty_head = matches!(child, Child::EmptyDefinition);
    let (terms, description, root) = if empty_head {
        let mut named_body = child_head();
        named_body.extend([text(" "), text(PAYLOAD)]);
        (
            vec![Vec::<Inline>::new().into()],
            vec![
                Block::VerticalSpace {
                    lines: 1,
                    source: Some(source(24)),
                },
                paragraph(named_body),
            ],
            EntryInlineRoot::Block { index: 1 },
        )
    } else {
        (
            vec![child_head().into()],
            vec![payload(surface)],
            EntryInlineRoot::Term { index: 0 },
        )
    };
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms,
            description,
            head_body_relation: HeadBodyRelation::Separate,
            layout: DefinitionLayout::default(),
            entry: Some(child_facts(root)),
            source: Some(source(25)),
        }],
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: Some(source(25)),
    }
}

pub(super) fn owned_value(
    child: Child,
    surface: Surface,
    position: Position,
    head: Head,
    relation: HeadBodyRelation,
) -> ResolvedContent {
    let mut value = specimen(
        Case {
            navigation: Navigation::External,
            head,
            body: Body::Paragraph,
            relation,
            spacing: 0,
        },
        true,
    );
    let navigation = item(&value).description[0].clone();
    let body = child_block(child, surface);
    owner_mut(&mut value).description = match position {
        Position::Prefix => vec![navigation, body],
        Position::Tail => vec![body, navigation],
    };
    value
}

pub(super) fn child_owner(value: &ResolvedContent, position: Position) -> EntryOwner<'_> {
    item(value).description[position.child_index()]
        .entry_owner()
        .unwrap()
}

pub(super) fn prefix_address(position: Position) -> ContentLocation {
    ContentLocation::Content {
        sections: vec![0],
        blocks: vec![
            ContentBlockStep::Block { index: 0 },
            ContentBlockStep::DefinitionItem { index: 0 },
            ContentBlockStep::Block {
                index: position.navigation_index(),
            },
        ],
        root: ContentInlineRoot::Inlines,
        path: vec![0],
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum BodyTail {
    NoBody,
    EmptyList,
    Literal,
}

pub(super) fn hard_value(
    relation: HeadBodyRelation,
    tail: BodyTail,
    multiple: bool,
) -> ResolvedContent {
    let mut value = specimen(
        Case {
            navigation: Navigation::External,
            head: Head::Hard,
            body: Body::Literal,
            relation,
            spacing: 0,
        },
        true,
    );
    let literal = item(&value).description[2].clone();
    let owner = owner_mut(&mut value);
    owner.description = match tail {
        BodyTail::NoBody => vec![],
        BodyTail::EmptyList => vec![Block::List {
            kind: ListKind::Plain,
            compact: true,
            items: vec![],
            layout: LayoutHint::default(),
            source: Some(source(20)),
        }],
        BodyTail::Literal => vec![literal],
    };
    if multiple {
        owner
            .terms
            .push(vec![text(SECOND_HEAD), Inline::LineBreak {}].into());
    }
    owner.terms.push(
        vec![Inline::Link {
            target: LinkTarget::External {
                uri: EXTERNAL_A.into(),
            },
            title: None,
            children: vec![],
        }]
        .into(),
    );
    value
}
