use super::{PreparedLine, UnitKind, WrapMode, workload};

fn painted(line: &PreparedLine<'_>, units: std::ops::Range<usize>) -> String {
    let mut result = String::new();
    for unit in &line.units()[units] {
        match unit.kind {
            UnitKind::Grapheme => result.push_str(&line.source()[unit.bytes.clone()]),
            UnitKind::Tab { .. } => result.push(' '),
        }
    }
    result
}

#[test]
fn tab_advances_keep_source_byte_and_scalar_identity_at_every_local_phase() {
    for origin in [0, 1, 7, 8, 9] {
        for prefix in [0, 1, 7, 8, 9] {
            let source = format!("{}\t中e\u{301}", "a".repeat(prefix));
            let line = PreparedLine::new(&source, origin);
            let spaces = 8 - (origin + prefix) % 8;
            assert_eq!(line.source(), source);
            assert_eq!(line.columns(), prefix + spaces + 3);
            let tabs: Vec<_> = line
                .units()
                .iter()
                .filter(|unit| matches!(unit.kind, UnitKind::Tab { .. }))
                .collect();
            assert_eq!(tabs.len(), spaces);
            for (part, unit) in tabs.iter().enumerate() {
                assert_eq!(unit.bytes, prefix..prefix + 1);
                assert_eq!(unit.scalars, prefix..prefix + 1);
                assert_eq!(
                    unit.kind,
                    UnitKind::Tab {
                        part,
                        total: spaces
                    }
                );
                assert_eq!(unit.columns, 1);
            }
        }
    }
}

#[test]
fn word_rows_retain_exact_internal_seams_and_authored_edge_whitespace() {
    for (source, width, expected, seams) in [
        ("  abc def  ", 5, vec!["  abc", "def  "], vec![" ", ""]),
        ("one  two  xx", 8, vec!["one  two", "xx"], vec!["  ", ""]),
        ("abc\tdef", 7, vec!["abc", "def"], vec!["\t", ""]),
        ("long ", 4, vec!["long", " "], vec!["", ""]),
        ("    ", 2, vec!["  ", "  "], vec!["", ""]),
        ("", 4, vec![""], vec![""]),
    ] {
        let line = PreparedLine::new(source, 0);
        let rows = line.wrap(width, 0, 0, WrapMode::Word);
        assert_eq!(
            rows.iter()
                .map(|row| painted(&line, row.units.clone()))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            rows.iter()
                .map(|row| &source[row.seam_after.clone()])
                .collect::<Vec<_>>(),
            seams
        );
        let mut restored = String::new();
        for row in &rows {
            restored.push_str(&source[row.source.clone()]);
            restored.push_str(&source[row.seam_after.clone()]);
        }
        assert_eq!(restored, source);
    }
    for fixed in ['\u{a0}', '\u{2007}', '\u{202f}'] {
        let source = format!("a{fixed}b");
        let line = PreparedLine::new(&source, 0);
        let rows = line.wrap(1, 0, 0, WrapMode::Word);
        assert!(rows.iter().all(|row| row.seam_after.is_empty()));
        assert_eq!(line.longest_word_columns(), 3);
    }
}

#[test]
fn visual_origins_do_not_rephase_tabs_and_split_parts_keep_one_source_scalar() {
    let line = PreparedLine::new("\t", 3);
    let rows = line.wrap(2, 0, 0, WrapMode::Character);
    assert_eq!(
        rows.iter().map(|row| row.columns).collect::<Vec<_>>(),
        [2, 2, 1]
    );
    for row in &rows {
        assert_eq!(row.source, 0..1);
        assert!(row.seam_after.is_empty());
        assert!(
            line.units()[row.units.clone()]
                .iter()
                .all(|unit| unit.scalars == (0..1))
        );
    }
    let line = PreparedLine::new("aa\tB\tC", 3);
    let expected = painted(&line, 0..line.units().len());
    for width in [3, 5, 8, 13] {
        let rows = line.wrap(width, 1, 2, WrapMode::Character);
        assert_eq!(rows[0].origin, 1);
        assert!(rows[1..].iter().all(|row| row.origin == 2));
        assert_eq!(
            rows.iter()
                .map(|row| painted(&line, row.units.clone()))
                .collect::<String>(),
            expected
        );
        assert!(rows.iter().all(|row| row.origin + row.columns <= width));
    }
}

