use super::*;

#[derive(serde::Deserialize)]
struct Case {
    source: String,
    probe: String,
    want: usize,
    link: bool,
}

#[test]
fn visible_slots_agree_across_paint_copy_search_and_link_hits() {
    // ASCII and UTF-8 from the fixed CVS agree for these exact sources.
    // Include empty and superseded labels; their target alone cannot prove
    // that any displayed position belongs to the link.
    let cases = [
        include_str!("field_replay_oracle.json"),
        include_str!("word_projection_oracle.json"),
    ]
    .into_iter()
    .flat_map(|data| serde_json::from_str::<Vec<Case>>(data).unwrap());
    let target = LinkTarget::External(ExternalUri::parse("https://example.test/r").unwrap());
    for case in cases {
        assert_visible_slots(
            &case.source,
            &case.probe,
            case.want,
            case.link.then_some(&target),
        );
    }
}

fn assert_visible_slots(source: &str, probe: &str, want: usize, target: Option<&LinkTarget>) {
    let view = DocumentView::new(&restored(source));
    for width in [24, 78] {
        let rendered = view.render(width);
        let buffer = paint(&rendered, width);
        let shown: usize = (0..rendered.row_count)
            .map(|row| {
                painted(&buffer, row, 0, width.into())
                    .matches(probe)
                    .count()
            })
            .sum();
        let copy = rendered.selected_text(RenderedSelection {
            anchor: TextPosition { row: 0, column: 0 },
            focus: TextPosition {
                row: rendered.row_count - 1,
                column: usize::from(width) - 1,
            },
        });
        let hits = rendered.search(probe);
        assert_eq!(
            [shown, copy.matches(probe).count(), hits.len()],
            [want; 3],
            "{source} at {width}"
        );
        for hit in hits {
            for col in hit.start_column..hit.end_column {
                assert_eq!(
                    rendered.link_target_at(hit.row, col),
                    target,
                    "{source} at {width}"
                );
            }
        }
    }
}

