//! Table safe-reading cells and owner bytes share the real export boundary.

use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown,
};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

#[path = "../support/table_content_roles.rs"]
mod roles;
use crate::json_roundtrip;

#[test]
fn table_entity_text_keeps_json_scalars_and_safe_cells_after_two_reads() {
    let mut configurations = 0;
    for case in roles::CASES {
        for linked in [false, true] {
            let original = case.load(linked);
            let mut content = json_roundtrip::json_string_roundtrip(&original);
            for round in 0..=2 {
                if round > 0 {
                    case.assert_portable(&content);
                }
                let text = mant_render::render_query_text_at_width(&content, 40, |_, text| {
                    text.split('\n')
                        .map(mant_render::sanitize_terminal_text)
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                assert_eq!(text, format!("Table\n\n{}", case.reading));
                let artifact =
                    render_addressable_markdown_with_options(&content, MarkdownOptions::default());
                assert_eq!(artifact.text(), render_markdown(&content));
                assert!(!artifact.text().contains('\r'));
                for row in case.rows {
                    let found = mant_query::search_query(
                        &content,
                        &SearchQuery {
                            pattern: (*row).into(),
                            syntax: SearchSyntax::Literal,
                            case: SearchCase::Sensitive,
                            scope: SearchScope::Visible,
                            word: false,
                            context_lines: 0,
                            limit: 20,
                            offset: 0,
                        },
                    )
                    .unwrap();
                    assert_eq!(found.total, 1, "{}/{linked}/{round}: {row:?}", case.id);
                    assert_eq!(found.matches[0].occurrences[0].matched_text, *row);
                }
                if round < 2 {
                    content = json_roundtrip::json_string_roundtrip(
                        &mant_loader::load_markdown_text(artifact.text(), None).unwrap(),
                    );
                }
            }
            assert_eq!(
                original,
                case.load(linked),
                "original JSON owner is unchanged"
            );
            configurations += 1;
        }
    }
    assert_eq!(configurations, 10);
}
