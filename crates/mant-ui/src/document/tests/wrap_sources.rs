//! Viewport wrapping preserves authored source identity, not generated paint.

use super::*;
use serde_json::{Value, json};

fn source_block(value: &str, style: usize, shape: usize) -> Block {
    let text = json!({"type":"text", "value":value});
    let children = match style {
        0 => vec![text],
        1 => vec![json!({"type":"strong", "children":[text]})],
        2 => vec![
            json!({"type":"link", "target":{"kind":"external", "uri":"https://example.test"}, "children":[text]}),
        ],
        _ => {
            let split = value
                .char_indices()
                .nth(value.chars().count() / 2)
                .map_or(value.len(), |(at, _)| at);
            vec![
                json!({"type":"strong", "children":[{"type":"text", "value":&value[..split]}]}),
                json!({"type":"link", "target":{"kind":"external", "uri":"https://example.test"}, "children":[{"type":"text", "value":&value[split..]}]}),
            ]
        }
    };
    let paragraph = json!({"type":"paragraph", "children":children});
    let table = |body: Value| json!({"type":"table", "rows":[{"cells":[{"blocks":[body]}]}]});
    serde_json::from_value(match shape {
        0 => paragraph,
        1 => json!({"type":"preformatted", "children":children}),
        2 => json!({"type":"definition-list", "compact":true, "items":[{"terms":[{"content":children}], "description":[]}]}),
        3 => json!({"type":"definition-list", "compact":true, "items":[{"terms":[], "description":[paragraph]}]}),
        4 => table(paragraph),
        5 => table(table(paragraph)),
        _ => json!({"type":"list", "kind":{"kind":"plain"}, "compact":true, "items":[{"blocks":[paragraph]}]}),
    }).unwrap()
}

fn content(block: Block) -> ResolvedContent {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![block];
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
    serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into()
}

#[test]
fn source_whitespace_search_is_stable_across_owners_styles_and_resizes() {
    for value in [
        "  abc def",
        "abc  ",
        "        abc    def        ",
        "        ",
        "a  b",
        "a\tb",
        "\ta\t",
        "\t",
        "a\u{a0}b",
        "a\u{2007}b",
        "a\u{202f}b",
        "中 e\u{301} 👩‍💻  Z",
    ] {
        for style in 0..4 {
            for shape in 0..7 {
                let original = content(source_block(value, style, shape));
                let view = DocumentView::new(&original);
                for width in [80, 1, 2, 3, 4, 7, 8, 9, 20, 80] {
                    let rendered = view.render(width);
                    assert_eq!(
                        rendered.search(value).len(),
                        1,
                        "{value:?} style={style} shape={shape} width={width}: {:?}",
                        rendered.search_records
                    );
                    assert!(
                        rendered
                            .search_records
                            .iter()
                            .any(|record| record.text == value),
                        "logical source must not acquire layout spaces: {value:?} shape={shape} width={width}"
                    );
                    assert_eq!(
                        rendered.search(&format!(" {value} ")).len(),
                        0,
                        "generated padding must not enter search"
                    );
                    for link in &rendered.links {
                        assert!(link.end_column > link.start_column);
                        assert!(link.end_column <= usize::from(width));
                    }
                }
                assert_eq!(
                    original,
                    content(source_block(value, style, shape)),
                    "resize must not mutate IR"
                );
            }
        }
    }
}

#[test]
fn authored_edge_spaces_remain_painted_and_selectable_without_surface_fill() {
    for value in ["  abc def", "abc  ", "        ", "  abc  "] {
        for literal in [false, true] {
            let view = DocumentView::new(&content(source_block(value, 2, usize::from(literal))));
            for width in [1, 2, 4, 8, 80] {
                let rendered = view.render(width);
                let copied = rendered.selected_text(RenderedSelection {
                    anchor: TextPosition { row: 0, column: 0 },
                    focus: TextPosition {
                        row: rendered.row_count - 1,
                        column: usize::MAX - 1,
                    },
                });
                // Selection remains visual-cell copying. Only internal soft
                // seams can be unpainted; authored hard-row edges never are.
                let flat = copied.replace('\n', "");
                let leading = value.len() - value.trim_start_matches(' ').len();
                let trailing = value.len() - value.trim_end_matches(' ').len();
                assert!(
                    flat.starts_with(&" ".repeat(leading)),
                    "{value:?} width={width}: {copied:?}"
                );
                assert!(
                    flat.ends_with(&" ".repeat(trailing)),
                    "{value:?} width={width}: {copied:?}"
                );
                if literal || value.trim().is_empty() {
                    assert_eq!(flat, value);
                }
            }
        }
    }
}

