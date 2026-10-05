//! Hard rows are content; source block distance is consumed separately.
//!
//! These are public IR/Markdown contracts. No roff execution expectation is
//! inferred from an exporter, and the pinned native suites remain separate.
use super::{round_trip, text};
use mant_codec::encode::{
    MarkdownFragmentOptions, MarkdownNode, MarkdownOptions,
    render_addressable_markdown_with_options, render_blocks_fragment,
};
use mant_ir::*;
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

#[path = "markdown_seams/assertions.rs"]
mod assertions;
#[path = "markdown_seams/fixtures.rs"]
mod fixtures;
#[path = "markdown_seams/head_tails.rs"]
mod head_tails;
#[path = "markdown_seams/heads.rs"]
mod heads;
#[path = "markdown_seams/seeds.rs"]
mod seeds;
use assertions::*;
use fixtures::*;

const BODY: &str = "Seam中";
const AFTER: &str = "AfterSeam";
const NAME: &str = "SeamHead";
const URI: &str = "https://example.test/seam-empty";
const OWNER: &str = "seam-owner";

#[test]
fn authored_markdown_edge_rows_survive_two_actual_export_import_cycles() {
    for case in seeds::cases() {
        let source = mant_loader::load_markdown_text(&case.markdown(), None).unwrap();
        assert_eq!(reading(&source), case.expected(), "{}", case.name);
        let source = round_trip(&source);
        assert_two_cycles(
            &source,
            &case.expected(),
            &case.name,
            case.leaf,
            case.linked,
        );
    }
}

#[test]
fn standalone_hard_rows_stay_distinct_from_positive_block_distance() {
    let mut failures = vec![];
    for context in Context::ALL {
        for leaf in Leaf::ALL {
            for edge in [Edge::Leading, Edge::Trailing] {
                for hard_rows in 0..=2 {
                    for positive_gap in [false, true] {
                        for navigation in [false, true] {
                            let label = format!(
                                "{context:?}/{leaf:?}/{edge:?}/{hard_rows}/{positive_gap}/{navigation}"
                            );
                            let source = fixture(
                                &edge_content(
                                    context,
                                    leaf,
                                    edge,
                                    hard_rows,
                                    positive_gap,
                                    navigation,
                                ),
                                &label,
                            );
                            assert_original(&source, leaf, navigation);
                            let expected =
                                edge_reading(&source, context, leaf, edge, hard_rows, positive_gap);
                            check_two_cycles(
                                &source,
                                &expected,
                                &label,
                                leaf,
                                navigation,
                                &mut failures,
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} row failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn inline_and_literal_hard_tails_do_not_become_new_block_gap_requests() {
    let mut failures = vec![];
    for context in Context::ALL {
        for literal in [false, true] {
            for hard_rows in 0..=2 {
                for positive_gap in [false, true] {
                    for navigation in [false, true] {
                        let label = format!(
                            "{context:?}/tail/{literal}/{hard_rows}/{positive_gap}/{navigation}"
                        );
                        let source = fixture(
                            &tail_content(context, literal, hard_rows, positive_gap, navigation),
                            &label,
                        );
                        let leaf = if literal { Leaf::Fence } else { Leaf::Plain };
                        assert_original(&source, leaf, navigation);
                        let expected = reference_reading(&source);
                        check_two_cycles(
                            &source,
                            &expected,
                            &label,
                            leaf,
                            navigation,
                            &mut failures,
                        );
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} tail failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn isolated_hard_rows_between_structural_and_phrasing_roots_stay_content() {
    let mut failures = vec![];
    for context in Context::ALL {
        for before in [Leaf::Fence, Leaf::Rule] {
            for after in [Leaf::Plain, Leaf::Fence, Leaf::Rule] {
                for hard_rows in [1, 2] {
                    for positive_gap in [false, true] {
                        for navigation in [false, true] {
                            let label = format!(
                                "{context:?}/middle/{before:?}/{after:?}/{hard_rows}/{positive_gap}/{navigation}"
                            );
                            let source = fixture(
                                &middle_content(
                                    context,
                                    before,
                                    after,
                                    hard_rows,
                                    positive_gap,
                                    navigation,
                                ),
                                &label,
                            );
                            assert_original(&source, before, navigation);
                            let expected = reference_reading(&source);
                            check_two_cycles(
                                &source,
                                &expected,
                                &label,
                                before,
                                navigation,
                                &mut failures,
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} middle-row failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn generated_inline_owner_html_keeps_the_existing_literal_reader_policy() {
    let source = fixture(
        &tail_content(Context::List, false, 0, false, true),
        "inline owner html",
    );
    assert_original(&source, Leaf::Plain, true);
    // This is generated artifact navigation, not authored source content. The
    // locked reader preserves this inline HTML as text. Do not teach the
    // product a broader HTML interpretation to simplify the seam matrix.
    let expected = format!("Probe\n\n• <a id=\"{OWNER}\"></a>{BODY}");
    let mut value = source;
    for cycle in 1..=2 {
        let (parsed, markdown) = next(&value, true);
        assert_eq!(reading(&parsed), expected, "cycle {cycle}: {markdown}");
        assert_targets(&parsed, true);
        value = parsed;
    }
}

#[test]
fn completed_definition_head_rows_survive_the_first_body_seam() {
    let mut failures = vec![];
    for context in [Context::Root, Context::Nested] {
        for leaf in Leaf::ALL {
            for hard_rows in 0..=2 {
                for positive_gap in [false, true] {
                    for navigation in [false, true] {
                        let label = format!(
                            "{context:?}/head/{leaf:?}/{hard_rows}/{positive_gap}/{navigation}"
                        );
                        let value = fixture(
                            &heads::content(context, leaf, hard_rows, positive_gap, navigation),
                            &label,
                        );
                        assert_original(&value, leaf, navigation);
                        assert_header_projection(&value, 1 + usize::from(leaf == Leaf::Definition));
                        let expected =
                            heads::expected(&value, context, leaf, hard_rows, positive_gap);
                        check_two_cycles(
                            &value,
                            &expected,
                            &label,
                            leaf,
                            navigation,
                            &mut failures,
                        );
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn leading_list_distance_and_joined_hard_only_roots_keep_their_rows() {
    let mut failures = vec![];
    for (label, value, expected, leaf) in heads::adjacent_cases() {
        let value = fixture(&value, &label);
        assert_original(&value, leaf, false);
        check_two_cycles(&value, &expected, &label, leaf, false, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn head_only_eof_and_next_root_preserve_completed_rows_without_adding_a_gap() {
    let mut failures = vec![];
    for case in head_tails::cases() {
        let source = fixture(&case.value, &case.label);
        assert_original(&source, Leaf::Rule, false);
        assert_header_projection(&source, 1);
        assert_eq!(reading(&source), case.native, "{} source", case.label);
        check_two_cycles(
            &source,
            &case.portable,
            &case.label,
            case.leaf,
            false,
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn sibling_definition_heads_complete_rows_before_resolved_item_distance() {
    let mut failures = vec![];
    for case in head_tails::siblings() {
        let source = fixture(&case.value, &case.label);
        assert_original(&source, Leaf::Rule, false);
        assert_header_projection(&source, 1);
        assert_eq!(reading(&source), case.native, "{} source", case.label);
        check_two_cycles(
            &source,
            &case.portable,
            &case.label,
            Leaf::Rule,
            false,
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
