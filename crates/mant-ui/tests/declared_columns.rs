//! Declared field origins survive JSON and viewport changes. Exact native
//! inputs were recorded before these assertions; `mdoc_term.c::termp_it_pre`
//! measures the declaration, not the styled/UTF-8 spelling of the cell.
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, widgets::Widget};

#[path = "support/reader_actions.rs"]
mod reader_actions;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

const PRE: &str = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n";

#[test]
fn declared_origins_keep_unicode_links_and_search_coordinates_after_json() {
    for first in ["Sy A", "中", "e\u{301}", "😀"] {
        let source = format!(
            "{PRE}.Bl -column \"12345678\" \"b\"\n.It {first} Ta Lk https://e.example/x SECOND\n.El\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let decoded: mant_ir::ResolvedContent = decoded.into();
        let view = DocumentView::new(&decoded);
        for width in [80, 120, 16, 8, 80] {
            let rendered = view.render(width);
            let found = rendered.search("SECOND");
            assert_eq!(found.len(), 1, "{first} at {width}: {:?}", rendered.text);
            let second = &found[0];
            if width >= 80 {
                // The measured eight-cell declaration plus dcol four.
                let left = if first == "Sy A" { "A" } else { first };
                let left = rendered
                    .search(left)
                    .into_iter()
                    .filter(|hit| hit.row == second.row && hit.start_column < second.start_column)
                    .collect::<Vec<_>>();
                assert_eq!(
                    left.len(),
                    1,
                    "{first}: identify the first cell, not URI text"
                );
                assert_eq!(second.row, left[0].row);
                assert_eq!(second.start_column - left[0].start_column, 12);
            }
            assert!(!rendered.text.to_string().contains('\u{fffd}'));
            let height = u16::try_from(rendered.text.lines.len()).unwrap();
            let area = Rect::new(0, 0, width, height);
            let mut buffer = Buffer::empty(area);
            rendered.text.clone().render(area, &mut buffer);
            if width >= 80 {
                for (index, expected) in "SECOND".chars().enumerate() {
                    assert_eq!(
                        buffer[(
                            u16::try_from(second.start_column + index).unwrap(),
                            u16::try_from(second.row).unwrap()
                        )]
                            .symbol(),
                        expected.to_string()
                    );
                }
            }
        }
        assert_column_pointer_and_copy(&decoded);
    }
}

fn column_label_position(buffer: &Buffer) -> (u16, u16) {
    // Navigation entries occur to the left of the body; actual terminal
    // cells determine the clickable range, rather than sidebar labels.
    positions(buffer, "SECOND")
        .into_iter()
        .max_by_key(|(column, _)| *column)
        .unwrap()
}

fn assert_column_pointer_and_copy(query: &mant_ir::ResolvedContent) {
    for width in [80, 128] {
        let mut app = App::new(query);
        let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = column_label_position(terminal.backend().buffer());
        click(&mut app, column, row);
        let activated = opened_targets(&mut app);
        assert_eq!(activated, ["https://e.example/x"], "column link at {width}");

        select_span(&mut app, column, row, column + 5, None);
        let copied = copied_selections(&mut app, |request| {
            let CopyRequest::Selection { text } = request else {
                panic!("column selection must copy its visual glyphs");
            };
            text
        });
        assert_eq!(copied, ["SECOND"], "column copy at {width}");
    }
}

#[test]
fn responsive_table_geometry_keeps_native_owners_rules_and_buffer_coordinates() {
    // Exact pristine ASCII/UTF-8/HTML inputs precede these assertions.
    // mdoc_term.c retains the nested tag owner; tbl_term.c distinguishes an
    // authored horizontal rule from optional device frame decoration.
    for (name, tokens) in [
        ("cw10_list", &["A", "x", "B", "C"][..]),
        ("cw12_box", &["A", "B"][..]),
        ("cw12_span", &["A", "B", "C"][..]),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mant-engine/tests/roff_lowering/macro_consumer_matrix/cases")
            .join(format!("{name}.1"));
        let query = mant_loader::load_roff_bytes(&std::fs::read(path).unwrap()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let view = DocumentView::new(&decoded.into());
        for width in [80, 120, 8, 80] {
            let rendered = view.render(width);
            let start = rendered.anchor_row("description").unwrap();
            let end = rendered.anchor_row("next").unwrap();
            let hits = tokens
                .iter()
                .map(|token| {
                    let hits = rendered
                        .search(token)
                        .into_iter()
                        .filter(|hit| hit.row > start && hit.row < end)
                        .collect::<Vec<_>>();
                    assert_eq!(
                        hits.len(),
                        1,
                        "{name}/{token} at {width}: {:?}",
                        rendered.text
                    );
                    hits.into_iter().next().unwrap()
                })
                .collect::<Vec<_>>();
            assert!(hits.windows(2).all(|pair| {
                (pair[0].row, pair[0].start_column) < (pair[1].row, pair[1].start_column)
            }));
            if name == "cw10_list" {
                assert_eq!(rendered.anchor_row("term-x"), Some(hits[1].row));
                assert_eq!(
                    hits[2].row - hits[1].row,
                    1,
                    "native closed HEAD {name} at {width}"
                );
                // Exact termp_it_post/term_flushln: the nested list closed B,
                // then the outer empty column flush emitted an empty row.
                assert_eq!(hits[3].row - hits[2].row, 2, "{name} at {width}");
            }
            if name == "cw12_span" {
                let rules = rendered.text.lines[hits[0].row + 1..hits[1].row]
                    .iter()
                    .map(ToString::to_string)
                    .filter(|row| {
                        let row = row.trim();
                        !row.is_empty() && row.chars().all(|ch| matches!(ch, '-' | '─'))
                    })
                    .count();
                assert_eq!(rules, 1, "authored rule {name} at {width}");
            }
            let area = Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
            let mut buffer = Buffer::empty(area);
            rendered.text.clone().render(area, &mut buffer);
            for (token, hit) in tokens.iter().zip(&hits) {
                assert_eq!(
                    buffer[(
                        u16::try_from(hit.start_column).unwrap(),
                        u16::try_from(hit.row).unwrap()
                    )]
                        .symbol(),
                    *token,
                    "{name}/{token} buffer at {width}"
                );
            }
        }
    }
}

#[test]
fn column_row_fit_uses_printed_extent_without_discarding_fixed_spaces() {
    // The exact same September 8 source/header was recorded with pristine
    // CVS before these assertions: ASCII separator padding is not printed,
    // whereas fixed blanks keep graph extent in term_fill/term_field.
    let pre = ".Dd September 8, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n";
    for (operand, first, fits) in [
        ("AAAA B", "AAAA B", true),
        ("AAAA BB", "AAAA BB", false),
        ("AAAA B\\~", "AAAA B", false),
        ("AAAA B\\0", "AAAA B", false),
        ("Em \"AAAA B\"", "AAAA B", true),
        ("Sy \"AAAA B\"", "AAAA B", true),
        ("中中 B", "中中 B", true),
        ("中中 BB", "中中 BB", false),
        ("😀😀 B", "😀😀 B", true),
        ("😀😀 BB", "😀😀 BB", false),
    ] {
        let source = format!("{pre}.Bl -column AAA BBB -compact\n.It {operand} Ta C\n.El\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let view = DocumentView::new(&decoded.into());
        for width in [80, 120, 16, 8, 80] {
            let rendered = view.render(width);
            let heading = rendered.anchor_row("description").unwrap();
            let first = rendered.search(first);
            assert_eq!(first.len(), 1, "{operand} at {width}");
            let next = rendered
                .search("C")
                .into_iter()
                .filter(|hit| hit.row > heading)
                .collect::<Vec<_>>();
            assert_eq!(next.len(), 1, "{operand} at {width}");
            if width >= 16 {
                assert_eq!(
                    next[0].row,
                    first[0].row + usize::from(!fits),
                    "{operand} at {width}: {:?}",
                    rendered.text
                );
                assert_eq!(
                    next[0].start_column,
                    first[0].start_column + 7,
                    "{operand} at {width}"
                );
            } else {
                // At eight cells the composed table margin leaves no room
                // for origin seven; the declared contract stacks actual cells.
                assert!(next[0].row >= first[0].row);
            }
            let area = Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
            let mut buffer = Buffer::empty(area);
            rendered.text.clone().render(area, &mut buffer);
            assert_eq!(
                buffer[(
                    u16::try_from(next[0].start_column).unwrap(),
                    u16::try_from(next[0].row).unwrap()
                )]
                    .symbol(),
                "C"
            );
        }
    }
}

#[test]
fn column_post_rows_survive_json_and_wide_or_narrow_tui() {
    // Exact pristine ASCII/UTF-8/tree: termp_it_post() calls term_flushln
    // even when Bd already ended its row. The last column's cleared NOBREAK
    // therefore emits one further empty device row (term.c:233-253).
    let pre =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (body, row_delta) in [
        (
            ".Bl -column X -compact\n.It\n.Bd -literal\nINNER\n.Ed\n.El\nAFTER\n",
            2,
        ),
        (
            ".Bl -column X -compact\n.It\n.Bd -literal -compact\nINNER\n.Ed\n.El\nAFTER\n",
            2,
        ),
        (
            ".Bl -column X -compact\n.It\n.Bd -filled\nINNER\n.Ed\n.El\nAFTER\n",
            2,
        ),
        (
            ".Bl -column X -compact\n.It A\n.Bd -literal\nINNER\n.Ed\n.El\nAFTER\n",
            2,
        ),
        (
            ".Bl -column X -compact\n.It A\n.Bd -literal -compact\nINNER\n.Ed\n.El\nAFTER\n",
            2,
        ),
        (".Bl -column X -compact\n.It INNER\n.sp 1\n.El\nAFTER\n", 3),
        (".Bl -column X -compact\n.It INNER\n.El\nAFTER\n", 1),
    ] {
        let source = format!("{pre}{body}.Sh NEXT\n.No END\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let decoded: mant_ir::ResolvedContent = decoded.into();
        let view = DocumentView::new(&decoded);
        for width in [80, 120, 16, 8, 80] {
            let rendered = view.render(width);
            let inner = rendered.search("INNER");
            let after = rendered.search("AFTER");
            assert_eq!(inner.len(), 1, "{source}");
            assert_eq!(after.len(), 1, "{source}");
            assert_eq!(
                after[0].row - inner[0].row,
                row_delta,
                "{source} at {width}: {:?}",
                rendered.text
            );
            for row in &rendered.text.lines[inner[0].row + 1..after[0].row] {
                assert!(row.to_string().trim().is_empty(), "{source}");
            }
        }
    }
}

#[test]
fn column_device_tail_closes_only_an_overrun_occupied_row() {
    // Exact pristine ASCII/UTF-8/tree runs precede these assertions.
    // termp_bd_post leaves the first column's viscol live; empty It BODY
    // post compares trailspace against the remaining vfield + half an en
    // (term.c:233-253). INNER closes its occupied row; I/INNE do not.
    let pre =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (word, wide_delta) in [("INNER", 1), ("I", 0), ("INNE", 0)] {
        let source = format!(
            "{pre}.Bl -column XX YY -compact\n.It\n.Bd -literal -compact\n{word}\n.Ed\n.Ta FINAL\n.El\nAFTER\n.Sh NEXT\n.No END\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let decoded: mant_ir::ResolvedContent = decoded.into();
        let view = DocumentView::new(&decoded);
        for width in [80, 120, 16, 8, 80] {
            let rendered = view.render(width);
            let section = rendered.anchor_row("description").unwrap();
            let final_hits = rendered.search("FINAL");
            let after = rendered.search("AFTER");
            assert_eq!(final_hits.len(), 1, "{source} at {width}");
            assert_eq!(after.len(), 1, "{source} at {width}");
            let final_hit = &final_hits[0];
            let before = rendered
                .search(word)
                .into_iter()
                .filter(|hit| {
                    hit.row > section
                        && (hit.row, hit.start_column) < (final_hit.row, final_hit.start_column)
                        // A narrow DESCRIPTION wraps into ION; identify the
                        // actual literal span rather than that heading hit.
                        && rendered.text.lines[hit.row].spans.iter().any(|span| {
                            span.content == word && span.style.bg.is_some()
                        })
                })
                .collect::<Vec<_>>();
            assert_eq!(before.len(), 1, "{source} at {width}: {:?}", rendered.text);
            if width >= 16 {
                assert_eq!(
                    final_hit.row - before[0].row,
                    wide_delta,
                    "{source} at {width}: {:?}",
                    rendered.text
                );
            }
            assert_eq!(
                after[0].row - final_hit.row,
                1,
                "{source} at {width}: {:?}",
                rendered.text
            );
        }
    }
}