#[test]
fn tab_search_projects_one_source_scalar_to_all_its_display_parts() {
    let view = DocumentView::new(&content(source_block("A\tB", 2, 0)));
    for width in [1, 2, 3, 8, 20, 1, 20] {
        let rendered = view.render(width);
        let tabs = rendered.search("\t");
        let [tab] = tabs.as_slice() else {
            panic!("one original TAB")
        };
        let fragments = std::iter::once((tab.row, tab.start_column, tab.end_column)).chain(
            tab.additional_fragments
                .iter()
                .map(|part| (part.row, part.start_column, part.end_column)),
        );
        for (row, start, end) in fragments {
            for column in start..end {
                assert!(rendered.link_target_at(row, column).is_some());
            }
        }
        assert_eq!(rendered.search("A\tB").len(), 1);
        assert_eq!(rendered.search("A       B").len(), 0);
    }
}

#[test]
fn nested_table_groups_keep_independent_scalar_origins_and_tabs() {
    for pair in [["A", "B"], ["A\t", "\tB"], ["\t", "\t"]] {
        let inner = json!({"type":"table", "rows":[{"cells":pair.map(|value| json!({"blocks":[{"type":"paragraph", "children":[{"type":"link", "target":{"kind":"external", "uri":"https://example.test"}, "children":[{"type":"text", "value":value}]}]}]}))}]});
        let outer = serde_json::from_value(
            json!({"type":"table", "rows":[{"cells":[{"blocks":[inner]}]}]}),
        )
        .unwrap();
        let view = DocumentView::new(&content(outer));
        for width in [1, 2, 8, 20, 80, 2, 80] {
            let rendered = view.render(width);
            for value in pair {
                let expected = pair.iter().filter(|other| **other == value).count();
                assert_eq!(
                    rendered.search(value).len(),
                    expected,
                    "{pair:?} width={width}: {:?}",
                    rendered.search_records
                );
            }
            assert_eq!(
                rendered.search_records.len(),
                2,
                "independent owners cannot share scalar identity"
            );
        }
    }
}

#[test]
fn declared_tab_extent_and_pending_ascii_spaces_choose_the_same_rows_as_cli() {
    for gap in [2, 4] {
        for value in ["A\t ", "A\t", "A ", " \t "] {
            let table = json!({"type":"table", "columnPreferences":{"widths":[4,4], "gapColumns":gap}, "rows":[{"cells":[{"blocks":[{"type":"paragraph", "children":[{"type":"text", "value":value}]}]}, {"blocks":[{"type":"paragraph", "children":[{"type":"link", "target":{"kind":"external", "uri":"https://example.test"}, "children":[{"type":"text", "value":"B"}]}]}]}]}]});
            for nested in [false, true] {
                let block = if nested {
                    json!({"type":"table", "rows":[{"cells":[{"blocks":[table.clone()]}]}]})
                } else {
                    table.clone()
                };
                let source = content(serde_json::from_value(block).unwrap());
                let view = DocumentView::new(&source);
                for width in [20, 80] {
                    let rendered = view.render(width);
                    let hits = rendered.search("B");
                    let [hit] = hits.as_slice() else {
                        panic!("one B")
                    };
                    // TAB reaches local column 8. The first declared field
                    // ends at width 4 + gap, while an ordinary trailing
                    // space remains pending with content width only 1.
                    let row = usize::from(value.contains('\t'));
                    let column = 4 + gap;
                    assert_eq!(
                        (hit.row, hit.start_column),
                        (row, column),
                        "{value:?}/{gap}/{nested}: {:?}",
                        rendered.text
                    );
                    let cli = mant_render::render_query_text_at_width(
                        &source,
                        usize::from(width),
                        |_, text| text.into(),
                    );
                    let cli_rows: Vec<_> = cli
                        .strip_prefix("demo\n\n")
                        .expect("query label precedes the document body")
                        .lines()
                        .collect();
                    assert_eq!(cli_rows[row].find('B'), Some(column), "{cli:?}");
                    assert!(rendered.link_target_at(hit.row, hit.start_column).is_some());
                    assert_eq!(
                        rendered.selected_text(RenderedSelection {
                            anchor: TextPosition {
                                row: hit.row,
                                column: hit.start_column
                            },
                            focus: TextPosition {
                                row: hit.row,
                                column: hit.start_column
                            }
                        }),
                        "B"
                    );
                }
            }
        }
    }
}
