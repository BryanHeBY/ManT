//! Real source JSON, viewport cells and source addresses share one table owner.
use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
use serde_json::Value;

fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}
fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| text(value).to_owned())
        .collect()
}
fn cases() -> Vec<Value> {
    // This package-local file is byte-identical to the engine fixture. No
    // cross-crate include is required by the published UI package.
    let matrix: Value =
        serde_json::from_str(include_str!("column_preferences/cases.json")).unwrap();
    assert_eq!(matrix["header"]["count"], 23);
    assert_eq!(matrix["header"]["profiles_per_source"], 5);
    assert_eq!(matrix["header"]["expectations_from_product"], false);
    matrix["cases"].as_array().unwrap().clone()
}

fn roundtrip(case: &Value) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(text(&case["source"]).as_bytes()).unwrap();
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
    assert!(!wire.contains("columnWidths"));
    assert!(!wire.contains("\\u0000mant:"));
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let decoded: ResolvedContent = decoded.into();
    assert_eq!(decoded, original);
    decoded
}

fn body_region(rendered: &RenderedDocument, case: &Value) -> std::ops::Range<usize> {
    let start = rendered
        .search("DESCRIPTION")
        .into_iter()
        .find(|hit| hit.start_column == 0)
        .unwrap()
        .row
        + 1;
    let end = rendered
        .search(text(&case["stop_heading"]))
        .into_iter()
        .find(|hit| hit.start_column == 0 && hit.row > start)
        .unwrap()
        .row;
    // Exactly one fixed section gap follows the table. The body keeps every
    // completed empty row; a bare open literal tail is reusable by the next cell.
    assert_eq!(rendered.text.lines[end - 1].to_string(), "");
    start..end - 1
}

fn body_rows(rendered: &RenderedDocument, case: &Value) -> Vec<String> {
    body_region(rendered, case)
        .map(|row| rendered.text.lines[row].to_string())
        .collect()
}

fn buffer(rendered: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut cells = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut cells);
    cells
}

fn fragments(hit: &RenderedSearchMatch) -> impl Iterator<Item = RenderedSearchFragment> + '_ {
    std::iter::once(RenderedSearchFragment {
        row: hit.row,
        start_column: hit.start_column,
        end_column: hit.end_column,
    })
    .chain(hit.additional_fragments.iter().copied())
}

