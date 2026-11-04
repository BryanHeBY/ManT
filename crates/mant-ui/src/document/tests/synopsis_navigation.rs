//! Native source labels retain distinct search, copy and activation ranges.

use super::*;
use mant_ir::visit::{self, Visit};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
use serde_json::Value;

const BODY_ORIGIN: usize = 3;

#[derive(Debug)]
struct Label {
    row: usize,
    column: usize,
    text: String,
    target: Option<LinkTarget>,
}

#[derive(Default)]
struct SourceLinks(Vec<(String, mant_ir::LinkTarget)>);

impl<'ir> Visit<'ir> for SourceLinks {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Link {
            children, target, ..
        } = inline
        {
            self.0
                .push((mant_ir::inline_plain_text(children), target.clone()));
        }
        visit::walk_inline(self, inline);
    }
}

fn number(value: &Value) -> usize {
    value.as_u64().unwrap().try_into().unwrap()
}

fn labels(case: &Value, linked: bool) -> Vec<Label> {
    case[if linked { "links" } else { "names" }]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| Label {
            row: number(&value["row"]),
            column: BODY_ORIGIN + number(&value["column"]),
            text: value["label"].as_str().unwrap().into(),
            target: linked.then(|| LinkTarget::Manual {
                name: value["name"].as_str().unwrap().into(),
                manual_section: Some(value["section"].as_str().unwrap().into()),
            }),
        })
        .collect()
}

fn paint(rendered: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    buffer
}

fn heading_row(buffer: &Buffer, expected: &str) -> usize {
    let rows: Vec<_> = (0..buffer.area.height)
        .filter(|row| {
            (0..buffer.area.width).all(|column| {
                let expected = expected.get(usize::from(column)..usize::from(column) + 1);
                buffer[(column, *row)].symbol() == expected.unwrap_or(" ")
            })
        })
        .collect();
    assert_eq!(rows.len(), 1, "one actual heading {expected}");
    usize::from(rows[0])
}

fn copy(rendered: &RenderedDocument, row: usize, start: usize, end: usize) -> String {
    rendered.selected_text(RenderedSelection {
        anchor: TextPosition { row, column: start },
        focus: TextPosition { row, column: end },
    })
}

fn assert_label_search(rendered: &RenderedDocument, start: usize, rows: usize, labels: &[Label]) {
    for label in labels {
        let hits: Vec<_> = rendered
            .search(&label.text)
            .into_iter()
            .filter(|hit| (start..start + rows).contains(&hit.row))
            .collect();
        let expected: Vec<_> = labels
            .iter()
            .filter(|other| other.text == label.text)
            .map(|other| {
                (
                    start + other.row,
                    other.column,
                    other.column + other.text.len(),
                )
            })
            .collect();
        assert_eq!(
            hits.iter()
                .map(|hit| (hit.row, hit.start_column, hit.end_column))
                .collect::<Vec<_>>(),
            expected,
            "independent source-label coordinates: {label:?}"
        );
        assert!(hits.iter().all(|hit| hit.additional_fragments.is_empty()));
        assert_eq!(
            copy(
                rendered,
                start + label.row,
                label.column,
                label.column + label.text.len() - 1,
            ),
            label.text
        );
    }
}

fn assert_link_fragments(rendered: &RenderedDocument, start: usize, rows: usize, labels: &[Label]) {
    for fragment in rendered
        .links
        .iter()
        .filter(|fragment| (start..start + rows).contains(&fragment.row))
    {
        assert!(fragment.start_column < fragment.end_column);
        assert_eq!(
            labels
                .iter()
                .filter(|label| {
                    fragment.row == start + label.row
                        && Some(&fragment.target) == label.target.as_ref()
                        && label.column <= fragment.start_column
                        && fragment.end_column <= label.column + label.text.len()
                })
                .count(),
            1,
            "each display fragment stays inside one source occurrence: {fragment:?}"
        );
    }
    for label in labels {
        for column in label.column..label.column + label.text.len() {
            assert!(rendered.links.iter().any(|fragment| {
                fragment.row == start + label.row
                    && Some(&fragment.target) == label.target.as_ref()
                    && (fragment.start_column..fragment.end_column).contains(&column)
            }));
        }
    }
}

