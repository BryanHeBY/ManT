//! A skipped first item cannot change the actual next ordered marker.
use super::*;

#[derive(Clone, Copy, Debug)]
enum First {
    Paragraph,
    Link,
    Spacing,
}

fn ordered(start: Option<u64>, first: First) -> Block {
    let first = match first {
        First::Paragraph => zero(Zero::Paragraph),
        First::Link => zero(Zero::Link),
        First::Spacing => spaced_carrier(match start {
            None => Gap::Vertical,
            Some(1) => Gap::Block,
            Some(_) => Gap::Item,
        }),
    };
    let item = |blocks| ListItem {
        blocks,
        layout: ListItemLayout::default(),
        source: Some(source(12)),
        entry: None,
    };
    Block::List {
        kind: ListKind::Ordered { start },
        compact: true,
        items: vec![item(vec![first]), item(vec![prose(vec![text(BODY)], 20)])],
        layout: LayoutHint::default(),
        source: Some(source(12)),
    }
}

fn assert_ordered(value: &ResolvedContent, start: Option<u64>, spacing: bool) {
    struct Ordered(Vec<(ListKind, usize)>);
    impl<'ir> visit::Visit<'ir> for Ordered {
        fn visit_block(&mut self, block: &'ir Block) {
            if let Block::List { kind, items, .. } = block
                && matches!(kind, ListKind::Ordered { .. })
            {
                self.0.push((*kind, items.len()));
            }
            visit::walk_block(self, block);
        }
    }
    let expected = start.unwrap_or(1) + 1;
    let mut found = Ordered(vec![]);
    visit::Visit::visit_document(&mut found, value.document.as_ref().unwrap());
    assert_eq!(
        found.0,
        [(
            ListKind::Ordered {
                start: Some(expected)
            },
            1
        )]
    );
    let plain = mant_render::render_query_man(value);
    assert_eq!(
        plain.matches(&format!("{expected}. {BODY}")).count(),
        1,
        "{plain}"
    );
    assert_eq!(plain.matches(BODY).count(), 1);
    let name = plain.find(NAME).map_or_else(
        || plain.find("Probe").unwrap() + "Probe".len(),
        |start| start + NAME.len(),
    );
    let body = plain.find(&format!("{expected}. {BODY}")).unwrap();
    assert!(
        plain[name..body].matches('\n').count() >= if spacing { 2 } else { 1 },
        "an ordinal other than 1 needs a real list boundary: {plain}"
    );
    assert_receiver(value, Physical::List);
}

#[test]
fn ordered_lists_use_the_first_retained_marker_after_zero_output_items() {
    for context in [Context::Definition, Context::Nested] {
        for start in [None, Some(1), Some(2)] {
            for first in [First::Paragraph, First::Link, First::Spacing] {
                for relation in [
                    HeadBodyRelation::joined(),
                    HeadBodyRelation::separated(),
                    HeadBodyRelation::Separate,
                ] {
                    let mut original = install(context, vec![ordered(start, first)]);
                    set_relation(&mut original, relation);
                    let restored = round_trip(&original);
                    let control = round_trip(&baseline(&original));
                    let linked = !matches!(first, First::Paragraph);
                    assert_snapshot(&original, &restored);
                    assert_queries(&restored, context, true, linked);
                    for preserve_anchors in [false, true] {
                        let parsed =
                            assert_control(&restored, &control, options(preserve_anchors), linked);
                        assert_ordered(&parsed, start, matches!(first, First::Spacing));
                    }
                    assert_eq!(restored, original);
                }
            }
        }
    }
}

#[test]
fn detached_ordered_receivers_keep_positive_spacing_and_their_actual_ordinal() {
    // CommonMark only allows ordinal 1 to interrupt a paragraph. A detached
    // blank row is phrasing, so an actual 2/3 marker needs its own syntax blank
    // after that row. Numeric source distance simplifies to this minimum
    // grammar; navigation cannot add a further gap or turn it into prose.
    for start in [None, Some(1), Some(2)] {
        let original = install(Context::Root, vec![ordered(start, First::Spacing)]);
        let restored = round_trip(&original);
        let control = round_trip(&baseline(&original));
        assert_snapshot(&original, &restored);
        assert_queries(&restored, Context::Root, true, true);
        for preserve_anchors in [false, true] {
            let parsed = assert_control(&restored, &control, options(preserve_anchors), true);
            assert_ordered(&parsed, start, true);
            for value in [&restored, &control] {
                let isolated = import_root_fragment(value, preserve_anchors);
                assert_ordered(&isolated, start, true);
                let plain = mant_render::render_query_man(&isolated);
                let marker = plain
                    .find(&format!("{}. {BODY}", start.unwrap_or(1) + 1))
                    .unwrap();
                assert_eq!(&plain["Probe".len()..marker], "\n\n\n\n");
            }
        }
        assert_eq!(restored, original);
    }
}
