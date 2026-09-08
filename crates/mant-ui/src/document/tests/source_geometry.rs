//! Source-derived geometry must agree across the two production frontends.
use super::*;

fn column(text: &str, token: &str) -> usize {
    text.lines()
        .find_map(|line| line.find(token))
        .unwrap_or_else(|| panic!("missing {token}: {text}"))
}

#[test]
fn relative_definition_geometry_matches_text_and_all_normal_viewports() {
    // Original minimal shape of btrfs-subvolume's nested INDENT/TP regions.
    // mandoc CVS HEAD 1.250 and groff 1.24.1 agree on these relative columns.
    let query = mant_engine::query_roff_bytes(b".TH GEOMETRY 1\n.SH SUBCOMMAND\n.RS 0\n.TP\n.B create\nCREATE_BODY\n.sp\nOPTIONS_BODY\n.RS 7\n.TP\n.B -i QGROUP\nINNER_BODY\n.RE\n.TP\n.B delete\nDELETE_BODY\n.RE\n").unwrap();
    let before = query.clone();
    let text = mant_engine::render_query_text(&query);
    let expected = [
        ("create", 0),
        ("CREATE_BODY", 7),
        ("OPTIONS_BODY", 7),
        ("-i QGROUP", 7),
        ("INNER_BODY", 14),
        ("delete", 0),
        ("DELETE_BODY", 7),
    ];
    for (token, origin) in expected {
        assert_eq!(column(&text, token), origin, "{text}");
    }
    let view = DocumentView::new(&query);
    for width in [40, 80, 120] {
        let rendered = view.render(width);
        let text = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        for (token, origin) in expected {
            assert_eq!(column(&text, token), origin + 3, "width {width}: {text}");
            // Case-insensitive search also sees the CREATE_/DELETE_ body
            // witness, independently of the visible label's column.
            assert_eq!(
                rendered.search(token).len(),
                if matches!(token, "create" | "delete") {
                    2
                } else {
                    1
                }
            );
        }
    }
    assert_eq!(query, before);
}

#[test]
fn markdown_semantic_annotation_does_not_change_translated_content_geometry() {
    for loose in [false, true] {
        let separator = if loose { "\n\n" } else { "\n" };
        let plain = format!(
            "# Demo\n\n## Commands\n\n- `run`: First.{separator}- `stop`: Second.\n\n  ```text\n  CODE\n  ```\n"
        );
        let annotated = plain.replace("- `run`", "<!-- mant:entries role=command -->\n- `run`");
        let plain = mant_engine::query_markdown_text(&plain, None).unwrap();
        let annotated = mant_engine::query_markdown_text(&annotated, None).unwrap();
        assert_eq!(
            mant_engine::render_query_text(&plain),
            mant_engine::render_query_text(&annotated)
        );
        for query in [&plain, &annotated] {
            let blocks = &query.document.as_ref().unwrap().sections[0].blocks;
            let mut baseline: Option<Vec<(usize, String)>> = None;
            for shift in [0, 2, 5] {
                let mut builder = DocumentBuilder::new("translation".into(), None);
                builder.blocks(blocks, shift);
                let rows = builder
                    .lines
                    .iter()
                    .map(|line| {
                        (
                            line.indent,
                            line.spans
                                .iter()
                                .map(|span| span.content.as_ref())
                                .collect::<String>(),
                        )
                    })
                    .collect::<Vec<_>>();
                if let Some(original) = &baseline {
                    let expected: Vec<_> = original
                        .iter()
                        .map(|(indent, text): &(usize, String)| {
                            (
                                if text.is_empty() {
                                    *indent
                                } else {
                                    *indent + usize::try_from(shift).unwrap()
                                },
                                text.clone(),
                            )
                        })
                        .collect();
                    assert_eq!(rows, expected);
                } else {
                    baseline = Some(rows);
                }
            }
        }
    }
}