fn copied_hit(rendered: &RenderedDocument, hit: &RenderedSearchMatch) -> String {
    fragments(hit)
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

fn original_link(content: &ResolvedContent, word: &str) -> Option<mant_ir::LinkTarget> {
    let mut target = None;
    let report = mant_ir::scan_reference_scope(
        content.document.as_ref().unwrap(),
        mant_ir::ReferenceScope::Block {
            sections: &[1],
            blocks: &[mant_ir::ContentBlockStep::Block { index: 0 }],
        },
        mant_ir::ReferenceScanLimits::default(),
        |occurrence, _| {
            if mant_ir::inline_plain_text(occurrence.label) == word {
                assert!(target.is_none(), "one original source link");
                target = Some(occurrence.target.clone());
            }
            std::ops::ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    target
}

fn check_source_cells(
    view: &DocumentView,
    content: &ResolvedContent,
    case: &Value,
    rendered: &RenderedDocument,
    width: u16,
) {
    let cells = buffer(rendered, width);
    let body = body_region(rendered, case);
    for word in strings(&case["payloads"])
        .iter()
        .flat_map(|value| value.split_whitespace())
    {
        let hits = rendered
            .search(word)
            .into_iter()
            .filter(|hit| body.contains(&hit.row))
            // Search is case-insensitive and may find "a" inside LastWord.
            // Verify the actual selected source scalars before asserting the
            // single occurrence of each independently supplied source word.
            .filter(|hit| copied_hit(rendered, hit) == word)
            .collect::<Vec<_>>();
        let [hit] = hits.as_slice() else {
            panic!(
                "{} width={width}: duplicate or absent {word}: {hits:?}",
                case["id"]
            )
        };
        assert_eq!(
            copied_hit(rendered, hit),
            word,
            "{} width={width}: original scalar",
            case["id"]
        );
        let expected_target =
            original_link(content, word).and_then(|target| view.activation_target(&target));
        for fragment in fragments(hit) {
            assert_eq!(
                rendered.link_target_at(fragment.row, fragment.start_column),
                expected_target.as_ref(),
                "{} width={width}: target",
                case["id"]
            );
            assert_eq!(
                rendered.link_target_at(fragment.row, fragment.end_column - 1),
                expected_target.as_ref()
            );
            let symbol = cells[(
                fragment.start_column.try_into().unwrap(),
                fragment.row.try_into().unwrap(),
            )]
                .symbol();
            assert_ne!(symbol, "");
            assert!(
                word.contains(symbol),
                "{word}: real viewport glyph {symbol}"
            );
        }
    }
    check_link_padding(rendered, &body);
}

fn check_link_padding(rendered: &RenderedDocument, body: &std::ops::Range<usize>) {
    for row in body.clone() {
        for (column, character) in rendered.text.lines[row].to_string().chars().enumerate() {
            if character == ' ' {
                assert_eq!(
                    rendered.link_target_at(row, column),
                    None,
                    "table padding cannot inherit a cell link"
                );
            }
        }
    }
}

fn expected_rows(case: &Value, width: u16) -> Vec<String> {
    if case["id"] == "advance" && width == 1200 {
        // Frozen device columns at 5/262/519, with UI's section origin three.
        // The native advance cap is source policy, independent of viewport.
        return strings(&case["native_rows"])
            .into_iter()
            .map(|row| row[2..].to_owned())
            .collect();
    }
    let key = format!("ui_rows_{width}");
    strings(case.get(&key).unwrap_or(&case["ui_rows_78"]))
}

fn check_view(case: &Value, content: &ResolvedContent) {
    let view = DocumentView::new(content);
    let initial = view.render(12).text;
    let widths = if case["id"] == "advance" {
        vec![12, 78, 120, 1200, 12]
    } else {
        vec![12, 78, 120, 12]
    };
    for width in widths {
        let rendered = view.render(width);
        check_source_cells(&view, content, case, &rendered, width);
        if width >= 78 {
            assert_eq!(
                body_rows(&rendered, case),
                expected_rows(case, width),
                "{} width={width}: declared reading policy {}",
                case["id"],
                case["reading_policy"]
            );
        }
    }
    assert_eq!(
        view.render(12).text,
        initial,
        "{}: resized cache",
        case["id"]
    );
}

#[test]
fn native_column_sources_survive_wire_real_buffer_resize_copy_and_clicks() {
    // Exact sources ran registered pristine in ASCII/UTF-8/HTML/tree/lint.
    // mdoc_term.c::termp_it_pre/post and term.c::term_flushln establish the
    // frozen native fields, excess capacity and completed row receipts.
    // Portable reading geometry is separately registered: signed actual
    // parent, child origins, no terminal soft margin, and viewport stacking.
    for case in cases() {
        let content = roundtrip(&case);
        let before = content.clone();
        check_view(&case, &content);
        assert_eq!(content, before);
    }
}

#[test]
fn cell_closed_graph_receipts_keep_actual_json_buffer_copy_and_resize_rows() {
    // Native term_vspace/term_flushln receipts were recorded for each exact
    // source in five profiles before these assertions. Cell breakAfter closes
    // occupied graph without creating an extra empty row; later source words
    // reopen the tail. UI consumes that same source-neutral JSON fact.
    let fixture: Value =
        serde_json::from_str(include_str!("column_preferences/cell_boundaries.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 17);
    for case in fixture["cases"].as_array().unwrap() {
        let content = roundtrip(case);
        check_view(case, &content);
    }
}

fn install_pure_ir_hints(content: &mut ResolvedContent) {
    let section = content
        .document
        .as_mut()
        .unwrap()
        .sections
        .iter_mut()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    let Block::Table { rows, .. } = &mut section.blocks[0] else {
        panic!("table")
    };
    let Block::Paragraph { inline_layout, .. } = &mut rows[0].cells[0].blocks[0] else {
        panic!("paragraph")
    };
    inline_layout.row_hints = [(0, -2), (1, 3), (2, -1)]
        .into_iter()
        .map(|(row, indent_columns)| mant_ir::RowLayoutHint {
            row,
            indent_columns,
        })
        .collect();
}

#[test]
fn explicit_owner_hint_json_is_separate_from_retired_native_ti_geometry() {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "hinted-hard-rows")
        .unwrap();
    let mut original = roundtrip(&case);
    install_pure_ir_hints(&mut original);
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let content: ResolvedContent = decoded.into();
    assert_eq!(content, original);
    let view = DocumentView::new(&content);
    for width in [12, 78, 120, 12] {
        let rendered = view.render(width);
        check_source_cells(&view, &content, &case, &rendered, width);
        if width >= 78 {
            assert_eq!(
                body_rows(&rendered, &case),
                ["      A", "           C", "       D", "        B"]
            );
        }
        // Narrow views retain the registered readable_origins minimum body
        // width policy. Source copy, search, links and viewport cells above
        // remain exact; signed owner origins are asserted at normal widths.
    }
    check_fenced_hint_readback(&content, &case);
}

fn check_fenced_hint_readback(content: &ResolvedContent, case: &Value) {
    // Fenced tables deliberately project positive row corrections to ASCII
    // cells. These derived literal cells are copyable; the source owner's
    // author text and signed row geometry above remain independent.
    // Fresh pristine lastmargin/hinted-hard-rows runs in all five profiles
    // keep the original native gold. termp_it_pre selects the last field's
    // margin, and roff_term_pre_ti executes its preceding break; neither rule
    // changes the separately frozen literal Markdown projection.
    check_addressable_hint_owner(content);
    let markdown = mant_codec::encode::render_markdown(content);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let section = readback
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    let [
        Block::Preformatted {
            children,
            inline_layout,
            ..
        },
    ] = section.blocks.as_slice()
    else {
        panic!("actual fenced payload: {:?}", section.blocks)
    };
    assert_eq!(mant_ir::inline_plain_text(children), "A\n   C\nD | B");
    assert!(inline_layout.is_empty());
    let view = DocumentView::new(&readback);
    let rendered = view.render(78);
    let cells = buffer(&rendered, 78);
    for (word, row_offset, column) in [("A", 0, 3), ("C", 1, 6), ("D", 2, 3), ("B", 2, 7)] {
        let hit = rendered
            .search(word)
            .into_iter()
            .find(|hit| {
                body_region(&rendered, case).contains(&hit.row)
                    && copied_hit(&rendered, hit) == word
            })
            .unwrap();
        assert_eq!(hit.row - body_region(&rendered, case).start, row_offset);
        assert_eq!(hit.start_column, column);
        assert_eq!(
            cells[(column.try_into().unwrap(), hit.row.try_into().unwrap())].symbol(),
            word
        );
    }
    let body = body_region(&rendered, case);
    let copied = rendered.selected_text(RenderedSelection {
        anchor: TextPosition {
            row: body.start,
            column: 0,
        },
        focus: TextPosition {
            row: body.end - 1,
            column: 7,
        },
    });
    // Visual-cell selection retains the section's existing three-cell margin;
    // source payload and exact word selection above remain independent.
    assert_eq!(copied, "   A\n      C\n   D | B");
    check_source_cells(&view, &readback, case, &rendered, 78);
}

fn check_addressable_hint_owner(content: &ResolvedContent) {
    let original = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    let artifact = mant_codec::encode::render_addressable_markdown(content);
    let mut mapped = false;
    for node in artifact.nodes() {
        if let mant_codec::encode::MarkdownNode::DocumentSection { section, .. } = node.node()
            && std::ptr::eq(artifact.section(*section).unwrap().section(), original)
        {
            assert!(!mapped, "one original section owner");
            assert!(artifact.text()[node.range()].contains("A\n   C\nD | B"));
            mapped = true;
        }
    }
    assert!(mapped);
}
