//! Source-neutral frame and spanning-area contracts, after real JSON text round trips.
use super::super::*;
use mant_ir::{TableAlignment, TableBorderStyle, TableBorders};
use serde_json::json;

fn query_table(value: serde_json::Value) -> ResolvedContent {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![serde_json::from_value(value).unwrap()];
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    serde_json::from_str::<mant_protocol::QueryBundle>(&json)
        .unwrap()
        .into()
}

fn cell(value: &str) -> serde_json::Value {
    json!({"blocks":[{"type":"paragraph","children":[{"type":"text","value":value}]}]})
}

fn copied(rendered: &RenderedDocument) -> String {
    rendered.selected_text(RenderedSelection {
        anchor: TextPosition { row: 0, column: 0 },
        focus: TextPosition {
            row: rendered.row_count - 1,
            column: usize::MAX - 1,
        },
    })
}

#[test]
fn authored_frames_do_not_enter_search_copy_or_link_hit_cells() {
    for stroke in [TableBorderStyle::Single, TableBorderStyle::Double] {
        let query = query_table(json!({"type":"table", "alignment":"center",
            "borders":TableBorders { outer:Some(stroke), inner_rows:Some(stroke), inner_columns:Some(stroke) },
            "rows":[{"cells":[cell("A"),cell("B")]},{"cells":[cell("C"),cell("D")]}]
        }));
        let rendered = DocumentView::new(&query).render(25);
        let lines = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let (top, middle, bottom, side) = match stroke {
            TableBorderStyle::Single => ("┌───┬───┐", "├───┼───┤", "└───┴───┘", "│"),
            TableBorderStyle::Double => ("╔═══╦═══╗", "╠═══╬═══╣", "╚═══╩═══╝", "║"),
        };
        let offset = " ".repeat(8);
        assert_eq!(
            lines,
            vec![
                format!("{offset}{top}"),
                format!("{offset}{side} A {side} B {side}"),
                format!("{offset}{middle}"),
                format!("{offset}{side} C {side} D {side}"),
                format!("{offset}{bottom}")
            ]
        );
        assert_eq!(rendered.search("A")[0].start_column, 10);
        assert_eq!(rendered.search(side).len(), 0);
        assert_eq!(copied(&rendered), "A  B\nC  D");
    }
}

#[test]
fn soft_native_widths_shrink_before_a_centered_frame_can_stack() {
    let mut first = cell("alpha beta gamma delta epsilon");
    first["preferredWidthColumns"] = json!(34);
    let mut second = cell("one two three four five six seven");
    second["preferredWidthColumns"] = json!(34);
    let query = query_table(
        json!({"type":"table","alignment":"center","borders":{"outer":"single","innerColumns":"single"},"rows":[{"cells":[first,second]}]}),
    );
    let view = DocumentView::new(&query);
    for width in [32, 50, 80] {
        let rendered = view.render(width);
        assert_eq!(
            rendered.search("alpha")[0].row,
            rendered.search("one")[0].row,
            "width {width}"
        );
        assert!(
            rendered
                .text
                .lines
                .iter()
                .all(|line| super::super::super::inline::spans_width(&line.spans)
                    <= usize::from(width))
        );
        assert_eq!(rendered.search("epsilon").len(), 1);
        assert_eq!(rendered.search("seven").len(), 1);
    }
}

#[test]
fn span_owners_use_the_entire_area_and_suppress_internal_grid_segments() {
    let mut owner = cell("OWNER");
    owner["columnSpan"] = json!(2);
    owner["rowSpan"] = json!(2);
    let query = query_table(
        json!({"type":"table", "borders":{"outer":"single","innerRows":"single","innerColumns":"single"},
            "rows":[{"cells":[owner,cell("A")]}, {"kind":"horizontal-rule","cells":[]}, {"cells":[{"blocks":[]},{"blocks":[]},cell("B")]}]
        }),
    );
    let rendered = DocumentView::new(&query).render(40);
    let owner = &rendered.search("OWNER")[0];
    let row = rendered.text.lines[owner.row].to_string();
    assert_eq!(
        row.matches('│').count(),
        3,
        "outer sides and only the span's end: {row}"
    );
    assert_eq!(rendered.search("OWNER").len(), 1);
    let rule = rendered.text.lines[owner.row + 1].to_string();
    assert_eq!(
        rule.chars().filter(|character| *character == '─').count(),
        3,
        "only final column, its leading gap and right padding have a rule: {rule}"
    );
    assert_eq!(
        rule.matches('│').count(),
        1,
        "no internal divider through the covering owner: {rule}"
    );
    assert_eq!(rendered.search("B")[0].row, owner.row + 2);
    assert!(!copied(&rendered).contains('│'));
}

