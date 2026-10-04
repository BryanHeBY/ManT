//! Navigation syntax cannot add a paragraph or blank row before the real BODY.
//!
//! These are source-neutral IR contracts. Empty link identity belongs to the
//! original prefix; ordinary Markdown may omit Manual wrappers, never BODY.

use super::{NAME, item, round_trip, text};
use mant_codec::encode::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options,
    render_markdown_with_options,
};
use mant_ir::*;
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

const BODY: &str = "Body中";
const TAIL: &str = "OutsideTail";
const EXTERNAL_A: &str = "https://example.test/empty-a";
const EXTERNAL_B: &str = "https://example.test/empty-b";

#[path = "navigation/addresses.rs"]
mod addresses;
#[path = "navigation/carriers.rs"]
mod carriers;
#[path = "navigation/fixtures.rs"]
mod fixtures;
#[path = "navigation/neighbors.rs"]
mod neighbors;
#[path = "navigation/owners.rs"]
mod owners;
#[path = "navigation/readback.rs"]
mod readback;
#[path = "navigation/table_markers.rs"]
mod table_markers;
#[path = "navigation/tails.rs"]
mod tails;

#[test]
fn empty_link_prefixes_preserve_body_types_gaps_identity_and_tail_on_actual_readback() {
    use fixtures::{Body, Case, Head, Navigation, specimen};

    for navigation in [
        Navigation::External,
        Navigation::Manual,
        Navigation::Multiple,
        Navigation::Adjacent,
    ] {
        for body in [
            Body::Paragraph,
            Body::Literal,
            Body::List,
            Body::Ordered,
            Body::Table,
            Body::Equation,
            Body::Thematic,
        ] {
            for head in [Head::Empty, Head::Word, Head::Hard, Head::Anchor] {
                for relation in [
                    HeadBodyRelation::joined(),
                    HeadBodyRelation::separated(),
                    HeadBodyRelation::Separate,
                ] {
                    for spacing in [0, 1] {
                        let case = Case {
                            navigation,
                            head,
                            body,
                            relation,
                            spacing,
                        };
                        let baseline = round_trip(&specimen(case, false));
                        let original = specimen(case, true);
                        let restored = round_trip(&original);
                        addresses::assert_original(&restored, &baseline, case);
                        for preserve_anchors in [false, true] {
                            let options = MarkdownOptions {
                                preserve_anchors,
                                preserve_semantics: false,
                            };
                            let actual = render_markdown_with_options(&restored, options);
                            let expected = render_markdown_with_options(&baseline, options);
                            readback::assert_reader(&actual, &expected, case, options);
                            addresses::assert_artifact(&restored, &baseline, options);
                        }
                        assert_eq!(
                            restored, original,
                            "render/readback cannot mutate canonical owners"
                        );
                    }
                }
            }
        }
    }
}
