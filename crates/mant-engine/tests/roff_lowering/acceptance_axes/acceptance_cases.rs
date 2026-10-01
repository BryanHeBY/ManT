//! Registered acceptance cards for the recorded case set.
//!
//! Every card's row facts are validated against the committed oracle
//! snapshot (see `validate_against_oracle`); semantic facts cite the case
//! source and the typed contracts the guide freezes. Expectations marked
//! `after_repair` describe oracle facts the product does not yet produce;
//! the comparator must currently *detect* those divergences, and the repair
//! unit that closes one flips its status to `must` in the same commit.

use super::axis_model::{
    AcceptanceCase, Axis, AxisKind, AxisPolicy, BlankCountExpect, ContentPolicy, GoldCard,
    HardRowExpect, HardRowRelation, IdentityExpect, IdentityPolicy, IndentPolicy, Owner,
    OwnershipExpect, RowsPolicy, ScalarRangeExpect, SeparatorExpect, SeparatorRelation,
    SourceExpect, StyleExpect, UnitStyle,
};

const NO_EXACT_ROWS: &str = "row text compares through declared row facts; exact row equality is not a reviewed contract for this shape";
const NO_EXACT_ROWS_RESPONSIVE_COLUMNS: &str = "column widths may rearrange responsively; authored hard rows are pinned instead of exact row text";
const NO_EXACT_ROWS_RESPONSIVE_RUN_IN: &str = "run-in head and body origins for this shape are not a reviewed contract; row events are pinned instead of exact row text";
const NO_INTERIOR_BLANKS: &str = "the recorded window has no interior blank rows";
const SINGLE_WORD_REGION: &str = "the region is a single word; unit adjacency carries no signal";
const SINGLE_CONTENT_ROW: &str = "one content row; row membership carries no signal";
const NO_LINKS: &str = "no typed links are authored in this source";
const ALL_WORDS_ACCEPTED: &str =
    "every authored word of this source is accepted on the recorded oracle path";
const SCALAR_PENDING: &str =
    "unit-level scalar spans are pinned together with the ownership repair";
const STYLE_DEVICE_LEVEL: &str =
    "terminal decoration is device-level; the IR retains typed identity instead";
const SOURCE_PENDING: &str =
    "unit-level source attribution is pinned together with the ownership repair";

/// Case names whose oracle gold anchors the six main review examples.
pub(crate) const MAIN_EXAMPLES: [&str; 6] = [
    "word_owner_pending_prefix",
    "rejected_suffix_revival",
    "column_tail_hard_row",
    "tag_explicit_vspace",
    "hang_final_gap",
    "escaped_delimiter_payload",
];

/// Auxiliary case names backing the comparator mutation samples.
pub(crate) const AUXILIARY_EXAMPLES: [&str; 5] = [
    "join_proven_hang",
    "run_in_nonbreaking",
    "column_cell_order",
    "same_uri_occurrences",
    "styled_units",
];

pub(crate) fn case_by_name(name: &str) -> AcceptanceCase {
    match name {
        "word_owner_pending_prefix" => word_owner_pending_prefix(),
        "rejected_suffix_revival" => rejected_suffix_revival(),
        "column_tail_hard_row" => column_tail_hard_row(),
        "tag_explicit_vspace" => tag_explicit_vspace(),
        "hang_final_gap" => hang_final_gap(),
        "escaped_delimiter_payload" => escaped_delimiter_payload(),
        "join_proven_hang" => join_proven_hang(),
        "run_in_nonbreaking" => run_in_nonbreaking(),
        "column_cell_order" => column_cell_order(),
        "same_uri_occurrences" => same_uri_occurrences(),
        "styled_units" => styled_units(),
        other => panic!("unknown acceptance case {other:?}"),
    }
}

