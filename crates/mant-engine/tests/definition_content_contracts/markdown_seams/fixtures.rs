use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Context {
    Root,
    Section,
    List,
    Definition,
    Nested,
}

impl Context {
    pub(super) const ALL: [Self; 5] = [
        Self::Root,
        Self::Section,
        Self::List,
        Self::Definition,
        Self::Nested,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Leaf {
    Plain,
    Fence,
    Rule,
    List,
    Ordered,
    Definition,
}

impl Leaf {
    pub(super) const ALL: [Self; 6] = [
        Self::Plain,
        Self::Fence,
        Self::Rule,
        Self::List,
        Self::Ordered,
        Self::Definition,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edge {
    Leading,
    Trailing,
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

pub(super) fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: Some(source(12)),
    }
}

pub(super) fn gap() -> Block {
    Block::VerticalSpace {
        lines: 1,
        source: Some(source(13)),
    }
}

pub(super) fn navigation() -> Block {
    paragraph(vec![Inline::Link {
        target: LinkTarget::External { uri: URI.into() },
        title: None,
        children: vec![],
    }])
}

pub(super) fn literal(children: Vec<Inline>) -> Block {
    Block::Preformatted {
        children,
        inline_layout: InlineLayout::default(),
        language: Some("seam".into()),
        layout: LayoutHint::default(),
        source: Some(source(12)),
    }
}

pub(super) fn facts(root: EntryInlineRoot) -> EntryFacts {
    // An unnamed owner still has a stable original form/root. It must not
    // need a generated display label to keep its children addressable.
    EntryFacts {
        id: OWNER.into(),
        kind: EntryKind::Term,
        case: NameCase::Sensitive,
        names: vec![],
        forms: vec![EntryForm {
            parts: vec![EntryContentSlice {
                root,
                path: vec![],
                bytes: None,
            }],
        }],
        name_bindings: vec![],
        alias_groups: vec![],
        alias_of: None,
        value_domain: None,
    }
}

pub(super) fn list(kind: ListKind, blocks: Vec<Block>, entry: bool) -> Block {
    let form = blocks
        .iter()
        .position(|block| matches!(block, Block::Paragraph { .. } | Block::Preformatted { .. }));
    Block::List {
        kind,
        compact: true,
        items: vec![ListItem {
            blocks,
            layout: ListItemLayout::default(),
            source: Some(source(10)),
            entry: entry.then(|| {
                let mut value = facts(EntryInlineRoot::Block {
                    index: form.unwrap_or(0),
                });
                if form.is_none() {
                    // A structural-only item still owns its addressable
                    // content, but does not claim a nonexistent inline form.
                    value.forms.clear();
                }
                value
            }),
        }],
        layout: LayoutHint::default(),
        source: Some(source(10)),
    }
}

fn definition(named: bool, blocks: Vec<Block>, entry: bool) -> Block {
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![if named { vec![text(NAME)] } else { vec![] }.into()],
            description: blocks,
            head_body_relation: HeadBodyRelation::Separate,
            layout: DefinitionLayout::default(),
            source: Some(source(10)),
            entry: entry.then(|| facts(EntryInlineRoot::Term { index: 0 })),
        }],
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: Some(source(10)),
    }
}

pub(super) fn leaf(kind: Leaf) -> Block {
    leaf_text(kind, BODY)
}

fn leaf_text(kind: Leaf, value: &str) -> Block {
    let body = paragraph(vec![text(value)]);
    match kind {
        Leaf::Plain => body,
        Leaf::Fence => literal(vec![text(value)]),
        Leaf::Rule => Block::ThematicBreak {
            source: Some(source(12)),
        },
        Leaf::List => list(ListKind::Bullet, vec![body], false),
        Leaf::Ordered => list(ListKind::Ordered { start: Some(2) }, vec![body], false),
        Leaf::Definition => definition(true, vec![body], false),
    }
}

pub(super) fn install(context: Context, mut blocks: Vec<Block>) -> ResolvedContent {
    match context {
        Context::List => blocks = vec![list(ListKind::Bullet, blocks, true)],
        Context::Definition => blocks = vec![definition(false, blocks, true)],
        Context::Nested => {
            blocks = vec![list(
                ListKind::Bullet,
                vec![definition(false, blocks, true)],
                false,
            )];
        }
        Context::Root | Context::Section => {}
    }
    let mut value = mant_loader::load_markdown_text("# Probe\n\n## OPTIONS\n", None).unwrap();
    let document = value.document.as_mut().unwrap();
    if matches!(context, Context::Root) {
        document.sections.clear();
        document.blocks = blocks;
    } else {
        document.sections[0].blocks = blocks;
    }
    value
}

pub(super) fn edge_content(
    context: Context,
    kind: Leaf,
    edge: Edge,
    hard_rows: usize,
    positive_gap: bool,
    linked: bool,
) -> ResolvedContent {
    // Zero authored rows is the absence of a hard-row root. An otherwise
    // empty paragraph is a distinct fixture, covered by contribution tests.
    let mut boundary = vec![];
    if hard_rows > 0 {
        boundary.push(paragraph(
            std::iter::repeat_with(Inline::line_break)
                .take(hard_rows)
                .collect(),
        ));
    }
    if linked {
        boundary.push(navigation());
    }
    if positive_gap {
        boundary.push(gap());
    }
    let mut blocks = vec![];
    if edge == Edge::Trailing {
        blocks.push(leaf(kind));
    }
    blocks.extend(boundary);
    if edge == Edge::Leading {
        blocks.push(leaf(kind));
    }
    install(context, blocks)
}