#[test]
fn declared_fields_keep_sequential_placement_inside_an_outer_frame() {
    let query = query_table(
        json!({"type":"table","alignment":TableAlignment::Left,"borders":{"outer":"single"},"columnPreferences":{"widths":[3,3]},"rows":[{"cells":[cell("LONGFIRST"),cell("NEXT")]}]}),
    );
    let rendered = DocumentView::new(&query).render(40);
    assert_eq!(rendered.search("LONGFIRST").len(), 1);
    assert_eq!(rendered.search("NEXT").len(), 1);
    assert!(rendered.search("NEXT")[0].row >= rendered.search("LONGFIRST")[0].row);
    assert_eq!(
        copied(&rendered).lines().collect::<Vec<_>>(),
        ["LONGFIRST", "NEXT"]
    );
}

#[test]
fn nested_frame_rows_are_not_copyable_when_a_neighbor_cell_has_body() {
    let nested =
        json!({"type":"table","borders":{"outer":"single"},"rows":[{"cells":[cell("INNER")]}]});
    let query = query_table(
        json!({"type":"table","borders":{"outer":"double"},"rows":[{"cells":[{"blocks":[nested]},cell("NEIGHBOR")]}]}),
    );
    let rendered = DocumentView::new(&query).render(60);
    let selected = copied(&rendered);
    assert!(
        selected.contains("INNER") && selected.contains("NEIGHBOR"),
        "{selected:?}"
    );
    assert!(
        !selected
            .chars()
            .any(|character| "─│┌┐└┘═║╔╗╚╝".contains(character)),
        "{selected:?}"
    );
}

#[test]
fn navigation_carriers_do_not_consume_a_vertical_span_row() {
    let mut owner = cell("OWNER");
    owner["rowSpan"] = json!(2);
    let mut table = json!({"type":"table", "borders":{"outer":"single","innerRows":"single"},"rows":[{"cells":[owner,cell("A")]},{"cells":[{"blocks":[]},cell("B")]}]});
    let original = DocumentView::new(&query_table(table.clone())).render(40);
    table["rows"].as_array_mut().unwrap().insert(1,json!({"cells":[{"blocks":[{"type":"paragraph","children":[{"type":"anchor","id":"inside-span"}]}]}]}));
    let marked = DocumentView::new(&query_table(table)).render(40);
    assert_eq!(original.text, marked.text);
    assert_eq!(
        marked.anchor_row("inside-span"),
        Some(marked.search("B")[0].row)
    );
}

#[test]
fn mixed_outer_and_grid_strokes_join_without_changing_either_strength() {
    let query = query_table(
        json!({"type":"table","borders":{"outer":"double","innerRows":"single","innerColumns":"single"},"rows":[{"cells":[cell("A"),cell("B")]},{"cells":[cell("C"),cell("D")]}]}),
    );
    let rendered = DocumentView::new(&query).render(30);
    assert_eq!(rendered.text.lines[0].to_string(), "╔═══╤═══╗");
    assert_eq!(rendered.text.lines[2].to_string(), "╟───┼───╢");
    assert_eq!(rendered.text.lines[4].to_string(), "╚═══╧═══╝");
}

#[test]
fn a_soft_width_does_not_split_a_word_that_fits_the_real_viewport() {
    let mut word = cell("UNBREAKABLE_LONG_TOKEN");
    word["preferredWidthColumns"] = json!(4);
    let query = query_table(
        json!({"type":"table","borders":{"outer":"single","innerColumns":"single"},"rows":[{"cells":[word,cell("RIGHT")]}]}),
    );
    let view = DocumentView::new(&query);
    let wide = view.render(80);
    assert_eq!(
        wide.text
            .lines
            .iter()
            .filter(|line| line.to_string().contains("UNBREAKABLE_LONG_TOKEN"))
            .count(),
        1
    );
    let narrow = view.render(20);
    assert_eq!(narrow.search("UNBREAKABLE_LONG_TOKEN").len(), 1);
    assert_eq!(narrow.search("RIGHT").len(), 1);
}

