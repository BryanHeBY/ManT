//! Physical word columns survive output ownership and real viewport rendering.
use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
use serde_json::Value;

fn cases() -> Vec<Value> {
    let matrix: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mant-engine/tests/roff_lowering/empty_word_columns/cases.json"
    )))
    .unwrap();
    assert_eq!(matrix["header"]["count"], 472);
    assert_eq!(matrix["header"]["asserted_row_count"], 460);
    matrix["cases"].as_array().unwrap().clone()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}
fn number(value: &Value) -> usize {
    value.as_u64().unwrap().try_into().unwrap()
}

fn roundtrip(case: &Value) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(text(&case["source"]).as_bytes()).unwrap();
    let encoded = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&encoded).unwrap();
    let decoded: ResolvedContent = decoded.into();
    assert_eq!(decoded, content);
    decoded
}

fn buffer(rendered: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut cells = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut cells);
    cells
}

fn copied_hit(rendered: &RenderedDocument, hit: &RenderedSearchMatch) -> String {
    std::iter::once(RenderedSearchFragment {
        row: hit.row,
        start_column: hit.start_column,
        end_column: hit.end_column,
    })
    .chain(hit.additional_fragments.iter().copied())
    .map(|fragment| {
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: fragment.row,
                column: fragment.start_column,
            },
            focus: TextPosition {
                row: fragment.row,
                column: fragment.end_column - 1,
            },
        })
    })
    .collect()
}

fn check_body(
    rendered: &RenderedDocument,
    cells: &Buffer,
    position: &Value,
    base_row: usize,
    exact_column: bool,
    context: &str,
) {
    let word = text(&position["value"]);
    let hits = rendered.search(word);
    let [hit] = hits.as_slice() else {
        panic!("{context}: missing or duplicate {word}: {hits:?}")
    };
    assert_eq!(
        copied_hit(rendered, hit),
        word,
        "{context}: mapped selection"
    );
    assert_eq!(
        cells[(
            hit.start_column.try_into().unwrap(),
            hit.row.try_into().unwrap()
        )]
            .symbol(),
        &word[..1],
        "{context}: real cell"
    );
    if !exact_column {
        return;
    }
    assert!(
        hit.additional_fragments.is_empty(),
        "{context}: bounded witness unexpectedly wraps"
    );
    assert_eq!(
        hit.start_column,
        number(&position["column"]) + 3,
        "{context}: source column"
    );
    assert_eq!(
        hit.row,
        base_row + number(&position["row"]),
        "{context}: physical row"
    );
    let expected = format!("   {}{word}", text(&position["prefix"]));
    let copied = rendered.selected_text(RenderedSelection {
        anchor: TextPosition {
            row: hit.row,
            column: 0,
        },
        focus: TextPosition {
            row: hit.row,
            column: hit.end_column - 1,
        },
    });
    assert_eq!(copied, expected, "{context}: SP/NBSP/combining prefix copy");
}

fn check_active_links(rendered: &RenderedDocument, case: &Value) {
    let Some(uri) = case["metadata"]["active_target"].as_str() else {
        return;
    };
    let target = LinkTarget::External(ExternalUri::parse(uri).unwrap());
    let label = text(&case["metadata"]["active_label"]);
    let hits = rendered.search(label);
    let [hit] = hits.as_slice() else {
        panic!("{}: URI {uri}", text(&case["id"]))
    };
    assert_eq!(copied_hit(rendered, hit), label);
    for fragment in std::iter::once(RenderedSearchFragment {
        row: hit.row,
        start_column: hit.start_column,
        end_column: hit.end_column,
    })
    .chain(hit.additional_fragments.iter().copied())
    {
        assert_eq!(
            rendered.link_target_at(fragment.row, fragment.start_column),
            Some(&target)
        );
        assert_eq!(
            rendered.link_target_at(fragment.row, fragment.end_column - 1),
            Some(&target)
        );
    }
}