#[test]
fn accepted_mdoc_writes_keep_the_same_body_in_paint_copy_and_search() {
    // Fixed terminal profiles agree on the body row after DESCRIPTION.
    // term_word() applies the retreat and term_fill() consumes the gap
    // before a word is available for rendering or search.
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
    for (word, expected, absent) in [
        (r#".Sy A No "\&" No "\z\o''T""#, "A T", "AT"),
        (r#".No "P\zQ" No "\&" Ns No "\z\o''T""#, "PQT", "PQ T"),
        (r#".No PM Ns No "\h'-1n'T""#, "PT", "PMT"),
        (r#".No PM No "\h'-2n'T""#, "PT", "PMT"),
        (r#".No PM No "\h'-3n'T""#, "TM", "PMT"),
        (r#".No "P\zM" No "\h'-4n'\zT""#, "TM", "PMT"),
        (r#".No "P\zM" Ns No "\h'-3n'T""#, "TM", "PMT"),
        (r#".No "P\fBM\h'-2n'\o' T'""#, "TM", "PMT"),
        (r#".Sy PM Ns No "\h'-4n'T""#, "TM", "PMT"),
        (r#".Em PM Ns No "\h'-4n'T""#, "TM", "PMT"),
        (r#".Bq Cm v No "\o''""#, "[]", "[v]"),
        (r".No PRE\z\o'AB'\p T", "PREBT", "PREB T"),
    ] {
        let source = format!("{header}{word}\n.Pp\nDONE.\n");
        let view = DocumentView::new(&restored(&source));
        for width in [24_u16, 78] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let row_text = |row| painted(&buffer, row, 0, width.into());
            let heading = (0..rendered.row_count)
                .find(|&row| row_text(row).trim() == "DESCRIPTION")
                .unwrap();
            let first = (heading + 1..rendered.row_count)
                .find(|&row| !row_text(row).trim().is_empty())
                .unwrap();
            assert_eq!(row_text(first).trim(), expected, "{source}");
            let copy = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: first,
                    column: 0,
                },
                focus: TextPosition {
                    row: first,
                    column: usize::from(width) - 1,
                },
            });
            assert_eq!(copy.trim(), expected, "{source}");
            // PT also occurs in DESCRIPTION. Count only hits in the body.
            for (probe, count) in [(expected, 1), (absent, 0)] {
                assert_eq!(
                    rendered
                        .search(probe)
                        .iter()
                        .filter(|hit| hit.row > heading)
                        .count(),
                    count,
                    "{source} at {width}"
                );
            }
        }
    }
}

#[test]
fn a_pending_link_label_moves_with_its_accepted_native_slot() {
    // term.c::bufferc()/encode1(): the second operand can replace M or
    // move T to its left without transferring M's link to either neighbor.
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
    let target = LinkTarget::External(ExternalUri::parse("https://example.test/r").unwrap());
    for (distance, visible) in [(2, "PT"), (3, "PTM"), (4, "TM")] {
        let source = format!(
            "{header}.No \"P\\X'tty: link https://example.test/r'\\zM\\X'tty: link'\" No \"\\h'-{distance}n'T\"\n.Pp\nDONE.\n"
        );
        let view = DocumentView::new(&restored(&source));
        for width in [24_u16, 78] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let row = (0..rendered.row_count)
                .find(|&row| painted(&buffer, row, 0, width.into()).trim() == visible)
                .unwrap_or_else(|| panic!("{source} at {width}"));
            let hit = rendered
                .search(visible)
                .into_iter()
                .find(|hit| hit.row == row)
                .unwrap();
            assert_eq!(hit.end_column - hit.start_column, visible.len());
            for (offset, character) in visible.bytes().enumerate() {
                assert_eq!(
                    rendered.link_target_at(row, hit.start_column + offset),
                    (character == b'M').then_some(&target),
                    "{source} at {width}"
                );
            }
        }
    }
}

#[test]
fn raw_overstrike_spelling_keeps_the_tbl_word_searchable_at_both_widths() {
    // Fixed CVS term_strlen() sees the raw & in the overstrike on both
    // devices. With a preceding \z, that slot measures but is covered.
    let header =
        ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- example probe\n.SH DESCRIPTION\n";
    for (skip, expected, absent) in [("", "LEFT&T", "LEFTT"), (r"\z", "LEFTT", "LEFT&T")] {
        for alignment in ["l", "r", "c", "n"] {
            for columns in [0, 5, 6, 7] {
                let witness = if columns == 0 {
                    String::new()
                } else {
                    format!("{}\n", "W".repeat(columns))
                };
                let source = format!(
                    "{header}.TS\nbox;\n{alignment}.\nLEFT{skip}\\o'\\&'\\:T\n{witness}.TE\nENDTEXT\n"
                );
                assert_visible_slots(&source, expected, 1, None);
                assert_visible_slots(&source, absent, 0, None);
            }
        }
    }
}

#[test]
fn tbl_breakpoint_measurement_preserves_the_real_link_after_a_retreat() {
    // Fixed ASCII and UTF-8 print LEFTa<BS>b<BS>YTAIL on one row at
    // each witness width. Only YTAIL survives inside a tty link.
    let header =
        ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- example probe\n.SH DESCRIPTION\n";
    let old = LinkTarget::External(ExternalUri::parse("https://old.test/b").unwrap());
    let new = LinkTarget::External(ExternalUri::parse("https://new.test/y").unwrap());
    let cell = r"LEFT\X'tty: link https://old.test/b'\z\o'ab'\X'tty: link'\:\X'tty: link https://new.test/y'YTAIL\X'tty: link'";
    for columns in [0, 9, 10] {
        let witness = if columns == 0 {
            String::new()
        } else {
            format!("{}\n", "W".repeat(columns))
        };
        let source = format!("{header}.TS\nl.\n{cell}\n{witness}.TE\nENDTEXT\n");
        let view = DocumentView::new(&restored(&source));
        for width in [24_u16, 78] {
            let rendered = view.render(width);
            let buffer = paint(&rendered, width);
            let hits = rendered.search("LEFTYTAIL");
            let [hit] = hits.as_slice() else {
                panic!("expected the complete table word at {width}: {source}");
            };
            assert_eq!(
                painted(&buffer, hit.row, hit.start_column, hit.end_column),
                "LEFTYTAIL",
                "{source}"
            );
            let copy = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column,
                },
                focus: TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1,
                },
            });
            assert_eq!(copy, "LEFTYTAIL", "{source}");
            for col in hit.start_column..hit.end_column {
                assert_eq!(
                    rendered.link_target_at(hit.row, col),
                    (col >= hit.start_column + "LEFT".len()).then_some(&new),
                    "{source}"
                );
            }
            assert!(
                (0..rendered.row_count).all(|row| {
                    (0..usize::from(width))
                        .all(|col| rendered.link_target_at(row, col) != Some(&old))
                }),
                "{source}"
            );
            assert_eq!(rendered.search("ENDTEXT").len(), 1, "{source}");
        }
    }
}

#[test]
fn an_equal_label_keeps_the_owner_of_the_surviving_device_copy() {
    let header = ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- sample probe\n.SH DESCRIPTION\nSTARTFLAG\n.br\n";
    let first = LinkTarget::External(ExternalUri::parse("https://one.test").unwrap());
    let second = LinkTarget::External(ExternalUri::parse("https://two.test").unwrap());
    // The fixed devices print the same rows with equal labels and with
    // the independent q/v tracers. term.c::encode1 retains the earlier
    // bold copy when only the final styled slot is overwritten.
    for (before_second, after_second, traced, target) in [
        (r"\h'-2n'", r"\h'-2n'", "pqz", Some(&first)),
        ("", "", "pvz", Some(&second)),
        ("", r"\h'-2n'", "pz", None),
    ] {
        for (left, right, expected) in [
            ('q', 'v', traced.to_owned()),
            ('m', 'm', traced.replace(['q', 'v'], "m")),
        ] {
            let word = format!(
                "p\\X'tty: link https://one.test'\\fB\\z{left}\\fR\\X'tty: link'{before_second}\\X'tty: link https://two.test'\\fR{right}\\fR\\X'tty: link'{after_second}z"
            );
            for source in [
                format!("{header}{word}\n.br\nENDFLAG\n"),
                format!("{header}.nf\n{word}\n.fi\nENDFLAG\n"),
                format!(
                    "{header}.TS\nl.\n{word}\n{}\n.TE\n.br\nENDFLAG\n",
                    "W".repeat(32)
                ),
            ] {
                let view = DocumentView::new(&restored(&source));
                for width in [24_u16, 78] {
                    let rendered = view.render(width);
                    let hits = rendered.search(&expected);
                    let [hit] = hits.as_slice() else {
                        panic!("{source} at {width}");
                    };
                    let buffer = paint(&rendered, width);
                    assert_eq!(
                        painted(&buffer, hit.row, hit.start_column, hit.end_column),
                        expected
                    );
                    let at = |column| TextPosition {
                        row: hit.row,
                        column,
                    };
                    assert_eq!(
                        rendered.selected_text(RenderedSelection {
                            anchor: at(hit.start_column),
                            focus: at(hit.end_column - 1),
                        }),
                        expected
                    );
                    for col in hit.start_column..hit.end_column {
                        let author = if col == hit.start_column + 1 {
                            target
                        } else {
                            None
                        };
                        assert_eq!(rendered.link_target_at(hit.row, col), author, "{source}");
                    }
                    assert_eq!(rendered.search("ENDFLAG").len(), 1, "{source}");
                }
            }
        }
    }
}
