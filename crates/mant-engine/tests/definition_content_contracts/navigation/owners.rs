//! Relocated navigation keeps its original semantic owner and physical roots.
use super::fixtures::{Body, Case, Head, Navigation, source, specimen};
use super::neighbors::{owner_mut, projection, without_empty_links};
use super::*;

const CHILD: &str = "Child名";
const PAYLOAD: &str = "Payload終";
const CHILD_ID: &str = "child-owner";
const SECOND_HEAD: &str = "SecondHard";

#[path = "owners/assertions.rs"]
mod assertions;
#[path = "owners/fixtures.rs"]
mod fixtures;

use assertions::{
    assert_hard_rows, assert_navigation_address, assert_owned_ranges, assert_owned_rows,
    assert_owner_identity, assert_search_owner,
};
use fixtures::{BodyTail, Child, Position, Surface, hard_value, owned_value, prefix_address};

#[test]
fn ancestor_navigation_does_not_become_child_artifact_or_search_content() {
    for child in [Child::List, Child::Definition, Child::EmptyDefinition] {
        let surfaces = match child {
            Child::List => &[
                Surface::Paragraph,
                Surface::Literal,
                Surface::LiteralFirst,
                Surface::NestedList,
            ][..],
            Child::Definition => &[Surface::Paragraph, Surface::Literal, Surface::NestedList][..],
            Child::EmptyDefinition => &[Surface::Paragraph][..],
        };
        for &surface in surfaces {
            for position in [Position::Prefix, Position::Tail] {
                for head in [Head::Empty, Head::Word] {
                    for relation in [
                        HeadBodyRelation::joined(),
                        HeadBodyRelation::separated(),
                        HeadBodyRelation::Separate,
                    ] {
                        let original = owned_value(child, surface, position, head, relation);
                        let value = round_trip(&original);
                        let baseline = round_trip(&without_empty_links(&original));
                        assert_owner_identity(&value, &baseline, position);
                        assert_navigation_address(&value, &prefix_address(position), 12);
                        assert_search_owner(
                            &value,
                            EXTERNAL_A,
                            "navigation-owner",
                            10,
                            SearchScope::Markdown,
                        );
                        for scope in [SearchScope::Visible, SearchScope::Markdown] {
                            assert_search_owner(&value, CHILD, CHILD_ID, 25, scope);
                        }
                        for preserve_anchors in [false, true] {
                            let options = MarkdownOptions {
                                preserve_anchors,
                                preserve_semantics: false,
                            };
                            let actual = render_markdown_with_options(&value, options);
                            let control = render_markdown_with_options(&baseline, options);
                            let imported = projection(&actual, &control, Navigation::External);
                            assert_owned_rows(&imported, child, surface, head, options);
                            assert_owned_ranges(&value, &baseline, position, options);
                        }
                        assert_eq!(value, original);
                    }
                }
            }
        }
    }
}

#[test]
fn zero_link_term_after_hard_head_keeps_open_tail_and_independent_terms() {
    for relation in [
        HeadBodyRelation::joined(),
        HeadBodyRelation::separated(),
        HeadBodyRelation::Separate,
    ] {
        for tail in [BodyTail::NoBody, BodyTail::EmptyList, BodyTail::Literal] {
            for multiple in [false, true] {
                let original = hard_value(relation, tail, multiple);
                let value = round_trip(&original);
                let baseline = round_trip(&without_empty_links(&original));
                assert_navigation_address(
                    &value,
                    &ContentLocation::Content {
                        sections: vec![0],
                        blocks: vec![ContentBlockStep::Block { index: 0 }],
                        root: ContentInlineRoot::DefinitionTerm {
                            item_index: 0,
                            term_index: if multiple { 2 } else { 1 },
                        },
                        path: vec![0],
                    },
                    10,
                );
                assert_eq!(item(&value).entry, item(&baseline).entry);
                assert_search_owner(
                    &value,
                    EXTERNAL_A,
                    "navigation-owner",
                    10,
                    SearchScope::Markdown,
                );
                for preserve_anchors in [false, true] {
                    let options = MarkdownOptions {
                        preserve_anchors,
                        preserve_semantics: false,
                    };
                    let actual = render_markdown_with_options(&value, options);
                    let control = render_markdown_with_options(&baseline, options);
                    let imported = projection(&actual, &control, Navigation::External);
                    assert_hard_rows(&imported, tail, multiple);
                    assert_eq!(
                        actual.matches("<br>").count(),
                        // Separate literal BODY retains the last term row
                        // which ordinary Paragraph closing would otherwise
                        // consume. The extra delimiter represents that row.
                        1 + usize::from(multiple) + usize::from(matches!(tail, BodyTail::Literal)),
                        "{actual}"
                    );
                    let last_head = if multiple { SECOND_HEAD } else { NAME };
                    assert!(
                        actual.find(EXTERNAL_A).unwrap() < actual.find(last_head).unwrap(),
                        "navigation must precede its occupied Hard row: {actual}"
                    );
                }
                assert_eq!(value, original);
            }
        }
    }
}
