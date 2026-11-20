//! Closed table cells retain source positions and never invent blank rows.

use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

mod field_replay;

const REVIEW_HEADER: &str = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd exercise descriptors\n.Sh DESCRIPTION\n";

fn restored(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = mant_render::render_query_json(&query, false).unwrap();
    serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into()
}

fn paint(document: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, document.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(document.text.clone()).render(area, &mut buffer);
    buffer
}

fn painted(buffer: &Buffer, row: usize, start: usize, end: usize) -> String {
    (start..end)
        .map(|col| buffer[(col.try_into().unwrap(), row.try_into().unwrap())].symbol())
        .collect()
}

#[test]
fn native_slot_writes_preserve_painting_copy_search_and_link_hits() {
    // Exact inputs ran the pinned CVS terminal profiles. term_word()
    // changes slots before term_fill() decides whether to keep NEEDLE.
    let header = ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- sample\n.SH DESCRIPTION\n";
    let target = LinkTarget::External(ExternalUri::parse("https://example.test/r").unwrap());
    for (open, close) in [
        ("", ""),
        (".TS\nl.\n", ".TE\n"),
        (".UR https://example.test/r\n", ".UE\n"),
    ] {
        for (operation, keeps_needle) in [
            ("P", true),
            (r"\l'1n '", false),
            (r"P\l'1n '\h'0n'\h'|5n'", true),
            (r"\l'1n.'\h'-1n'", false),
            (r"Q\l'1n '\h'-1n'", true),
            (r"START\p \o' '", true),
        ] {
            let source =
                format!("{header}{open}{operation}\\p NEEDLE\nTAILWORD\n{close}.PP\nENDTEXT\n");
            let view = DocumentView::new(&restored(&source));
            for width in [24, 78] {
                let rendered = view.render(width);
                let buffer = paint(&rendered, width);
                let shown = (0..rendered.row_count)
                    .map(|row| {
                        painted(&buffer, row, 0, width.into())
                            .matches("NEEDLE")
                            .count()
                    })
                    .sum::<usize>();
                let copy = rendered.selected_text(RenderedSelection {
                    anchor: TextPosition { row: 0, column: 0 },
                    focus: TextPosition {
                        row: rendered.row_count - 1,
                        column: usize::from(width) - 1,
                    },
                });
                let hits = rendered.search("NEEDLE");
                assert_eq!(
                    [shown, copy.matches("NEEDLE").count(), hits.len()],
                    [usize::from(keeps_needle); 3],
                    "{source} at {width}"
                );
                let expected_target = (close == ".UE\n").then_some(&target);
                for hit in hits {
                    for column in hit.start_column..hit.end_column {
                        assert_eq!(
                            rendered.link_target_at(hit.row, column),
                            expected_target,
                            "{source}"
                        );
                    }
                }
                assert_eq!(rendered.search("ENDTEXT").len(), 1);
            }
        }
    }
}