pub(super) fn tail_content(
    context: Context,
    literal_tail: bool,
    hard_rows: usize,
    positive_gap: bool,
    linked: bool,
) -> ResolvedContent {
    // Hard rows are inside the original content owner. They must not be
    // reinterpreted as the distance of a separately imported marker root.
    let mut children = vec![Inline::Strong {
        children: vec![text(BODY)],
    }];
    children.extend(std::iter::repeat_with(Inline::line_break).take(hard_rows));
    let mut blocks = vec![if literal_tail {
        literal(children)
    } else {
        paragraph(children)
    }];
    if linked {
        blocks.push(navigation());
    }
    if positive_gap {
        blocks.push(gap());
    }
    install(context, blocks)
}

pub(super) fn middle_content(
    context: Context,
    before: Leaf,
    after: Leaf,
    hard_rows: usize,
    positive_gap: bool,
    linked: bool,
) -> ResolvedContent {
    let hard = paragraph(
        std::iter::repeat_with(Inline::line_break)
            .take(hard_rows)
            .collect(),
    );
    let mut blocks = vec![leaf(before), hard];
    if linked {
        blocks.push(navigation());
    }
    if positive_gap {
        blocks.push(gap());
    }
    blocks.push(leaf_text(after, AFTER));
    install(context, blocks)
}

pub(super) fn scope_blocks(value: &ResolvedContent) -> &[Block] {
    let document = value.document.as_ref().unwrap();
    if document.sections.is_empty() {
        &document.blocks
    } else {
        &document.sections[0].blocks
    }
}

pub(super) fn reference_reading(value: &ResolvedContent) -> String {
    // Portable Markdown spells a Definition as a bullet item. Build that
    // documented, finite projection explicitly before using the independent
    // reading Flow, rather than obtaining an expectation from the encoder.
    struct Portable;
    impl visit::VisitMut for Portable {
        fn visit_list_item_mut(&mut self, item: &mut ListItem) {
            visit::walk_list_item_mut(self, item);
            // Navigation-only roots are attached to adjacent content by the
            // portable spelling. Their target identity is asserted separately;
            // they do not supply an extra bare reading marker row.
            item.blocks.retain(|block| {
                !matches!(
                    block,
                    Block::Paragraph { children, .. }
                        if first_visible_character(children).is_none()
                )
            });
        }

        fn visit_block_mut(&mut self, block: &mut Block) {
            visit::walk_block_mut(self, block);
            let Block::DefinitionList {
                items,
                compact,
                layout,
                source,
                ..
            } = block
            else {
                return;
            };
            let items = items
                .iter()
                .map(|item| {
                    let mut blocks = item
                        .terms
                        .iter()
                        .filter(|term| first_visible_character(&term.content).is_some())
                        .map(|term| {
                            let mut content = term.content.clone();
                            if last_visible_character(&content) == Some('\n') {
                                // Separate Definition terms own their last logical
                                // row. Paragraph closes one provisional tail; add
                                // its delimiter so this finite portable model does
                                // not silently drop that completed term row.
                                content.push(Inline::line_break());
                            }
                            paragraph(content)
                        })
                        .collect::<Vec<_>>();
                    blocks.extend(item.description.clone());
                    blocks.retain(|block| {
                        !matches!(
                            block,
                            Block::Paragraph { children, .. }
                                if first_visible_character(children).is_none()
                        )
                    });
                    ListItem {
                        blocks,
                        layout: ListItemLayout {
                            spacing_before_lines: item.layout.spacing_before_lines,
                        },
                        source: item.source,
                        entry: None,
                    }
                })
                .collect();
            *block = Block::List {
                kind: ListKind::Bullet,
                compact: *compact,
                items,
                layout: *layout,
                source: *source,
            };
        }
    }
    let mut reference = detached(value);
    visit::VisitMut::visit_document_mut(&mut Portable, reference.document.as_mut().unwrap());
    mant_render::render_query_man(&reference)
}

pub(super) fn detached(value: &ResolvedContent) -> ResolvedContent {
    let mut result = mant_loader::load_markdown_text("# Probe\n", None).unwrap();
    result.document.as_mut().unwrap().blocks = scope_blocks(value).to_vec();
    result
}

pub(super) fn reading(value: &ResolvedContent) -> String {
    mant_render::render_query_man(&detached(value))
}

pub(super) fn edge_reading(
    value: &ResolvedContent,
    context: Context,
    leaf: Leaf,
    edge: Edge,
    hard_rows: usize,
    positive_gap: bool,
) -> String {
    let mut expected = reference_reading(value);
    if leaf == Leaf::Ordered
        && edge == Edge::Leading
        && ((hard_rows > 0 && !positive_gap)
            || (hard_rows == 0
                && positive_gap
                && matches!(context, Context::Root | Context::Section)))
    {
        // A non-1 marker cannot interrupt a preceding empty phrasing root.
        // An actual source gap after an authored hard root supplies that
        // required separator already. A generated detached gap row alone
        // still needs its minimum grammar separator (documented export policy).
        let marker = expected.find("2. ").unwrap();
        let row_start = expected[..marker].rfind('\n').unwrap() + 1;
        expected.insert(row_start, '\n');
    }
    if edge == Edge::Leading && hard_rows == 0 && positive_gap {
        let marker = match context {
            Context::List | Context::Definition => Some("Probe\n\n•"),
            Context::Nested => Some("Probe\n\n•\n  •"),
            Context::Root | Context::Section => None,
        };
        if let Some(marker) = marker {
            // The portable framing paragraph has a generated marker gap.
            // Model that one exact cell, keeping authored spaces untouched.
            assert!(expected.starts_with(&format!("{marker}\n")));
            expected.insert(marker.len(), ' ');
        }
    }
    expected
}