#[test]
fn grapheme_units_keep_combining_and_joined_source_ranges_whole() {
    let source = "e\u{301}👩‍💻界";
    let line = PreparedLine::new(source, 0);
    assert_eq!(
        line.units()
            .iter()
            .map(|unit| (unit.bytes.clone(), unit.scalars.clone(), unit.columns))
            .collect::<Vec<_>>(),
        [(0..3, 0..2, 1), (3..14, 2..5, 2), (14..17, 5..6, 2)]
    );
    for mode in [WrapMode::Word, WrapMode::Character] {
        let rows = line.wrap(1, 0, 0, mode);
        assert_eq!(
            rows.iter()
                .map(|row| painted(&line, row.units.clone()))
                .collect::<Vec<_>>(),
            ["e\u{301}", "👩‍💻", "界"]
        );
    }
}

#[test]
fn preparation_and_break_planning_visit_units_linearly_across_widths() {
    for pattern in ["x", " ", "x ", "x\t"] {
        for mode in [WrapMode::Word, WrapMode::Character] {
            for width in [1, 3, 7, 80, 512] {
                let mut previous = None;
                for count in [512, 2048, 8192] {
                    let source = pattern.repeat(count);
                    let ((line, rows), visits) = workload::measure(|| {
                        let line = PreparedLine::new(&source, 0);
                        let rows = line.wrap(width, 0, 0, mode);
                        (line, rows)
                    });
                    assert!(
                        visits <= line.units().len() * 8 + 32,
                        "{pattern:?}/{mode:?}/{width}/{count}: {visits}"
                    );
                    assert_ne!(rows, []);
                    if let Some(smaller) = previous {
                        // Four larger final partial rows may inspect a
                        // different fitted prefix; that is bounded by width.
                        assert!(
                            visits <= smaller * 4 + width * 4 + 128,
                            "{pattern:?}/{mode:?}/{width}/{count}: {smaller} -> {visits}"
                        );
                    }
                    previous = Some(visits);
                }
            }
        }
    }
}

#[test]
fn long_authored_edges_and_internal_seams_do_not_rescan_unconsumed_suffixes() {
    for source in [
        format!("{}tail", " ".repeat(100_000)),
        format!("head{}", " ".repeat(100_000)),
        format!("head{}tail", " ".repeat(100_000)),
    ] {
        for width in [1, 4] {
            let ((line, rows), visits) = workload::measure(|| {
                let line = PreparedLine::new(&source, 0);
                let rows = line.wrap(width, 0, 0, WrapMode::Word);
                (line, rows)
            });
            assert!(visits <= line.units().len() * 8 + 32);
            let mut restored = String::new();
            for row in rows {
                restored.push_str(&source[row.source]);
                restored.push_str(&source[row.seam_after]);
            }
            assert_eq!(restored, source);
        }
    }
}

#[test]
fn unsafe_control_glyphs_are_not_word_wrap_separators() {
    for control in ['\u{b}', '\u{c}', '\r', '\u{85}'] {
        let source = format!("ab{control}cd");
        let line = PreparedLine::new(&source, 0);
        assert_eq!(line.columns(), 5);
        assert_eq!(line.longest_word_columns(), 5);
        for width in [1, 2, 3, 80] {
            let rows = line.wrap(width, 0, 0, WrapMode::Word);
            assert!(rows.iter().all(|row| row.seam_after.is_empty()));
        }
    }
}