#[test]
fn active_native_words_paint_and_copy_the_accepted_rows() {
    // term.c::encode1 writes one or more raw slots per glyph. Later
    // ESCAPE_HORIZ can reveal earlier ink or overwrite a decorated copy
    // before term_fill accepts the row, across a marker and link controls.
    let link = LinkTarget::External(ExternalUri::parse("https://one.test").unwrap());
    for (word, rows, label) in [
        (r#".No "P \zMA" No "\h'-3n'T""#, &["P MTA"][..], None),
        (r#".No "\zMA\zBC" No "\h'-3n'T""#, &["ABTC"][..], None),
        (r#".Sy "A\pB" No "\h'-2n'T""#, &["AT"][..], None),
        (r#".No "A\h'2n'B" No "\h'-2n'T""#, &["A  T"][..], None),
        (r#".No "A\l'1n 'B" No "\h'-2n'T""#, &["A T"][..], None),
        (
            r#".Sy "P\X'tty: link https://one.test'M\pA\X'tty: link'B" No "T" Ns No "\h'-3n'Y""#,
            &["PMAY", "T"][..],
            Some("MA"),
        ),
        (
            r#".Sy "P\h'2n'\X'tty: link https://one.test'M\X'tty: link'AB" No "\h'-2n'T""#,
            &["P  MAT"][..],
            Some("M"),
        ),
        (
            r#".No "\zA\p\X'tty: link https://one.test'B\X'tty: link'" No "\h'-3n'T""#,
            &["T"][..],
            None,
        ),
        (r#".No "A\zB\pC" No "\h'-2n'T""#, &["AT"][..], None),
        (
            r#".No "\pAB" No T Ns No "\h'-3n'Y""#,
            &["AY", "T"][..],
            None,
        ),
        (
            r#".No "A\pB\:C" No T Ns No "\h'-3n'Y""#,
            &["AB", "Y T"][..],
            None,
        ),
        (
            r#".No "A\h'65n'B" Ns No "DC" No "\h'-2n'X""#,
            &["A BDX"][..],
            None,
        ),
    ] {
        let source = format!("{REVIEW_HEADER}{word}\n");
        assert_native_word_appearance(&source, rows, label, &link);
    }
}

#[test]
fn marker_rewinds_keep_painted_words_and_remove_overwritten_link_hits() {
    // Exact inputs ran pinned CVS. term_word leaves both \p cells live
    // until the later rewind, including the native cells of a link label.
    let head = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    let link = LinkTarget::External(ExternalUri::parse("https://one.test").unwrap());
    for (word, rows) in [
        (r#".No "P\p" Ns No "\pA" No "\h'-4n'T""#, &["PTA"][..]),
        (r#".No "P\p" Ns No "\pA" No "\h'-5n'T""#, &["TA"][..]),
        (
            r#".No "A\p \X'tty: link https://one.test'B\X'tty: link'" No "\h'-3n'T""#,
            &["A", "T"][..],
        ),
        (r#".No "A\p\h'2n'B" No "\h'-2n'T""#, &["A  T"][..]),
    ] {
        assert_native_word_appearance(&format!("{head}{word}\n"), rows, None, &link);
    }
}

#[test]
fn live_subsection_rows_preserve_only_the_real_gap_when_painted_and_copied() {
    // pre_SS with PD=0 and post_SH::term_newln share the old field.
    // The exact source forms distinguish its end from an empty HEAD pass.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
    let tail = "\nGG\n.PD 1\n.SH ENDTEST\nZZ\n";
    for (word, heading, gap) in [
        (r"AA\p\~BB", "DD", 0),
        (r"AA\p\:BB", r"\&\p DD", 0),
        (r"AA\p\~BB", r"\&\p DD", 1),
    ] {
        let source = format!("{head}{word}\n.SS {heading}{tail}");
        let view = DocumentView::new(&restored(&source));
        for width in [24, 78] {
            let rendered = view.render(width);
            let before = rendered.search("BB")[0].row;
            let title = rendered.search("DD")[0].row;
            let after = rendered.search("GG")[0].row;
            assert_eq!((title - before, after - title), (1 + gap, 1), "{source}");
            let buffer = paint(&rendered, width);
            assert_eq!(painted(&buffer, title, 0, width.into()).trim(), "DD");
            let copied = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: before,
                    column: 0,
                },
                focus: TextPosition {
                    row: after,
                    column: usize::from(width) - 1,
                },
            });
            let expected = if gap == 0 {
                "BB\nDD\nGG"
            } else {
                "BB\n\nDD\nGG"
            };
            let copied = copied.replace("\u{a0}", " ");
            assert!(
                copied
                    .lines()
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .ends_with(expected)
            );
        }
    }
}

fn assert_native_word_appearance(
    source: &str,
    rows: &[&str],
    label: Option<&str>,
    link: &LinkTarget,
) {
    let view = DocumentView::new(&restored(source));
    for width in [24, 78] {
        let rendered = view.render(width);
        let first = rendered.search("DESCRIPTION")[0].row + 1;
        let buffer = paint(&rendered, width);
        for (offset, &row) in rows.iter().enumerate() {
            assert_eq!(
                painted(&buffer, first + offset, 0, width.into()).trim(),
                row,
                "{source}: {width}"
            );
            let hits = rendered.search(row);
            assert!(
                hits.iter().any(|hit| hit.row == first + offset),
                "{source}: {row}"
            );
        }
        let selected = rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: first,
                column: 0,
            },
            focus: TextPosition {
                row: first + rows.len() - 1,
                column: usize::from(width) - 1,
            },
        });
        assert_eq!(
            selected.lines().map(str::trim).collect::<Vec<_>>(),
            rows,
            "{source}"
        );
        if let Some(label) = label {
            let hit = rendered
                .search(label)
                .into_iter()
                .find(|hit| hit.row == first)
                .unwrap();
            for column in hit.start_column..hit.end_column {
                assert_eq!(
                    rendered.link_target_at(first, column),
                    Some(link),
                    "{source}"
                );
            }
            assert_eq!(
                rendered.link_target_at(first, hit.end_column),
                None,
                "{source}"
            );
        } else {
            for offset in 0..rows.len() {
                for column in 0..width.into() {
                    assert_eq!(
                        rendered.link_target_at(first + offset, column),
                        None,
                        "{source}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_native_table_inside_an_extended_definition_head_paints_every_cell() {
    let header = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
    // CVS term_tbl emits both cells inside Xo, before the original It BODY.
    for tag in ["", ".Tg Table.Point\n"] {
        let source = format!(
            "{header}.Bl -tag -width Ds\n.It Xo\n.No PRE\n{tag}.TS\ntab(|);\nl l.\nXYZ|SECOND\n.TE\n.No AFTER\n.Xc\nEND\n.El\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let view = DocumentView::new(&query);
        for width in [18, 78] {
            let rendered = view.render(width);
            let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
            let mut buffer = Buffer::empty(area);
            Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
            let mut previous = (0, 0);
            for text in ["PRE", "XYZ", "SECOND", "AFTER", "END"] {
                let hits = rendered.search(text);
                assert_eq!(hits.len(), 1, "{source}: {text}");
                let hit = &hits[0];
                let painted: String = (hit.start_column..hit.end_column)
                    .map(|col| {
                        buffer[(col.try_into().unwrap(), hit.row.try_into().unwrap())].symbol()
                    })
                    .collect();
                assert_eq!(painted, text);
                let location = (hit.row, hit.start_column);
                assert!(location >= previous, "{source}: {text}");
                previous = location;
            }
            assert_eq!(
                view.validate_fragment("Table.Point").is_ok(),
                !tag.is_empty()
            );
        }
    }
}

#[test]
fn head_table_rows_and_prefix_words_survive_narrow_and_wide_paint() {
    // The engine row matrix ran these exact sources against pristine CVS.
    for style in ["tag", "hang"] {
        for prefix in ["", ".No PRE\n", ".No PRE\n.br\n"] {
            for blanks in 0..=2 {
                let blank_rows = "\\&\n".repeat(blanks);
                let source = format!(
                    "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n{prefix}.TS\nl.\nFIRST\n{blank_rows}LAST\n.TE\n.No AFTER\n.Xc\nENDING\n.El\n.Sh EXAMPLES\nTAIL\n"
                );
                let query = restored(&source);
                let view = DocumentView::new(&query);
                for width in [14, 78, 14] {
                    let rendered = view.render(width);
                    let raw = paint(&rendered, width);
                    let first = &rendered.search("FIRST")[0];
                    let last = &rendered.search("LAST")[0];
                    assert_eq!(last.row - first.row, blanks + 1, "{source}: {width}");
                    assert!(
                        (first.row + 1..last.row).all(|row| painted(
                            &raw,
                            row,
                            0,
                            usize::from(width)
                        )
                        .trim()
                        .is_empty()),
                        "{source}: {width}"
                    );
                    for (word, hit) in [("FIRST", first), ("LAST", last)] {
                        assert_eq!(
                            painted(&raw, hit.row, hit.start_column, hit.end_column),
                            word
                        );
                    }
                    let prefixes = rendered.search("PRE");
                    assert_eq!(prefixes.len(), usize::from(!prefix.is_empty()));
                    if let Some(pre) = prefixes.first()
                        && width == 78
                    {
                        let split = style == "tag" && prefix.contains(".br");
                        assert_eq!(pre.row + usize::from(split), first.row, "{source}");
                        if !split {
                            assert!(first.start_column > pre.end_column, "{source}");
                            assert_eq!(
                                painted(&raw, first.row, pre.start_column, first.end_column),
                                "PRE FIRST",
                                "{source}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn visible_cell_alignment_places_the_same_word_boundary_in_paint_and_copy() {
    // The exact native engine matrix checks l/c/r with one or both slots
    // and a HANG prefix at less than, equal to, or beyond its field width.
    for (options, kind, field, second, expected) in [
        ("", "l", "4n", false, "LONGPREFIXXYZ"),
        ("", "c", "4n", false, "LONGPREFIX XYZ"),
        ("", "r", "4n", false, "LONGPREFIX XYZ"),
        ("", "l", "8n", true, "LONGPREFIX XYZ"),
        ("center;\n", "l", "4n", false, "LONGPREFIX XYZ"),
        ("", "a", "8n", false, "LONGPREFIXXYZ"),
    ] {
        let layout = if second {
            format!("l {kind}")
        } else {
            kind.to_owned()
        };
        let words = if second {
            "\\&\tXYZ\nA\tWIDESTCELL"
        } else {
            "XYZ\nWIDESTCELL"
        };
        let source = format!(
            "{REVIEW_HEADER}.Bl -hang -width {field}\n.It Xo\n.No LONGPREFIX\n.br\n.TS\n{options}{layout}.\n{words}\n.TE\n.Xc\nENDING\n.El\n.Sh EXAMPLES\nTAIL\n"
        );
        let query = restored(&source);
        let view = DocumentView::new(&query);
        for width in [24, 78, 24] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let matches = rendered.search(expected);
            let [hit] = matches.as_slice() else {
                panic!("{source}: {width}");
            };
            assert_eq!(
                painted(&buffer, hit.row, hit.start_column, hit.end_column),
                expected
            );
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
                expected
            );
        }
    }
}

#[test]
fn table_opening_preserves_painted_and_copied_row_boundaries() {
    // Fixed CVS tbl_term places border spacing/frame before the first cell.
    // The portable term omits its drawings while keeping the actual endline.
    for (options, layout, first, second, joined) in [
        ("", "l", "XYZ", "WIDESTCELL", Some("LONGPREFIXXYZ")),
        ("", "c", "XYZ  ", "WIDESTCELL", Some("LONGPREFIX XYZ")),
        ("", "c", r"XYZ\~\~", "WIDESTCELL", Some("LONGPREFIXXYZ")),
        ("", "r", "XYZ  ", "WWWWWWWW", Some("LONGPREFIX XYZ")),
        ("", "|l", "XYZ", "WIDESTCELL", None),
        ("", "l|", "XYZ", "WIDESTCELL", None),
        ("box;\n", "l", "XYZ", "WIDESTCELL", None),
        ("allbox;\n", "l", "XYZ", "WIDESTCELL", None),
    ] {
        let source = format!(
            "{REVIEW_HEADER}.Bl -hang -width 4n\n.It Xo\n.No LONGPREFIX\n.br\n.TS\n{options}{layout}.\n{first}\n{second}\n.TE\n.Xc\nENDING\n.El\n.Sh EXAMPLES\nTAIL\n"
        );
        let query = restored(&source);
        let view = DocumentView::new(&query);
        for width in [24, 78, 24] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let prefix = &rendered.search("LONGPREFIX")[0];
            let cell = &rendered.search("XYZ")[0];
            assert_eq!(
                cell.row - prefix.row,
                usize::from(joined.is_none()),
                "{source}"
            );
            if let Some(joined) = joined {
                assert_eq!(
                    painted(&buffer, cell.row, prefix.start_column, cell.end_column),
                    joined,
                    "{source}"
                );
            } else {
                assert_eq!(
                    painted(&buffer, cell.row, cell.start_column, cell.end_column),
                    "XYZ"
                );
            }
            let copied = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: prefix.row,
                    column: prefix.start_column,
                },
                focus: TextPosition {
                    row: cell.row,
                    column: cell.end_column - 1,
                },
            });
            let rows = copied
                .lines()
                .map(|line| line.trim_matches(' '))
                .collect::<Vec<_>>();
            if let Some(joined) = joined {
                assert_eq!(rows, [joined], "{source}");
            } else {
                assert_eq!(rows, ["LONGPREFIX", "XYZ"], "{source}");
            }
        }
    }
}

#[test]
fn cell_pending_word_end_break_never_adds_another_painted_row() {
    // Exact fixed-CVS rows separate accepted internal \p and the data row
    // end, including a real line holding only a nonbreaking blank.
    for (first, word, rows) in [
        (r"AA\pBB", "AABB", 2),
        (r"AA\p BB", "AA", 3),
        (r"AA\pBB\p", "AABB", 2),
        (r"AA\p  ", "AA", 2),
        (r"AA\p \&", "AA", 2),
        (r"AA\p \~", "AA", 3),
    ] {
        let source = format!(
            "{REVIEW_HEADER}.Bl -tag -width 4n\n.It Xo\n.TS\nl.\nFIRST\n{first}\nLAST\n.TE\n.Xc\nENDING\n.El\n.Sh EXAMPLES\nTAIL\n"
        );
        let query = restored(&source);
        let view = DocumentView::new(&query);
        for width in [20, 78, 20] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let start = &rendered.search("FIRST")[0];
            let middle = &rendered.search(word)[0];
            let end = &rendered.search("LAST")[0];
            assert_eq!(middle.row - start.row, 1, "{source}");
            assert_eq!(end.row - start.row, rows, "{source}");
            assert_eq!(
                painted(&buffer, middle.row, middle.start_column, middle.end_column),
                word
            );
            let copied = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: start.row,
                    column: start.start_column,
                },
                focus: TextPosition {
                    row: end.row,
                    column: end.end_column - 1,
                },
            });
            assert_eq!(copied.lines().count(), rows + 1, "{source}: {copied:?}");
        }
    }
}

#[test]
fn native_table_passes_keep_completed_empty_rows_in_paint_and_copy() {
    // These five exact sources were checked in fixed CVS. A rejected
    // second data pass can be a physical row even with no glyphs.
    const HEADER: &str = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n";
    for (source, before, after, distance) in [
        (
            format!(
                "{HEADER}.Bl -hang -width 4n\n.It Xo\n.TS\nl.\nSTART\\p \\p XYZ\nLAST\n.TE\n.Xc\nENDING\n.El\n"
            ),
            "START",
            "LAST",
            2,
        ),
        (
            format!(
                "{HEADER}.Bl -tag -width 4n\n.It TAG\n.TS\nl.\nSTART\\p \\p XYZ\nLAST\n.TE\nENDING\n.El\n"
            ),
            "START",
            "LAST",
            2,
        ),
        (
            format!("{HEADER}.TS\nl.\nSTART\\p\\: \\&\nLAST\n.TE\nENDING\n"),
            "START",
            "LAST",
            1,
        ),
        (
            format!("{HEADER}LONGPREFIX\n.br\n.TS\nl|l.\nXYZ\tA\nLAST\tB\n.TE\nENDING\n"),
            "LONGPREFIX",
            "XYZ",
            2,
        ),
        (
            format!("{HEADER}LONGPREFIX\n.sp -1\n.TS\nl|l.\nXYZ\tA\nLAST\tB\n.TE\nENDING\n"),
            "LONGPREFIX",
            "XYZ",
            1,
        ),
    ] {
        let query = restored(&source);
        let view = DocumentView::new(&query);
        for width in [24, 78, 24] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let (a, b) = (&rendered.search(before)[0], &rendered.search(after)[0]);
            assert_eq!(b.row - a.row, distance, "{source}");
            assert_eq!(
                painted(&buffer, a.row, a.start_column, a.end_column),
                before
            );
            assert_eq!(painted(&buffer, b.row, b.start_column, b.end_column), after);
            let copied = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: a.row,
                    column: a.start_column,
                },
                focus: TextPosition {
                    row: b.row,
                    column: b.end_column - 1,
                },
            });
            assert_eq!(copied.lines().count(), distance + 1, "{source}\n{copied:?}");
        }
    }
}

#[test]
fn native_cell_motion_preserves_the_prefix_seam_in_paint_and_copy() {
    // Exact E3 sources under fixed CVS c/r alignment. The reading text
    // keeps XYZ and Q while device writes only decide the prefix seam.
    const HEADER: &str = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n";
    for (request, layout, data, expected) in [
        (
            ".br",
            "c",
            "XYZ\\h'3n'\\h'-2n'Q\nWWWWWWWWWWWW",
            "LONGPREFIX XYZ",
        ),
        (".br", "r", "XYZ\\h'|5n'\nWWWWWWWWW", "LONGPREFIXXYZ"),
        (".br", "c", "XYZ\\l'3n'\nWWWWWWWWWW", "LONGPREFIXXYZ"),
        (".sp -1", "l0 l", "  \tXYZ\n1\tQ", "LONGPREFIXXYZ"),
        (
            ".br",
            "c",
            r"XYZ\o'ab'\h'|6n'
WWWWWWWWWW",
            "LONGPREFIX XYZb",
        ),
        (
            ".br",
            "c",
            r"XYZ\o'ab'\h'|6n'
WWWWWWWWW",
            "LONGPREFIXXYZb",
        ),
        (".sp -1", "l0 l", "\\l'2n.'\tXYZ\n1\tQ", "LONGPREFIX XYZ"),
        (".sp -1", "l0 l", "\\l'2n '\tXYZ\n1\tQ", "LONGPREFIXXYZ"),
    ] {
        let source = format!(
            "{HEADER}.Bl -hang -width 4n\n.It Xo\n.No LONGPREFIX\n{request}\n.TS\n{layout}.\n{data}\n.TE\n.Xc\nENDING\n.El\n"
        );
        let query = restored(&source);
        let view = DocumentView::new(&query);
        for width in [24, 78, 24] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let hits = rendered.search(expected);
            let [hit] = hits.as_slice() else {
                panic!("{source}: width {width}");
            };
            assert_eq!(
                painted(&buffer, hit.row, hit.start_column, hit.end_column),
                expected
            );
            let copied = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column,
                },
                focus: TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1,
                },
            });
            assert_eq!(copied, expected, "{source}");
        }
    }
}

