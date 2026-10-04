//! Accepted IR contributions retain rows, boundaries, navigation and owners.
use super::{NAME, round_trip, text};
use mant_codec::encode::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options,
    render_markdown_with_options,
};
use mant_ir::*;
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

const BODY: &str = "Contribution中";
const BEFORE: &str = "BeforeWord";
const AFTER: &str = "AfterWord";
const URI: &str = "https://example.test/contribution-empty";
const OWNER: &str = "contribution-owner";

#[path = "contribution_contracts/assertions.rs"]
mod assertions;
#[path = "contribution_contracts/boundary_contracts.rs"]
mod boundary_contracts;
#[path = "contribution_contracts/fixtures.rs"]
mod fixtures;
use assertions::*;
use fixtures::*;

#[test]
fn zero_line_and_authored_empty_literal_contributions_remain_distinct() {
    for context in [
        Context::Root,
        Context::Section,
        Context::List,
        Context::Definition,
        Context::Nested,
    ] {
        for zero in [
            Zero::Paragraph,
            Zero::Link,
            Zero::LiteralNone,
            Zero::LiteralEmpty,
            Zero::NestedCarrier,
        ] {
            for position in [
                Position::Prefix,
                Position::Middle,
                Position::Tail,
                Position::TailFollow,
            ] {
                let original = install(context, sequence(zero, position));
                let restored = round_trip(&original);
                let control = round_trip(&baseline(&original));
                assert_snapshot(&original, &restored);
                assert_queries(
                    &restored,
                    context,
                    true,
                    matches!(zero, Zero::Link | Zero::NestedCarrier),
                );
                for preserve_anchors in [false, true] {
                    let options = MarkdownOptions {
                        preserve_anchors,
                        preserve_semantics: false,
                    };
                    let parsed = assert_control(
                        &restored,
                        &control,
                        options,
                        matches!(zero, Zero::Link | Zero::NestedCarrier),
                    );
                    assert_types(
                        &parsed,
                        Physical::Paragraph,
                        usize::from(matches!(zero, Zero::LiteralEmpty)),
                    );
                    let plain = mant_render::render_query_man(&parsed);
                    assert_eq!(
                        plain.matches(BEFORE).count(),
                        usize::from(!matches!(position, Position::Prefix))
                    );
                    assert_eq!(
                        plain.matches(AFTER).count(),
                        usize::from(matches!(
                            position,
                            Position::Prefix | Position::Middle | Position::TailFollow
                        ))
                    );
                }
                assert_eq!(restored, original);
            }
        }
    }
}

#[test]
fn positive_boundaries_inside_zero_body_carriers_reach_the_next_physical_block() {
    for context in [Context::Definition, Context::Nested] {
        for gap in [Gap::Vertical, Gap::Block, Gap::Item] {
            for physical in [
                Physical::Paragraph,
                Physical::Opaque,
                Physical::InlineEquation,
                Physical::Fence,
                Physical::List,
                Physical::Rule,
            ] {
                let mut blocks = vec![spaced_carrier(gap)];
                blocks.extend(physical.blocks());
                let original = install(context, blocks);
                let restored = round_trip(&original);
                let control = round_trip(&baseline(&original));
                assert_snapshot(&original, &restored);
                assert_queries(&restored, context, true, true);
                for preserve_anchors in [false, true] {
                    let options = MarkdownOptions {
                        preserve_anchors,
                        preserve_semantics: false,
                    };
                    let parsed = assert_control(&restored, &control, options, true);
                    assert_types(&parsed, physical, 0);
                    assert_positive_gap(&parsed, physical);
                }
                assert_eq!(restored, original);
            }
        }
    }
}

#[test]
fn separate_opaque_rows_and_thematic_rules_keep_their_physical_block_boundaries() {
    for context in [Context::Definition, Context::Nested] {
        for relation in [
            HeadBodyRelation::joined(),
            HeadBodyRelation::separated(),
            HeadBodyRelation::Separate,
        ] {
            for physical in [
                Physical::Paragraph,
                Physical::Opaque,
                Physical::InlineEquation,
                Physical::Rule,
            ] {
                let mut blocks = vec![zero(Zero::Link)];
                blocks.extend(physical.blocks());
                let mut original = install(context, blocks);
                set_relation(&mut original, relation);
                let restored = round_trip(&original);
                let control = round_trip(&baseline(&original));
                assert_snapshot(&original, &restored);
                assert_queries(&restored, context, true, true);
                for preserve_anchors in [false, true] {
                    let options = MarkdownOptions {
                        preserve_anchors,
                        preserve_semantics: false,
                    };
                    let parsed = assert_control(&restored, &control, options, true);
                    assert_types(&parsed, physical, 0);
                    assert_boundary(&parsed, physical, relation);
                }
                assert_eq!(restored, original);
            }
        }
    }
}

#[test]
fn root_and_section_rules_do_not_become_setext_headings_beside_navigation() {
    for context in [Context::Root, Context::Section] {
        for prefix in [false, true] {
            for following in [false, true] {
                let carrier = named_navigation();
                let rule = Block::ThematicBreak {
                    source: Some(source(20)),
                };
                let mut blocks = if prefix {
                    vec![carrier, rule]
                } else {
                    vec![rule, carrier]
                };
                if following {
                    blocks.push(prose(vec![text(BODY)], 21));
                }
                let original = install(context, blocks);
                let restored = round_trip(&original);
                let control = round_trip(&baseline(&original));
                assert_snapshot(&original, &restored);
                assert_uri_owner(&restored);
                if following {
                    assert_queries(&restored, context, true, false);
                }
                for preserve_anchors in [false, true] {
                    let options = MarkdownOptions {
                        preserve_anchors,
                        preserve_semantics: false,
                    };
                    let parsed = assert_control(&restored, &control, options, true);
                    assert_rule(&parsed);
                    let plain = mant_render::render_query_man(&parsed);
                    assert_eq!(plain.matches(BODY).count(), usize::from(following));
                }
                assert_eq!(restored, original);
            }
        }
    }
}
