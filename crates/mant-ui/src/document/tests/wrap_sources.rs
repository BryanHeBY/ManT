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

#[test]
fn expanded_source_cells_reach_the_final_buffer_and_visual_selection() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };
    let view = DocumentView::new(&content(source_block("  A\tB  ", 2, 1)));
    // Local TAB stops advance column 3 to 8, independent of later wrapping.
    let expanded = "  A     B  ";
    for width in [1_u16, 2, 3, 4, 8, 20, 2, 20] {
        let rendered = view.render(width);
        let expected = expanded
            .as_bytes()
            .chunks(usize::from(width))
            .map(|row| std::str::from_utf8(row).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(rendered.row_count, expected.len());
        let area = Rect::new(0, 0, width, u16::try_from(expected.len()).unwrap());
        let mut buffer = Buffer::empty(area);
        Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
        for (row, text) in expected.iter().enumerate() {
            for (column, character) in text.chars().enumerate() {
                assert_eq!(
                    buffer[(u16::try_from(column).unwrap(), u16::try_from(row).unwrap())].symbol(),
                    character.to_string()
                );
                assert_eq!(
                    rendered.link_target_at(row, column),
                    Some(&LinkTarget::External(
                        ExternalUri::parse("https://example.test").unwrap()
                    )),
                    "authored TAB/edge cell lost its original target"
                );
            }
            for column in text.len()..usize::from(width) {
                assert!(
                    rendered.link_target_at(row, column).is_none(),
                    "surface paint must not extend the target"
                );
                assert_eq!(
                    buffer[(u16::try_from(column).unwrap(), u16::try_from(row).unwrap())].symbol(),
                    " "
                );
            }
        }
        let copied = rendered.selected_text(RenderedSelection {
            anchor: TextPosition { row: 0, column: 0 },
            focus: TextPosition {
                row: expected.len() - 1,
                column: usize::MAX - 1,
            },
        });
        assert_eq!(copied, expected.join("\n"));
        assert_eq!(rendered.search("  A\tB  ").len(), 1);
    }
}

#[test]
fn sanitized_control_glyphs_do_not_turn_into_soft_separators_after_resize() {
    for control in ['\u{b}', '\u{c}', '\r', '\u{85}'] {
        let source = format!("ab{control}cd");
        let view = DocumentView::new(&content(source_block(&source, 0, 0)));
        for width in [80, 1, 2, 3, 4, 8, 80] {
            let rendered = view.render(width);
            assert_eq!(
                rendered.search("ab�cd").len(),
                1,
                "{control:?} width={width}: {:?}",
                rendered.search_records
            );
            assert_eq!(rendered.search(&source).len(), 0);
        }
    }
}

fn assert_resolved_tab_link_row(
    rendered: &RenderedDocument,
    row: usize,
    width: u16,
    body_origin: usize,
    expected: &str,
) {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };

    let area = Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    let target = LinkTarget::External(ExternalUri::parse("https://example.test/tab").unwrap());
    let visual_row = format!("{}{expected}", " ".repeat(body_origin));
    for column in 0..usize::from(width) {
        let character = visual_row.as_bytes().get(column).copied().unwrap_or(b' ');
        assert_eq!(
            buffer[(u16::try_from(column).unwrap(), u16::try_from(row).unwrap())].symbol(),
            char::from(character).to_string(),
            "width={width}, column={column}"
        );
        assert_eq!(
            rendered.link_target_at(row, column),
            (body_origin..body_origin + 4)
                .contains(&column)
                .then_some(&target),
            "only the executed description label is clickable"
        );
    }
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row,
                column: body_origin
            },
            focus: TextPosition {
                row,
                column: body_origin + 3
            },
        }),
        "A  B"
    );
    // Visual copying retains the structural section origin and inter-cell
    // placement, but not viewport fill.
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition { row, column: 0 },
            focus: TextPosition {
                row,
                column: usize::MAX - 1,
            },
        }),
        visual_row
    );
}

#[test]
fn executed_tab_links_keep_their_owner_after_json_and_resize() {
    // This complete source ran fixed CVS ASCII/UTF-8(width78), HTML and tree
    // before the assertions. mdoc_term.c::termp_lk_pre() executes description,
    // colon and URI; term.c::term_field() uses the configured 3n stops at the
    // real .br flush. A at local column 0 makes this TAB exactly two spaces.
    let source = concat!(
        ".Dd November 3, 2026\n.Dt TAB-LINK 1\n.Os\n.Sh DESCRIPTION\n",
        ".Bl -column \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\" \"xxxx\"\n",
        ".It Xo\n.ta T 3n\n.Lk https://example.test/tab \"A\tB\"\n",
        ".br\n.No AFTER\n.Xc Ta RIGHT\n.El\n.Sh NEXT\n.No END\n",
    );
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = mant_render::render_query_json(&original, false).unwrap();
    let query: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into();
    assert_eq!(
        query, original,
        "JSON must retain every owner and target fact"
    );
    let [
        Block::Table {
            rows,
            column_preferences,
            ..
        },
    ] = query.document.as_ref().unwrap().sections[0]
        .blocks
        .as_slice()
    else {
        panic!("one original column owner");
    };
    assert_eq!(column_preferences.widths, [64, 4]);
    assert_eq!(column_preferences.gap_columns, 4);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 2);
    let [
        Block::Paragraph {
            children, source, ..
        },
    ] = rows[0].cells[0].blocks.as_slice()
    else {
        panic!("the executed link remains in its source paragraph");
    };
    assert_eq!(source.as_ref().map(|at| (at.line, at.column)), Some((8, 2)));
    let owner_text = "A  B: https://example.test/tab AFTER";
    assert_eq!(mant_ir::inline_plain_text(children), owner_text);
    let links: Vec<_> = children
        .iter()
        .filter_map(|node| match node {
            Inline::Link {
                target, children, ..
            } => Some((target, children)),
            _ => None,
        })
        .collect();
    let [(target, label)] = links.as_slice() else {
        panic!("one original link occurrence");
    };
    assert_eq!(target.to_uri().as_deref(), Some("https://example.test/tab"));
    assert_eq!(mant_ir::inline_plain_text(label), "A  B");
    let expected = format!("{owner_text}{}RIGHT", " ".repeat(68 - owner_text.len()));
    // DocumentBuilder::section_with_position() gives a top-level section
    // body a structural origin of three cells; CLI omits that reader margin.
    let body_origin = 3;
    let view = DocumentView::new(&query);
    for width in [78, 120, 78] {
        let cli = mant_render::render_query_text_at_width(&query, usize::from(width), |_, text| {
            text.into()
        });
        let cli_owner = cli.split_once("DESCRIPTION\n").unwrap().1;
        assert_eq!(cli_owner.split_once("\n\nNEXT").unwrap().0, expected);
        let rendered = view.render(width);
        for needle in ["A  B", "https://example.test/tab", "AFTER", "RIGHT"] {
            assert_eq!(rendered.search(needle).len(), 1, "{needle}, width={width}");
        }
        assert_eq!(rendered.search("A\tB").len(), 0);
        let hits = rendered.search("A  B");
        let [hit] = hits.as_slice() else {
            panic!("one executed description label");
        };
        assert_eq!(
            (hit.start_column, hit.end_column),
            (body_origin, body_origin + 4)
        );
        let right = &rendered.search("RIGHT")[0];
        assert_eq!((right.row, right.start_column), (hit.row, body_origin + 68));
        assert_resolved_tab_link_row(&rendered, hit.row, width, body_origin, &expected);
    }
    assert_eq!(
        query, original,
        "rendering must not mutate the source owners"
    );
}