#[test]
fn source_backed_head_cell_keeps_its_exact_click_and_copy_parts() {
    // Fixed tbl_cdata witnesses the same raw operand in either form. Only
    // the source-backed T{} cell recovers the Mt semantic link.
    for (cell, clickable) in [
        ("a@example.test", false),
        ("T{\n.Mt a@example.test\nT}", true),
    ] {
        let source = format!(
            "{REVIEW_HEADER}.Bl -tag -width Ds\n.It Xo\n.TS\nl.\n{cell}\n.TE\n.Xc\nOWNER\n.El\n"
        );
        let query = restored(&source);
        let view = DocumentView::new(&query);
        for width in [16, 78, 16] {
            let rendered = view.render(width);
            assert_eq!(!rendered.links.is_empty(), clickable, "{source}: {width}");
            if clickable {
                let buffer = paint(&rendered, width);
                let label = rendered
                    .links
                    .iter()
                    .map(|region| {
                        assert!(
                            matches!(rendered.link_target_at(region.row, region.start_column),
                        Some(LinkTarget::External(uri)) if uri.as_str() == "mailto:a@example.test")
                        );
                        let part =
                            painted(&buffer, region.row, region.start_column, region.end_column);
                        assert_eq!(
                            rendered.selected_text(RenderedSelection {
                                anchor: TextPosition {
                                    row: region.row,
                                    column: region.start_column,
                                },
                                focus: TextPosition {
                                    row: region.row,
                                    column: region.end_column - 1,
                                },
                            }),
                            part
                        );
                        part
                    })
                    .collect::<String>();
                assert_eq!(label, "a@example.test", "{source}: {width}");
                let owner = &rendered.search("OWNER")[0];
                assert!(
                    rendered
                        .link_target_at(owner.row, owner.start_column)
                        .is_none()
                );
            }
        }
    }
}