#[test]
fn nonbreaking_spaces_are_part_of_the_preferred_word_width() {
    for space in ['\u{a0}', '\u{2007}', '\u{202f}'] {
        let text = format!("AA{space}BBBB");
        let mut word = cell(&text);
        word["preferredWidthColumns"] = json!(2);
        let query = query_table(
            json!({"type":"table","borders":{"outer":"single"},"rows":[{"cells":[word]}]}),
        );
        let rendered = DocumentView::new(&query).render(40);
        assert_eq!(
            rendered
                .text
                .lines
                .iter()
                .filter(|line| line.to_string().contains(&text))
                .count(),
            1
        );
    }
}

#[test]
fn a_fully_covered_grid_boundary_does_not_create_a_blank_decoration_row() {
    let mut owner = cell("OWNER");
    owner["columnSpan"] = json!(2);
    owner["rowSpan"] = json!(2);
    let query = query_table(
        json!({"type":"table","borders":{"outer":"single","innerRows":"single","innerColumns":"single"},"rows":[{"cells":[owner]},{"cells":[{"blocks":[]},{"blocks":[]}]}]}),
    );
    let rendered = DocumentView::new(&query).render(40);
    assert_eq!(rendered.row_count, 4);
    assert_eq!(rendered.search("OWNER").len(), 1);
    assert!(
        !rendered
            .text
            .lines
            .iter()
            .any(|line| line.to_string().contains('├'))
    );
}

#[test]
fn tall_cells_do_not_expand_missing_logical_slots_on_every_physical_row() {
    let values = (0..100)
        .map(|index| format!("LINE_{index}"))
        .collect::<Vec<_>>();
    let literal = json!({"blocks":[{"type":"preformatted","children":[{"type":"text","value":values.join("\n")}]}]});
    let query = query_table(
        json!({"type":"table","columnCount":256,"borders":{"outer":"single"},"rows":[{"cells":[literal,cell("TAIL")]}]}),
    );
    let rendered = DocumentView::new(&query).render(1000);
    assert_eq!(rendered.search("LINE_99").len(), 1);
    assert_eq!(rendered.search("TAIL").len(), 1);
    let tail = &rendered.search("TAIL")[0];
    assert!(tail.row > rendered.search("LINE_99")[0].row);
    assert_eq!(rendered.row_count, 103);
    assert!(
        rendered
            .text
            .lines
            .iter()
            .all(|line| super::super::super::inline::spans_width(&line.spans) < 32)
    );
}

#[test]
fn declared_fields_keep_a_continuous_outside_frame_when_inner_grids_are_simplified() {
    let query = query_table(
        json!({"type":"table","borders":{"outer":"single","innerRows":"single","innerColumns":"single"},"columnPreferences":{"widths":[4,4]},"rows":[{"cells":[cell("A"),cell("B")]},{"cells":[cell("C"),cell("D")]}]}),
    );
    let rendered = DocumentView::new(&query).render(40);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            "┌─────────┐",
            "│ A     B │",
            "├─────────┤",
            "│ C     D │",
            "└─────────┘"
        ]
    );
}

#[test]
fn framed_sparse_layout_rules_keep_each_original_stroke_strength() {
    let query = query_table(
        json!({"type":"table","columnCount":257,"borders":{"outer":"single"},"rows":[{"kind":{"layout-rule":{"cells":["horizontal","double-horizontal"]}},"cells":[]},{"cells":[cell("A")]}]}),
    );
    let rendered = DocumentView::new(&query).render(40);
    assert_eq!(rendered.search("A").len(), 1);
    assert_eq!(rendered.row_count, 5);
    assert!(rendered.text.lines[1].to_string().contains('─'));
    assert!(rendered.text.lines[2].to_string().contains('═'));
    assert_eq!(copied(&rendered), "A");
}

