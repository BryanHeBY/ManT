//! Gap-only containers contribute boundaries independently of navigation.
use super::*;

#[path = "boundary_contracts/assertions.rs"]
mod assertions;
#[path = "boundary_contracts/edge_gaps.rs"]
mod edge_gaps;
#[path = "boundary_contracts/fixtures.rs"]
mod fixtures;
#[path = "boundary_contracts/ordered_receivers.rs"]
mod ordered_receivers;
use assertions::*;
use fixtures::*;

const CONTEXTS: [Context; 5] = [
    Context::Root,
    Context::Section,
    Context::List,
    Context::Definition,
    Context::Nested,
];
const GAPS: [Gap; 3] = [Gap::Vertical, Gap::Block, Gap::Item];
const RECEIVERS: [Physical; 4] = [
    Physical::Paragraph,
    Physical::Fence,
    Physical::List,
    Physical::Rule,
];

#[test]
fn gap_only_carriers_keep_boundaries_before_between_and_after_real_blocks() {
    for context in CONTEXTS {
        for gap in GAPS {
            for position in [Position::Prefix, Position::Middle, Position::Tail] {
                for physical in RECEIVERS {
                    let original = install(context, gap_sequence(gap, position, physical));
                    let restored = round_trip(&original);
                    let control = round_trip(&baseline(&original));
                    for (value, linked) in [(&restored, true), (&control, false)] {
                        assert_snapshot(value, value);
                        assert_queries(value, context, true, linked);
                    }
                    for preserve_anchors in [false, true] {
                        let parsed =
                            assert_control(&restored, &control, options(preserve_anchors), true);
                        let unlinked = import(&control, options(preserve_anchors));
                        for value in [&parsed, &unlinked] {
                            assert_receiver(value, physical);
                            assert_gap(value, context, position, physical);
                        }
                    }
                    assert_eq!(restored, original);
                }
            }
        }
    }
}

#[test]
fn containers_containing_only_spacing_keep_a_row_without_fabricating_body_syntax() {
    for context in CONTEXTS {
        for gap in GAPS {
            let original = only_gap(context, gap);
            let restored = round_trip(&original);
            let control = round_trip(&baseline(&original));
            for value in [&restored, &control] {
                assert_snapshot(value, value);
            }
            let query_context = if matches!(context, Context::Root) {
                Context::Root
            } else {
                Context::Section
            };
            assert_queries(&restored, query_context, false, true);
            for preserve_anchors in [false, true] {
                let parsed = assert_control(&restored, &control, options(preserve_anchors), true);
                let unlinked = import(&control, options(preserve_anchors));
                if matches!(context, Context::Root) && preserve_anchors {
                    for value in [&restored, &control] {
                        assert_only_spacing(
                            &import_root_fragment(value, preserve_anchors),
                            context,
                        );
                    }
                } else {
                    for value in [&parsed, &unlinked] {
                        assert_only_spacing(value, context);
                    }
                }
            }
            assert_eq!(restored, original);
        }
    }
}

#[test]
fn one_root_gap_before_prose_and_at_eof_is_consumed_exactly_once() {
    // Page furniture contributes one fixed blank row. The accepted gap
    // contributes one more row before BODY, or one completed row at EOF.
    // A generated hard-row spelling and Markdown's paragraph distance must
    // not each consume the same boundary.
    for gap in GAPS {
        for prefix in [false, true] {
            let original = install(Context::Root, root_gap(gap, prefix));
            let restored = round_trip(&original);
            let control = round_trip(&baseline(&original));
            for value in [&restored, &control] {
                assert_root_gap_once(value, prefix);
            }
            for preserve_anchors in [false, true] {
                let parsed = assert_control(&restored, &control, options(preserve_anchors), true);
                let unlinked = import(&control, options(preserve_anchors));
                if preserve_anchors {
                    for value in [&restored, &control] {
                        assert_root_gap_once(
                            &import_root_fragment(value, preserve_anchors),
                            prefix,
                        );
                    }
                } else {
                    for value in [&parsed, &unlinked] {
                        assert_root_gap_once(value, prefix);
                    }
                }
            }
            assert_eq!(restored, original);
        }
    }
}

#[test]
fn authored_empty_table_rows_have_a_fence_receiver_while_no_rows_do_not() {
    for context in [Context::Root, Context::Definition] {
        for shape in [
            EmptyTable::NoRows,
            EmptyTable::NoCells,
            EmptyTable::EmptyBlocks,
            EmptyTable::EmptyLiteral,
        ] {
            for break_after in [false, true] {
                let original = install(
                    context,
                    vec![empty_table(shape, break_after), prose(vec![text(BODY)], 20)],
                );
                let restored = round_trip(&original);
                assert_snapshot(&original, &restored);
                assert_queries(&restored, context, true, false);
                for preserve_anchors in [false, true] {
                    let parsed =
                        assert_control(&restored, &restored, options(preserve_anchors), false);
                    assert_empty_table(&parsed, shape);
                }
                assert_eq!(restored, original);
            }
        }
    }
}

#[test]
fn completed_eof_spacing_preserves_authored_hard_tails_through_inline_wrappers() {
    for context in [
        Context::Root,
        Context::List,
        Context::Definition,
        Context::Nested,
    ] {
        for hard_tail in 0..=2 {
            for style in 0..3 {
                let leaf = match style {
                    0 => text(BODY),
                    1 => Inline::Strong {
                        children: vec![text(BODY)],
                    },
                    _ => Inline::Code { value: BODY.into() },
                };
                let mut children = vec![leaf];
                children.extend(std::iter::repeat_with(Inline::line_break).take(hard_tail));
                let original = install(
                    context,
                    vec![prose(children, 20), spaced_carrier(Gap::Vertical)],
                );
                let restored = round_trip(&original);
                assert_snapshot(&original, &restored);
                assert_queries(&restored, context, true, true);
                // The independent reading consumer already owns the accepted
                // hard rows. Markdown must not settle an open tail a second
                // time or erase one of those rows when framing the EOF gap.
                let native = mant_render::render_query_man(&restored);
                let end = native.find(BODY).unwrap() + BODY.len();
                let expected = &native[end..];
                assert!(expected.chars().all(|character| character == '\n'));
                assert!(expected.len() >= 2);
                for preserve_anchors in [false, true] {
                    let markdown =
                        render_markdown_with_options(&restored, options(preserve_anchors));
                    let parsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
                    let plain = mant_render::render_query_man(&parsed);
                    let end = plain.find(BODY).unwrap() + BODY.len();
                    assert_eq!(
                        &plain[end..],
                        expected,
                        "{context:?}/{hard_tail}/{style}: {markdown}"
                    );
                    assert_eq!(plain.matches(BODY).count(), 1);
                }
                assert_eq!(restored, original);
            }
        }
    }
}