fn check_view(case: &Value, content: &ResolvedContent) {
    let view = DocumentView::new(content);
    let initial = view.render(20).text;
    for width in [20, 40, 78, 120, 20] {
        let rendered = view.render(width);
        let cells = buffer(&rendered, width);
        let heading = rendered.search("DESCRIPTION");
        assert_eq!(heading.len(), 1);
        let label = format!("{} width={width}", text(&case["id"]));
        // Only the eight long URI sources soft-wrap at 20 cells. Their actual
        // search/selection/link fragments still run there; exact source column
        // and all physical rows run at 40/78/120. All other 452 asserted sources
        // fit in every width, including the three-cell standard section origin.
        let exact_column = width != 20 || case["metadata"]["long_row"].is_null();
        for position in case["body_positions"].as_array().unwrap() {
            check_body(
                &rendered,
                &cells,
                position,
                heading[0].row + 1,
                exact_column,
                &label,
            );
        }
        check_active_links(&rendered, case);
    }
    assert_eq!(
        view.render(20).text,
        initial,
        "{}: resize cache",
        text(&case["id"])
    );
}

#[test]
fn native_empty_buffer_word_columns_survive_json_buffer_resize_and_copy() {
    // Every exact fixture ran registered pristine in five profiles before
    // assertions. term_newln (term.c:475-481) selects NOSPACE even with no
    // native buffer. No/Em/Sy/Li/Lk, raw empty TEXT, bare/pending zero advance,
    // author SP/NBSP and actual Sh BODY ownership are independent matrix axes.
    let mut checked = 0;
    for case in cases() {
        let content = roundtrip(&case);
        if text(&case["row_observation"]) != "asserted" {
            continue;
        }
        checked += 1;
        let before = content.clone();
        check_view(&case, &content);
        assert_eq!(content, before);
    }
    assert_eq!(checked, 460);
}

fn check_reader_column(
    case: &Value,
    position: &Value,
    rendered: &RenderedDocument,
    hit: &RenderedSearchMatch,
    width: u16,
) {
    if width == 20 && !case["metadata"]["long_row"].is_null() {
        return;
    }
    let word = text(&position["value"]);
    let source_word = if word.starts_with("ODY_") {
        format!("B{word}")
    } else {
        word.into()
    };
    let owner = case["ordinary_body_owners"]
        .as_array()
        .unwrap()
        .iter()
        .find(|owner| text(&owner["label"]) == source_word)
        .unwrap();
    let native_prefix = text(&position["prefix"]);
    // Ordinary Markdown phrasing discards edge ASCII padding; native_text
    // selects native portable-display children, not terminal page geometry.
    // Fenced literal rows and authored NBSP retain their exact cell origins.
    let prefix = if owner["no_fill"].as_bool().unwrap() {
        native_prefix
    } else {
        native_prefix.trim_start_matches([' ', '\t'])
    };
    assert_eq!(
        hit.start_column,
        mant_ir::geometry::text_width(prefix) + 3,
        "{}: declared reader column at width {width}",
        text(&case["id"])
    );
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: hit.row,
                column: 0
            },
            focus: TextPosition {
                row: hit.row,
                column: hit.end_column - 1
            },
        }),
        format!("   {prefix}{word}")
    );
}

#[test]
fn native_markdown_reader_coordinates_map_to_its_declared_phrasing_projection() {
    for case in cases()
        .into_iter()
        .filter(|case| text(&case["row_observation"]) == "asserted")
    {
        let content = roundtrip(&case);
        let markdown = mant_codec::encode::render_markdown_with_options(
            &content,
            mant_codec::encode::MarkdownOptions {
                native_text: true,
                ..mant_codec::encode::MarkdownOptions::default()
            },
        );
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let view = DocumentView::new(&readback);
        let initial = view.render(20).text;
        for width in [20, 40, 78, 120, 20] {
            let rendered = view.render(width);
            let cells = buffer(&rendered, width);
            for position in case["body_positions"].as_array().unwrap() {
                let word = text(&position["value"]);
                let hits = rendered.search(word);
                let [hit] = hits.as_slice() else {
                    panic!("{}: reader {word}: {markdown}", text(&case["id"]))
                };
                assert_eq!(copied_hit(&rendered, hit), word);
                check_reader_column(&case, position, &rendered, hit, width);
                assert_eq!(
                    cells[(
                        hit.start_column.try_into().unwrap(),
                        hit.row.try_into().unwrap()
                    )]
                        .symbol(),
                    &word[..1]
                );
            }
            // Literal Markdown exports a fence, which has no active links.
            // Ordinary phrasing retains the original typed visible label.
            if text(&case["metadata"]["mode"]) != "no-fill" {
                check_active_links(&rendered, &case);
            }
        }
        assert_eq!(view.render(20).text, initial);
    }
}
