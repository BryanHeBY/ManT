//! The original index determines a zero-output item's inherited spacing.
use super::*;

#[derive(Clone, Copy, Debug)]
enum Container {
    List,
    Definition,
}

fn blocks(linked: bool) -> Vec<Block> {
    if linked {
        vec![zero(Zero::Link)]
    } else {
        vec![]
    }
}

fn container(kind: Container, compact: bool, explicit: Option<u16>, linked: bool) -> Block {
    match kind {
        Container::List => {
            let item = |blocks, spacing_before_lines| ListItem {
                blocks,
                layout: ListItemLayout {
                    spacing_before_lines,
                },
                source: Some(source(12)),
                entry: None,
            };
            Block::List {
                kind: ListKind::Bullet,
                compact,
                items: vec![
                    item(vec![prose(vec![text(BODY)], 20)], None),
                    item(blocks(linked), explicit),
                ],
                layout: LayoutHint::default(),
                source: Some(source(12)),
            }
        }
        Container::Definition => {
            let item = |description, spacing_before_lines| DefinitionItem {
                terms: vec![vec![].into()],
                description,
                head_body_relation: HeadBodyRelation::Separate,
                layout: DefinitionLayout {
                    spacing_before_lines,
                    ..DefinitionLayout::default()
                },
                source: Some(source(12)),
                entry: None,
            };
            Block::DefinitionList {
                items: vec![
                    item(vec![prose(vec![text(BODY)], 20)], None),
                    item(blocks(linked), explicit),
                ],
                declaration_groups: vec![],
                compact,
                layout: LayoutHint::default(),
                source: Some(source(12)),
            }
        }
    }
}

fn assert_tail(value: &ResolvedContent, positive: bool) {
    struct Lists(Vec<usize>);
    impl<'ir> visit::Visit<'ir> for Lists {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::List { items, .. } = block {
                self.0.push(items.len());
            }
            visit::walk_block(self, block);
        }
    }
    let plain = mant_render::render_query_man(value);
    let body = plain.find(BODY).unwrap() + BODY.len();
    assert_eq!(
        &plain[body..],
        if positive { "\n\n" } else { "" },
        "a second item's one inherited/explicit gap owns one completed EOF row: {plain}"
    );
    assert_eq!(plain.matches(BODY).count(), 1);
    let mut lists = Lists(vec![]);
    visit::Visit::visit_document(&mut lists, value.document.as_ref().unwrap());
    assert_eq!(
        lists.0,
        [1],
        "the zero-body tail cannot fabricate a list item"
    );
    assert_receiver(value, Physical::List);
}

#[test]
fn zero_output_last_items_keep_inherited_or_explicit_spacing_exactly_once() {
    for kind in [Container::List, Container::Definition] {
        for compact in [false, true] {
            for explicit in [None, Some(0), Some(1)] {
                for linked in [false, true] {
                    let original = install(
                        Context::Root,
                        vec![container(kind, compact, explicit, linked)],
                    );
                    let restored = round_trip(&original);
                    let control = round_trip(&baseline(&original));
                    assert_snapshot(&original, &restored);
                    assert_queries(&restored, Context::Root, true, linked);
                    // The empty item remains index 1 in canonical IR. Absence
                    // inherits one row only for noncompact containers; Some(0)
                    // overrides it, rather than being treated as absence.
                    let positive = explicit.map_or(!compact, |rows| rows > 0);
                    for preserve_anchors in [false, true] {
                        let parsed =
                            assert_control(&restored, &control, options(preserve_anchors), linked);
                        assert_tail(&parsed, positive);
                        assert_tail(&import(&control, options(preserve_anchors)), positive);
                    }
                    assert_eq!(restored, original);
                }
            }
        }
    }
}