#[test]
fn soft_cell_widths_do_not_reflow_authored_literal_lines_when_the_viewport_fits() {
    let query = query_table(
        json!({"type":"table","borders":{"outer":"single"},"rows":[{"cells":[{"preferredWidthColumns":2,"blocks":[{"type":"preformatted","children":[{"type":"text","value":"AA BB CC"}]}]}]}]}),
    );
    let rendered = DocumentView::new(&query).render(80);
    assert_eq!(rendered.row_count, 3);
    assert!(rendered.text.lines[1].to_string().contains("AA BB CC"));
}

#[test]
fn declared_field_widths_own_placement_even_when_a_cell_has_a_soft_preference() {
    let mut table = json!({"type":"table","alignment":"center","borders":{"outer":"single"},"columnPreferences":{"widths":[4,4]},"rows":[{"cells":[cell("A"),cell("B")]}]});
    let plain = DocumentView::new(&query_table(table.clone())).render(80);
    table["rows"][0]["cells"][0]["preferredWidthColumns"] = json!(100);
    let preferred = DocumentView::new(&query_table(table)).render(80);
    assert_eq!(plain.text, preferred.text);
    assert_eq!(
        plain.search("A")[0].start_column,
        preferred.search("A")[0].start_column
    );
}

#[test]
fn minimum_frame_width_keeps_glyphs_links_copy_and_buffer_coordinates_together() {
    // The exact cw12_box.1 source was replayed with pristine CVS before
    // this assertion. tbl_term.c reserves both outside rails separately from
    // data columns; responsive wrapping may shrink the body, not its maps.
    for stroke in ["single", "double"] {
        for (count, span) in [(1, 1), (2, 1), (2, 2), (3, 2)] {
            let owner = json!({"columnSpan":span,"alignment":"center","blocks":[{
                "type":"paragraph","children":[{"type":"anchor","id":"minimum-frame"},
                    {"type":"link","target":{"kind":"external","uri":"https://example.test/minimum"},
                        "children":[{"type":"text","value":"A B"}]}]}]});
            let value = query_table(json!({"type":"table","columnCount":count,
                "layout":{"indentColumns":3},"borders":{"outer":stroke},"rows":[{"cells":[owner]}]}));
            let view = DocumentView::new(&value);
            for width in [7, 8, 9, 10, 8] {
                let rendered = view.render(width);
                for marker in ["A", "B"] {
                    assert_minimum_frame_hit(&rendered, width, marker);
                }
                let first = &rendered.search("A")[0];
                assert_eq!(rendered.anchor_row("minimum-frame"), Some(first.row));
                assert!(
                    rendered
                        .text
                        .lines
                        .iter()
                        .all(|line| super::super::super::inline::spans_width(&line.spans)
                            <= usize::from(width)),
                    "{stroke}, {count}/{span}, {width}: {:?}",
                    rendered.text
                );
                let selected = copied(&rendered);
                assert!(!selected.contains(['│', '║', '┌', '╔']), "{selected:?}");
            }
        }
    }
}

fn assert_minimum_frame_hit(rendered: &RenderedDocument, width: u16, marker: &str) {
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
    let hits = rendered.search(marker);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    let area = Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
    let mut buffer = Buffer::empty(area);
    rendered.text.clone().render(area, &mut buffer);
    assert_eq!(
        buffer[(
            u16::try_from(hit.start_column).unwrap(),
            u16::try_from(hit.row).unwrap()
        )]
            .symbol(),
        marker
    );
    assert!(rendered.link_target_at(hit.row, hit.start_column).is_some());
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: hit.row,
                column: hit.start_column
            },
            focus: TextPosition {
                row: hit.row,
                column: hit.end_column - 1
            },
        }),
        marker
    );
}

#[test]
fn stacked_structural_prefixes_remain_copyable_while_frame_prefixes_are_omitted() {
    for outer in [None, Some("single"), Some("double")] {
        let borders = outer.map_or_else(|| json!({}), |style| json!({"outer":style}));
        let query = query_table(
            json!({"type":"table","columnCount":257,"layout":{"indentColumns":3},
            "borders":borders,"rows":[{"cells":[{"blocks":[{"type":"preformatted","children":[{"type":"text","value":"  "}]}]}]}]}),
        );
        let rendered = DocumentView::new(&query).render(40);
        assert_eq!(
            copied(&rendered),
            if outer.is_none() { "     " } else { "  " }
        );
    }
}