fn cell(blocks: Vec<Block>, break_after: bool) -> TableCell {
    TableCell {
        preferred_width_columns: None,
        blocks,
        break_after,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

fn table(cells: Vec<TableCell>, preferences: mant_ir::ColumnPreferences) -> Block {
    Block::Table {
        borders: mant_ir::TableBorders::default(),
        alignment: None,
        column_count: None,
        rows: vec![TableRow {
            column_borders: vec![],
            kind: mant_ir::TableRowKind::Data,
            cells,
        }],
        column_preferences: preferences,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn literal(value: &str) -> Block {
    Block::Preformatted {
        children: vec![Inline::Text {
            value: value.into(),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn preferences() -> Vec<mant_ir::ColumnPreferences> {
    vec![
        mant_ir::ColumnPreferences::default(),
        mant_ir::ColumnPreferences {
            widths: vec![8, 8],
            ..Default::default()
        },
        mant_ir::ColumnPreferences {
            widths: vec![u16::MAX, 1],
            ..Default::default()
        },
        mant_ir::ColumnPreferences {
            gap_columns: u16::MAX,
            ..Default::default()
        },
    ]
}

fn assert_distance(block: Block, distance: usize) {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![block];
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    for width in [20, 80, u16::MAX] {
        let rendered = DocumentView::new(&restored).render(width);
        let first = &rendered.search("FIRST")[0];
        let second = &rendered.search("SECOND")[0];
        assert_eq!(second.row - first.row, distance, "width={width}");
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: first.row,
                    column: first.start_column
                },
                focus: TextPosition {
                    row: first.row,
                    column: first.end_column - 1
                },
            }),
            "FIRST"
        );
    }
}

#[test]
fn occupied_boundaries_and_completed_empty_rows_survive_json_and_copy() {
    for preferences in preferences() {
        for (first, distance) in [
            (vec![paragraph("FIRST")], 1),
            (vec![literal("FIRST\n")], 2),
            (
                vec![
                    paragraph("FIRST"),
                    Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    },
                ],
                2,
            ),
        ] {
            assert_distance(
                table(
                    vec![cell(first, true), cell(vec![paragraph("SECOND")], false)],
                    preferences.clone(),
                ),
                distance,
            );
        }
    }
}

#[test]
fn nested_boundaries_keep_occupied_and_empty_rows_distinct() {
    for preferences in preferences() {
        for (first, distance) in [
            (vec![paragraph("FIRST")], 1),
            (vec![literal("FIRST\n")], 2),
            (
                vec![
                    paragraph("FIRST"),
                    Block::VerticalSpace {
                        lines: 1,
                        source: None,
                    },
                ],
                2,
            ),
        ] {
            let inner = table(vec![cell(first, true)], preferences.clone());
            assert_distance(
                table(
                    vec![
                        cell(vec![inner], false),
                        cell(vec![paragraph("SECOND")], false),
                    ],
                    preferences.clone(),
                ),
                distance,
            );
        }
    }
}

#[test]
fn empty_data_boundaries_keep_first_middle_nested_and_eof_rows() {
    // Fresh NBRZW/\p controls establish the closed invisible data row;
    // term_fill's acceptance and term_flushln's close survive glyph retirement.
    for preferences in preferences() {
        for nested in [false, true] {
            let mut query = bundle();
            let document = query.document.as_mut().unwrap();
            document.sections.clear();
            let empty = if nested {
                vec![table(vec![cell(vec![], true)], preferences.clone())]
            } else {
                Vec::new()
            };
            document.blocks = vec![table(
                vec![cell(empty, !nested), cell(vec![paragraph("SECOND")], false)],
                preferences.clone(),
            )];
            let rendered = DocumentView::new(&query).render(80);
            let second = &rendered.search("SECOND")[0];
            let mut baseline = query.clone();
            baseline.document.as_mut().unwrap().blocks = vec![table(
                vec![cell(vec![paragraph("SECOND")], false)],
                preferences.clone(),
            )];
            let baseline = DocumentView::new(&baseline).render(80);
            assert_eq!(second.row, baseline.search("SECOND")[0].row + 1);
            assert_eq!(rendered.text.lines[second.row - 1].to_string().trim(), "");
        }
        assert_distance(
            table(
                vec![
                    cell(vec![paragraph("FIRST")], false),
                    cell(vec![], true),
                    cell(vec![paragraph("SECOND")], false),
                ],
                preferences.clone(),
            ),
            1,
        );
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![table(vec![cell(vec![], true)], preferences.clone())];
        let rendered = DocumentView::new(&query).render(80);
        let mut baseline = query.clone();
        baseline.document.as_mut().unwrap().blocks = vec![table(
            vec![cell(vec![paragraph("OCCUPIED")], false)],
            preferences,
        )];
        let baseline = DocumentView::new(&baseline).render(80);
        assert_eq!(rendered.text.lines.len(), baseline.text.lines.len());
        assert_eq!(rendered.text.lines.last().unwrap().to_string().trim(), "");
    }
}

#[test]
fn closed_anchor_hint_rows_keep_json_navigation_and_empty_source_copy() {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![table(
        vec![
            cell(
                vec![Block::Paragraph {
                    children: vec![Inline::anchor("empty-data")],
                    inline_layout: mant_ir::InlineLayout {
                        row_hints: vec![mant_ir::RowLayoutHint {
                            row: 0,
                            indent_columns: 12,
                            ..Default::default()
                        }],
                    },
                    layout: LayoutHint::default(),
                    source: None,
                }],
                true,
            ),
            cell(vec![paragraph("SECOND")], false),
        ],
        mant_ir::ColumnPreferences::default(),
    )];
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    let rendered = DocumentView::new(&restored).render(80);
    let empty = rendered.anchor_row("empty-data").unwrap();
    assert_eq!(rendered.search("SECOND")[0].row, empty + 1);
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: empty,
                column: 0
            },
            focus: TextPosition {
                row: empty,
                column: 12
            },
        }),
        ""
    );
}