fn word_owner_pending_prefix() -> AcceptanceCase {
    AcceptanceCase {
        id: "word_owner_pending_prefix",
        family: "word-acceptance",
        policy: AxisPolicy {
            content: ContentPolicy::Exact,
            rows: RowsPolicy::ConstrainedEvents,
            indent: IndentPolicy::OmitCommonMargin,
            identity: IdentityPolicy::RichInline,
        },
        gold: GoldCard {
            accepted_units: Some(Axis::after_repair(vec!["PY"])),
            forbidden_units: Some(Axis::must(vec!["Z", "AFTER"])),
            row_count: Some(Axis::must(3)),
            identities: Some(Axis::must(vec![IdentityExpect {
                uri: "https://ex.org",
                label: "Y",
            }])),
            ownership: Some(Axis::must(vec![
                OwnershipExpect {
                    unit: "P",
                    owner: Owner::None,
                },
                OwnershipExpect {
                    unit: "Y",
                    owner: Owner::Link(0),
                },
            ])),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::Separator, SINGLE_WORD_REGION),
            (AxisKind::HardRows, SINGLE_CONTENT_ROW),
            (AxisKind::BlankCount, NO_INTERIOR_BLANKS),
            (AxisKind::ExactRows, NO_EXACT_ROWS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn rejected_suffix_revival() -> AcceptanceCase {
    AcceptanceCase {
        id: "rejected_suffix_revival",
        family: "word-acceptance",
        policy: AxisPolicy {
            content: ContentPolicy::Exact,
            rows: RowsPolicy::ConstrainedEvents,
            indent: IndentPolicy::OmitCommonMargin,
            identity: IdentityPolicy::RichInline,
        },
        gold: GoldCard {
            accepted_units: Some(Axis::after_repair(vec!["P"])),
            forbidden_units: Some(Axis::after_repair(vec![":", "https://ex.org", "AFTER"])),
            row_count: Some(Axis::must(3)),
            identities: Some(Axis::after_repair(vec![IdentityExpect {
                uri: "https://ex.org",
                // The fully rejected label keeps its typed identity with a
                // zero-glyph visible range.
                label: "",
            }])),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::Separator, SINGLE_WORD_REGION),
            (AxisKind::HardRows, SINGLE_CONTENT_ROW),
            (AxisKind::BlankCount, NO_INTERIOR_BLANKS),
            (AxisKind::ExactRows, NO_EXACT_ROWS),
            (
                AxisKind::Ownership,
                "no visible unit survives inside the link's visible range",
            ),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn column_tail_hard_row() -> AcceptanceCase {
    AcceptanceCase {
        id: "column_tail_hard_row",
        family: "column-rows",
        policy: AxisPolicy {
            content: ContentPolicy::Exact,
            rows: RowsPolicy::ConstrainedEvents,
            indent: IndentPolicy::Responsive,
            identity: IdentityPolicy::RichInline,
        },
        gold: GoldCard {
            accepted_units: Some(Axis::must(vec!["D", "RightWord"])),
            forbidden_units: Some(Axis::must(vec!["AFTER"])),
            hard_rows: Some(Axis::after_repair(vec![HardRowExpect {
                left: "D",
                right: "RightWord",
                relation: HardRowRelation::DifferentRows,
            }])),
            blank_counts: Some(Axis::must(vec![BlankCountExpect {
                after: "D",
                before: "RightWord",
                count: 0,
            }])),
            row_count: Some(Axis::after_repair(3)),
            identities: Some(Axis::must(vec![])),
            ..GoldCard::none()
        },
        unregistered: &[
            (
                AxisKind::Separator,
                "the cell units sit on separate hard rows; adjacency carries no further signal",
            ),
            (AxisKind::ExactRows, NO_EXACT_ROWS_RESPONSIVE_COLUMNS),
            (AxisKind::Ownership, NO_LINKS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn tag_explicit_vspace() -> AcceptanceCase {
    AcceptanceCase {
        id: "tag_explicit_vspace",
        family: "vertical-spacing",
        policy: AxisPolicy {
            content: ContentPolicy::Exact,
            rows: RowsPolicy::ConstrainedEvents,
            indent: IndentPolicy::Responsive,
            identity: IdentityPolicy::RichInline,
        },
        gold: GoldCard {
            accepted_units: Some(Axis::must(vec!["D", "AFTER", "BodyWord"])),
            hard_rows: Some(Axis::must(vec![
                HardRowExpect {
                    left: "D",
                    right: "AFTER",
                    relation: HardRowRelation::DifferentRows,
                },
                HardRowExpect {
                    left: "AFTER",
                    right: "BodyWord",
                    relation: HardRowRelation::DifferentRows,
                },
            ])),
            // The explicit vertical spacing between D and AFTER is the
            // registered divergence; the AFTER/BodyWord adjacency rides on
            // the same axis and must hold once the repair flips this.
            blank_counts: Some(Axis::after_repair(vec![
                BlankCountExpect {
                    after: "D",
                    before: "AFTER",
                    count: 2,
                },
                BlankCountExpect {
                    after: "AFTER",
                    before: "BodyWord",
                    count: 0,
                },
            ])),
            row_count: Some(Axis::after_repair(6)),
            identities: Some(Axis::must(vec![])),
            ..GoldCard::none()
        },
        unregistered: &[
            (
                AxisKind::Separator,
                "unit pairs are pinned through hard rows and blank counts",
            ),
            (AxisKind::ForbiddenContent, ALL_WORDS_ACCEPTED),
            (AxisKind::ExactRows, NO_EXACT_ROWS_RESPONSIVE_RUN_IN),
            (AxisKind::Ownership, NO_LINKS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn hang_final_gap() -> AcceptanceCase {
    AcceptanceCase {
        id: "hang_final_gap",
        family: "word-separation",
        policy: AxisPolicy {
            content: ContentPolicy::Exact,
            rows: RowsPolicy::ConstrainedEvents,
            indent: IndentPolicy::Responsive,
            identity: IdentityPolicy::RichInline,
        },
        gold: GoldCard {
            accepted_units: Some(Axis::after_repair(vec!["D", "AFTER", "BodyWord"])),
            separators: Some(Axis::after_repair(vec![SeparatorExpect {
                left: "AFTER",
                right: "BodyWord",
                relation: SeparatorRelation::ResponsiveBreak,
            }])),
            hard_rows: Some(Axis::must(vec![HardRowExpect {
                left: "D",
                right: "AFTER",
                relation: HardRowRelation::DifferentRows,
            }])),
            blank_counts: Some(Axis::must(vec![BlankCountExpect {
                after: "D",
                before: "AFTER",
                count: 0,
            }])),
            row_count: Some(Axis::must(3)),
            identities: Some(Axis::must(vec![])),
            sources: Some(Axis::must(vec![SourceExpect {
                unit: "BodyWord",
                line: 14,
            }])),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::ForbiddenContent, ALL_WORDS_ACCEPTED),
            (AxisKind::ExactRows, NO_EXACT_ROWS_RESPONSIVE_RUN_IN),
            (AxisKind::Ownership, NO_LINKS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
        ],
    }
}

fn escaped_delimiter_payload() -> AcceptanceCase {
    AcceptanceCase {
        id: "escaped_delimiter_payload",
        family: "escape-delimiter",
        policy: AxisPolicy {
            content: ContentPolicy::Exact,
            rows: RowsPolicy::ConstrainedEvents,
            indent: IndentPolicy::OmitCommonMargin,
            identity: IdentityPolicy::RichInline,
        },
        gold: GoldCard {
            accepted_units: Some(Axis::after_repair(vec!["Y", "Z", "AFTER"])),
            forbidden_units: Some(Axis::after_repair(vec!["Y(aq"])),
            separators: Some(Axis::must(vec![SeparatorExpect {
                left: "Z",
                right: "AFTER",
                relation: SeparatorRelation::WordBoundary,
            }])),
            hard_rows: Some(Axis::must(vec![HardRowExpect {
                left: "Y",
                right: "Z",
                relation: HardRowRelation::DifferentRows,
            }])),
            row_count: Some(Axis::must(3)),
            identities: Some(Axis::must(vec![])),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::BlankCount, NO_INTERIOR_BLANKS),
            (AxisKind::ExactRows, NO_EXACT_ROWS),
            (AxisKind::Ownership, NO_LINKS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn join_proven_hang() -> AcceptanceCase {
    AcceptanceCase {
        id: "join_proven_hang",
        family: "word-separation",
        policy: auxiliary_policy(),
        gold: GoldCard {
            accepted_units: Some(Axis::must(vec!["D", "E", "AFTERBodyWord"])),
            separators: Some(Axis::must(vec![
                SeparatorExpect {
                    left: "E",
                    right: "AFTERBodyWord",
                    relation: SeparatorRelation::WordBoundary,
                },
                SeparatorExpect {
                    left: "AFTER",
                    right: "BodyWord",
                    // The recorded oracle proves this join: at an 8n hang
                    // width the native run-in glues AFTER to BodyWord.
                    relation: SeparatorRelation::Joined,
                },
            ])),
            row_count: Some(Axis::must(3)),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::ForbiddenContent, ALL_WORDS_ACCEPTED),
            (
                AxisKind::HardRows,
                "row membership carries no signal beyond the row count",
            ),
            (AxisKind::BlankCount, NO_INTERIOR_BLANKS),
            (AxisKind::ExactRows, NO_EXACT_ROWS),
            (AxisKind::Identity, NO_LINKS),
            (AxisKind::Ownership, NO_LINKS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn run_in_nonbreaking() -> AcceptanceCase {
    AcceptanceCase {
        id: "run_in_nonbreaking",
        family: "word-separation",
        policy: auxiliary_policy(),
        gold: GoldCard {
            accepted_units: Some(Axis::must(vec![RUN_IN_JOINED])),
            separators: Some(Axis::must(vec![SeparatorExpect {
                left: "HEAD",
                right: "BodyWord",
                relation: SeparatorRelation::Unbreakable,
            }])),
            row_count: Some(Axis::must(2)),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::ForbiddenContent, ALL_WORDS_ACCEPTED),
            (AxisKind::HardRows, SINGLE_CONTENT_ROW),
            (AxisKind::BlankCount, NO_INTERIOR_BLANKS),
            (AxisKind::ExactRows, NO_EXACT_ROWS),
            (AxisKind::Identity, NO_LINKS),
            (AxisKind::Ownership, NO_LINKS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn column_cell_order() -> AcceptanceCase {
    AcceptanceCase {
        id: "column_cell_order",
        family: "column-rows",
        policy: auxiliary_policy(),
        gold: GoldCard {
            accepted_units: Some(Axis::must(vec!["LEFT", "RIGHT"])),
            hard_rows: Some(Axis::must(vec![HardRowExpect {
                left: "LEFT",
                right: "RIGHT",
                relation: HardRowRelation::SameRow,
            }])),
            row_count: Some(Axis::must(2)),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::ForbiddenContent, ALL_WORDS_ACCEPTED),
            (
                AxisKind::Separator,
                "cell order is pinned by the accepted unit sequence",
            ),
            (AxisKind::BlankCount, NO_INTERIOR_BLANKS),
            (AxisKind::ExactRows, NO_EXACT_ROWS),
            (AxisKind::Identity, NO_LINKS),
            (AxisKind::Ownership, NO_LINKS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn same_uri_occurrences() -> AcceptanceCase {
    AcceptanceCase {
        id: "same_uri_occurrences",
        family: "identity",
        policy: AxisPolicy {
            content: ContentPolicy::Exact,
            rows: RowsPolicy::ConstrainedEvents,
            indent: IndentPolicy::OmitCommonMargin,
            identity: IdentityPolicy::RichInline,
        },
        gold: GoldCard {
            accepted_units: Some(Axis::must(vec![
                "first:",
                "https://ex.org",
                "second:",
                "https://ex.org",
            ])),
            row_count: Some(Axis::must(2)),
            identities: Some(Axis::must(vec![
                IdentityExpect {
                    uri: "https://ex.org",
                    label: "first",
                },
                IdentityExpect {
                    uri: "https://ex.org",
                    label: "second",
                },
            ])),
            scalar_ranges: Some(Axis::must(vec![
                ScalarRangeExpect {
                    owner: Owner::Link(0),
                    start: 5,
                    end: 10,
                },
                ScalarRangeExpect {
                    owner: Owner::Link(1),
                    start: 27,
                    end: 33,
                },
            ])),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::ForbiddenContent, ALL_WORDS_ACCEPTED),
            (
                AxisKind::Separator,
                "label and suffix adjacency is pinned by the accepted unit sequence",
            ),
            (AxisKind::HardRows, SINGLE_CONTENT_ROW),
            (AxisKind::BlankCount, NO_INTERIOR_BLANKS),
            (AxisKind::ExactRows, NO_EXACT_ROWS),
            (
                AxisKind::Ownership,
                "labels are pinned through identity occurrences and scalar ranges",
            ),
            (AxisKind::Style, STYLE_DEVICE_LEVEL),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn styled_units() -> AcceptanceCase {
    AcceptanceCase {
        id: "styled_units",
        family: "style",
        policy: auxiliary_policy(),
        gold: GoldCard {
            accepted_units: Some(Axis::must(vec!["glowing", "rigid", "plain"])),
            row_count: Some(Axis::must(2)),
            styles: Some(Axis::must(vec![
                StyleExpect {
                    unit: "glowing",
                    style: UnitStyle::Emphasis,
                },
                StyleExpect {
                    unit: "rigid",
                    style: UnitStyle::Strong,
                },
                StyleExpect {
                    unit: "plain",
                    style: UnitStyle::Plain,
                },
            ])),
            ..GoldCard::none()
        },
        unregistered: &[
            (AxisKind::ForbiddenContent, ALL_WORDS_ACCEPTED),
            (
                AxisKind::Separator,
                "style units are adjacent words; adjacency carries no signal here",
            ),
            (AxisKind::HardRows, SINGLE_CONTENT_ROW),
            (AxisKind::BlankCount, NO_INTERIOR_BLANKS),
            (AxisKind::ExactRows, NO_EXACT_ROWS),
            (AxisKind::Identity, NO_LINKS),
            (AxisKind::Ownership, NO_LINKS),
            (AxisKind::ScalarRange, SCALAR_PENDING),
            (AxisKind::Source, SOURCE_PENDING),
        ],
    }
}

fn auxiliary_policy() -> AxisPolicy {
    AxisPolicy {
        content: ContentPolicy::Exact,
        rows: RowsPolicy::ConstrainedEvents,
        indent: IndentPolicy::OmitCommonMargin,
        identity: IdentityPolicy::NotApplicable,
    }
}

/// The run-in head and body joined by the device's nonbreaking blanks.
const RUN_IN_JOINED: &str = "HEAD\u{a0}\u{a0}BodyWord";
