//! Public loading and transport consume resolved origins and physical rows.

#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/scoped_layout_cases.rs"]
mod recorded;

fn assert_search(content: &mant_ir::ResolvedContent, case: &recorded::Case) {
    for label in case.labels() {
        let found = mant_query::search_query(
            content,
            &mant_protocol::SearchQuery {
                pattern: (*label).into(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 20,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(found.total, 1, "{}: original visible owner", case.id);
        assert_eq!(found.matches[0].occurrences[0].matched_text, *label);
    }
}

#[test]
fn native_scope_rows_reach_cli_json_search_and_two_actual_markdown_readers() {
    // Each sealed complete source first ran five pristine profiles. In pinned
    // mdoc_term.c print_mdoc_node restores each non-TEXT node's offset, It BODY
    // pre reinstalls NOBREAK, and its actual post closes the row before Pp.
    for case in recorded::cases() {
        let original = case.load();
        assert_eq!(
            recorded::cli_body(&original),
            case.utf8_rows.join("\n"),
            "{}",
            case.id
        );
        let mut content = json_roundtrip::json_string_roundtrip(&original);
        assert_eq!(
            recorded::cli_body(&content),
            case.utf8_rows.join("\n"),
            "{}",
            case.id
        );
        assert_search(&content, &case);
        for _ in 0..2 {
            content = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(
                    &mant_codec::encode::render_markdown(&content),
                    None,
                )
                .unwrap(),
            );
            case.assert_portable(&content);
            assert_search(&content, &case);
        }
    }
}