fn linked_literal(value: &str) -> Block {
    Block::Preformatted {
        children: vec![Inline::Strong {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.test/tail".into(),
                },
                title: None,
                children: vec![
                    Inline::Text {
                        value: value.into(),
                    },
                    Inline::anchor("tail"),
                ],
            }],
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn generic_fallback_tail_receipts_keep_nested_json_styles_links_and_copy() {
    for depth in [1, 2, 4] {
        for (value, distance) in [("FIRST\n", 1), ("FIRST\n\n", 2), ("FIRST\n \n", 2)] {
            let mut inner = table(
                vec![
                    cell(vec![paragraph("PREFIX")], false),
                    cell(vec![linked_literal(value)], false),
                ],
                mant_ir::ColumnPreferences {
                    gap_columns: u16::MAX,
                    ..Default::default()
                },
            );
            for _ in 1..depth {
                inner = table(
                    vec![cell(vec![inner], false)],
                    mant_ir::ColumnPreferences::default(),
                );
            }
            let outer = table(
                vec![
                    cell(vec![inner], false),
                    cell(vec![paragraph("SECOND")], false),
                ],
                mant_ir::ColumnPreferences::default(),
            );
            assert_distance(outer.clone(), distance);
            assert_fallback_link_style(outer, distance);
        }
    }
}

fn assert_fallback_link_style(block: Block, distance: usize) {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![block];
    let wire = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    for width in [20, 80, u16::MAX] {
        let rendered = DocumentView::new(&restored).render(width);
        let first = &rendered.search("FIRST")[0];
        let second = &rendered.search("SECOND")[0];
        assert_eq!(second.row - first.row, distance);
        assert_eq!(
            rendered.link_target_at(first.row, first.start_column),
            Some(&LinkTarget::External(
                ExternalUri::parse("https://example.test/tail").unwrap()
            ))
        );
        assert!(rendered.text.lines[first.row].spans.iter().any(|span| {
            span.content.contains("FIRST") && span.style.add_modifier.contains(Modifier::BOLD)
        }));
        assert!(rendered.anchor_row("tail").unwrap() < second.row);
    }
}

#[test]
fn structural_data_rows_separate_incoming_gap_budgets_and_keep_empty_copy() {
    for preferences in preferences() {
        for origin in [0, -2] {
            let mut empty = table(Vec::new(), preferences.clone());
            if let Block::Table { layout, .. } = &mut empty {
                layout.indent_columns = origin;
            }
            let mut query = bundle();
            let document = query.document.as_mut().unwrap();
            document.sections.clear();
            document.blocks = vec![
                paragraph("BEFORE"),
                Block::VerticalSpace {
                    lines: 3000,
                    source: None,
                },
                empty,
                Block::VerticalSpace {
                    lines: 3000,
                    source: None,
                },
                paragraph("AFTER"),
            ];
            let rendered = DocumentView::new(&query).render(80);
            let before = &rendered.search("BEFORE")[0];
            let after = &rendered.search("AFTER")[0];
            assert_eq!(after.row - before.row, 6002);
            let blank = before.row + 3001;
            assert_eq!(rendered.text.lines[blank].to_string(), "");
            assert_eq!(
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: blank,
                        column: 0
                    },
                    focus: TextPosition {
                        row: blank,
                        column: 12
                    },
                }),
                ""
            );
        }
    }
}