fn assert_cells(case: &Value, rendered: &RenderedDocument, width: u16) {
    let buffer = paint(rendered, width);
    let start = heading_row(&buffer, "SYNOPSIS") + 1;
    let rows: Vec<_> = case["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_str().unwrap())
        .collect();
    let links = labels(case, true);
    let names = labels(case, false);
    assert_eq!(heading_row(&buffer, "NEXT"), start + rows.len() + 1);
    for (offset, expected) in rows.iter().enumerate() {
        for column in 0..usize::from(width) {
            let glyph = column
                .checked_sub(BODY_ORIGIN)
                .and_then(|index| expected.get(index..index + 1))
                .unwrap_or(" ");
            assert_eq!(
                buffer[(
                    column.try_into().unwrap(),
                    (start + offset).try_into().unwrap()
                )]
                    .symbol(),
                glyph,
                "{}: width {width}, row {offset}, column {column}",
                case["id"]
            );
            let target = links.iter().find_map(|link| {
                (link.row == offset
                    && (link.column..link.column + link.text.len()).contains(&column))
                .then_some(link.target.as_ref().unwrap())
            });
            // Check every body cell, including the native generated separator
            // before a following Link, authored punctuation and viewport fill.
            assert_eq!(rendered.link_target_at(start + offset, column), target);
        }
        assert_eq!(
            copy(
                rendered,
                start + offset,
                BODY_ORIGIN,
                BODY_ORIGIN + expected.len() - 1,
            ),
            *expected
        );
    }
    // Selection follows visual cells: the first row starts at the requested
    // body column, while subsequent rows include their selected left margin.
    let mut selected_rows = vec![rows[0].to_owned()];
    selected_rows.extend(
        rows[1..]
            .iter()
            .map(|row| format!("{}{row}", " ".repeat(BODY_ORIGIN))),
    );
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: start,
                column: BODY_ORIGIN
            },
            focus: TextPosition {
                row: start + rows.len() - 1,
                column: BODY_ORIGIN + rows.last().unwrap().len() - 1,
            },
        }),
        selected_rows.join("\n")
    );
    assert_label_search(rendered, start, rows.len(), &links);
    assert_label_search(rendered, start, rows.len(), &names);
    assert_link_fragments(rendered, start, rows.len(), &links);
}

fn assert_source_owner(case: &Value, content: &ResolvedContent) {
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.id.as_str() == "synopsis")
        .unwrap();
    let [
        Block::Paragraph {
            children, source, ..
        },
    ] = section.blocks.as_slice()
    else {
        panic!("one complete Xo source owner");
    };
    let source = source.unwrap();
    assert_eq!(
        usize::try_from(source.line).unwrap(),
        number(&case["owner_source"]["line"])
    );
    assert_eq!(
        usize::try_from(source.column).unwrap(),
        number(&case["owner_source"]["column"])
    );
    let rows: Vec<_> = case["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_str().unwrap())
        .collect();
    assert_eq!(mant_ir::inline_plain_text(children), rows.join("\n"));
    let mut actual = SourceLinks::default();
    actual.visit_section(section);
    assert_eq!(
        actual.0,
        case["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|link| (
                link["label"].as_str().unwrap().to_owned(),
                mant_ir::LinkTarget::Manual {
                    name: link["name"].as_str().unwrap().into(),
                    manual_section: Some(link["section"].as_str().unwrap().into()),
                },
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn synopsis_labels_keep_json_buffer_search_copy_and_click_owners_after_resize() {
    // All five complete inputs are copied byte-for-byte from the existing
    // pinned five-profile oracle cohorts (134-source synopsis matrix + F0).
    // print_mdoc_node/termp_nm_pre execute HEAD/BODY in the same state;
    // term_field's preceding advance is outside Xr's scanf(3) label
    // (mdoc_term.c:300-445,1153-1178; term.c:389-427). Body origin 3 is the
    // known default view margin. No expected cell/target comes from a hit.
    let fixture: Value =
        serde_json::from_str(include_str!("synopsis_navigation/cases.json")).unwrap();
    assert_eq!(
        fixture["oracle_sha256"],
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 5);
    for case in fixture["cases"].as_array().unwrap() {
        let content =
            mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let decoded: ResolvedContent = decoded.into();
        assert_source_owner(case, &decoded);
        let view = DocumentView::new(&decoded);
        let initial = view.render(78).text;
        for width in [40, 78, 40, 78] {
            assert_cells(case, &view.render(width), width);
            assert_source_owner(case, &decoded);
            assert_eq!(
                serde_json::to_string(&mant_protocol::QueryBundle::from(&decoded)).unwrap(),
                wire,
                "{}: source owner changed after resize",
                case["id"]
            );
        }
        assert_eq!(view.render(78).text, initial);
    }
}