#[test]
fn native_table_passes_keep_their_authored_cells_in_paint_and_copy() {
    // The native table driver closes both the accepted device-only pass and
    // the attempted follow-up. A HANG HEAD keeps NOBREAK, so the same hidden
    // drawing can leave different accepted words in the inline and block
    // owners without changing their physical row count.
    const HEADER: &str = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n";
    for (owner, operations, lines) in [
        ("block", r"\l'1n.' ", ["", ""]),
        ("body", r"Q\l'2n.'\l'2n '\h'1n'", ["Q", ""]),
        ("head", r"Q\l'2n.'\l'2n '\h'1n'", ["Q", "XYZ"]),
    ] {
        let table = format!(".TS\nl.\nSTART\n{operations}\\p XYZ\nLAST\n.TE\n");
        let source = match owner {
            "head" => format!("{HEADER}.Bl -hang -width 4n\n.It Xo\n{table}.Xc\nENDING\n.El\n"),
            "body" => format!("{HEADER}.Bl -tag -width 4n\n.It TAG\n{table}ENDING\n.El\n"),
            _ => format!("{HEADER}{table}ENDING\n"),
        };
        let query = restored(&source);
        let view = DocumentView::new(&query);
        for width in [24, 78, 24] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let start = &rendered.search("START")[0];
            let last = &rendered.search("LAST")[0];
            assert_eq!(last.row - start.row, 3, "{source}");
            for (index, line) in lines.iter().enumerate() {
                assert_eq!(
                    painted(&buffer, start.row + index + 1, 0, usize::from(width)).trim(),
                    *line,
                    "{source}"
                );
            }
            let copy = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: start.row,
                    column: start.start_column,
                },
                focus: TextPosition {
                    row: last.row,
                    column: last.end_column - 1,
                },
            });
            assert_eq!(
                copy.lines().map(str::trim).collect::<Vec<_>>(),
                ["START", lines[0], lines[1], "LAST"],
                "{source}"
            );
            assert_eq!(rendered.search("XYZ").len(), usize::from(owner == "head"));
        }
    }
}
