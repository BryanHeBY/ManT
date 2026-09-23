//! Table ownership and cell-content transfer from pinned tbl execution.

use super::*;

#[test]
fn simple_table_retains_data_and_empty_cell_roots() {
    // The exact input was run through the fixed CVS UTF-8/78 reference.
    // tbl_term.c::tbl_data renders each data row and tbl_word emits only
    // authored cell content; empty cells remain structural cells.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "table.1",
            b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl l.\nleft;right\nempty;\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude("table.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("supported tbl rows produce a checked native result");
    assert_eq!(document.tables.len(), 1);
    assert_eq!(document.table_rows.len(), 2);
    assert_eq!(document.table_cells.len(), 4);
    let texts = document
        .table_cells
        .iter()
        .map(|cell| {
            let root = document.content_points[(cell.point.expect("cell point") - 1) as usize].root;
            document
                .content_atoms
                .iter()
                .filter(|atom| atom.root == root)
                .map(|atom| atom.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(texts, ["left", "right", "empty", ""]);
}

#[test]
fn explicit_tbl_width_and_spacing_select_native_geometry() {
    // Each exact input was run through fixed CVS UTF-8/78 first.  In
    // tbl_layout.c::mods, SIZE_MAX means unspecified spacing while l0 is an
    // explicit zero; out.c::tblcalc applies both spacing and minimum width.
    for (layout, fixed) in [
        ("l l.", false),
        ("l0 l.", true),
        ("l9 l.", true),
        ("lw(20n) l.", true),
    ] {
        let source = format!(".TH T 1\n.SH D\n.TS\ntab(;);\n{layout}\na;b\n.TE\n");
        let mut bundle = SourceBundle::new();
        bundle.insert("spacing.1", source.into_bytes()).unwrap();
        let document = render_prelude(
            "spacing.1",
            &bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("explicit native table geometry is retained");
        assert_eq!(document.tables.len(), 1);
        assert_eq!(document.tables[0].fixed_view.is_some(), fixed, "{layout}");
    }
}

#[test]
fn boxed_cell_overstrike_keeps_native_overlay_placement() {
    // Exact source was run with fixed CVS UTF-8/78.  tbl_term.c::tbl_word
    // passes cell text through term.c::term_field; its backspace precedes the
    // next FIELD_PLACE even when the cell is inside a drawn frame.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "overlay.1",
            b".TH T 1\n.SH D\n.TS\nbox tab(;);\nl l.\n\\zAB;x\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "overlay.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("boxed native cell overstrike is a valid fixed result");
    assert_eq!(document.tables.len(), 1);
    assert!(document.tables[0].fixed_view.is_some());
    assert!(
        document
            .placements
            .iter()
            .any(|placement| placement.cell_map_kind == 3)
    );
}

#[test]
fn nofill_display_finishes_before_table_scope_binds_cells() {
    // Each exact source was run through fixed CVS UTF-8/78.  In both
    // man_term.c and mdoc_term.c, the ROFFT_TBL path calls term_newln before
    // term_tbl; the observer switches ownership after that flush, before
    // the table frame or first cell can be emitted.
    for (name, format, source, fixed_table) in [
        (
            "plain.1",
            InputFormat::Man,
            ".TH T 1\n.SH D\n.nf\nbefore\n.TS\ntab(;);\nl l.\na;b\n.TE\n.fi\n",
            false,
        ),
        (
            "boxed.1",
            InputFormat::Man,
            ".TH T 1\n.SH D\n.nf\nbefore\n.TS\nbox tab(;);\nl l.\na;b\n.TE\n.fi\n",
            true,
        ),
        (
            "literal.1",
            InputFormat::Mdoc,
            ".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Bd -literal\nbefore\n.TS\ntab(;);\nl l.\na;b\n.TE\n.Ed\n",
            false,
        ),
    ] {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = render_prelude(name, &bundle, format, 78, &Limits::default())
            .expect("the preceding no-fill line and table cells remain distinct");
        assert_eq!(document.tables.len(), 1, "{name}");
        assert_eq!(document.table_cells.len(), 2, "{name}");
        assert_eq!(
            document.tables[0].fixed_view.is_some(),
            fixed_table,
            "{name}"
        );
        let before = document
            .content_atoms
            .iter()
            .find(|atom| atom.text == "before")
            .expect("preceding fixed body atom");
        let prior_view = document.fixed_views[0].key;
        let prior_lines = document
            .fixed_lines
            .iter()
            .filter(|line| line.view == prior_view)
            .map(|line| line.key)
            .collect::<Vec<_>>();
        assert!(
            document.placements.iter().any(|placement| {
                placement.atom == Some(before.key) && prior_lines.contains(&placement.line)
            }),
            "{name}: the prior physical line stays in its display"
        );
        for cell in &document.table_cells {
            let root = document.content_points[(cell.point.expect("cell point") - 1) as usize].root;
            assert_ne!(root, before.root, "{name}: cell has its own root");
        }
    }
}

#[test]
fn table_count_budgets_are_independent_and_recover() {
    // Exact two-table, three-row, six-cell input was checked with fixed CVS
    // UTF-8/78. tbl_term.c::term_tbl executes each table and row; these
    // assertions concern only the collector's separate result budgets.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "limits.1",
            b".TH T 1\n.SH D\n.TS\ntab(;);\nl l.\na;b\nc;d\n.TE\n.TS\ntab(;);\nl l.\ne;f\n.TE\n"
                .to_vec(),
        )
        .unwrap();
    for (limits, kind, observed) in [
        (
            Limits {
                max_tables: 1,
                ..Limits::default()
            },
            17,
            2,
        ),
        (
            Limits {
                max_table_rows: 1,
                ..Limits::default()
            },
            18,
            2,
        ),
        (
            Limits {
                max_table_cells: 1,
                ..Limits::default()
            },
            19,
            2,
        ),
    ] {
        let error = render_prelude("limits.1", &bundle, InputFormat::Man, 78, &limits)
            .expect_err("the dedicated table budget must reject its second object");
        assert_eq!(error.status, STATUS_BUDGET, "{error:?}");
        assert_eq!(
            (error.limit_kind, error.observed, error.allowed),
            (kind, observed, 1)
        );
    }
    let recovered = render_prelude(
        "limits.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("a subsequent complete table render recovers");
    assert_eq!(
        (
            recovered.tables.len(),
            recovered.table_rows.len(),
            recovered.table_cells.len()
        ),
        (2, 3, 6)
    );
}

#[test]
fn table_count_budgets_accept_exact_boundary() {
    // This exact one-cell table was checked with fixed CVS UTF-8/78.
    let mut bundle = SourceBundle::new();
    bundle
        .insert("one.1", b".TH T 1\n.SH D\n.TS\nl.\nx\n.TE\n".to_vec())
        .unwrap();
    let limits = Limits {
        max_tables: 1,
        max_table_rows: 1,
        max_table_cells: 1,
        ..Limits::default()
    };
    let document = render_prelude("one.1", &bundle, InputFormat::Man, 78, &limits)
        .expect("one table, row, and cell fit their exact dedicated limits");
    assert_eq!(
        (
            document.tables.len(),
            document.table_rows.len(),
            document.table_cells.len()
        ),
        (1, 1, 1)
    );
}

#[test]
fn boxed_table_reuses_cell_atoms_in_native_fixed_geometry() {
    // Exact UTF-8/78 input was run through the fixed CVS reference first.
    // tbl_term.c::term_tbl emits native frame rows and cell placements;
    // tbl_word contributes logical content only once per authored cell.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "box.1",
            b".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nl l.\nleft;right\nempty;\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude("box.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("boxed native table has checked fixed geometry");
    assert_eq!(document.tables.len(), 1);
    assert!(document.tables[0].fixed_view.is_some());
    assert_eq!(document.table_cells.len(), 4);
    assert_eq!(
        document
            .content_atoms
            .iter()
            .filter(|atom| atom.text == "left")
            .count(),
        1
    );
    for cell in &document.table_cells {
        let point = cell.point.expect("each native cell has a content point");
        let placement = document
            .placements
            .iter()
            .find(|placement| placement.target_kind == 2 && placement.point == Some(point))
            .expect("even an empty boxed cell has a physical location");
        let line = &document.fixed_lines[placement.line as usize - 1];
        assert!(placement.columns.end <= line.terminal_columns);
    }
}

#[test]
fn fixed_table_rule_and_empty_rows_keep_cell_positions() {
    // Both exact inputs were run through fixed CVS UTF-8/78 first.
    // tbl_term.c::term_tbl prints even empty/rule data cells without calling
    // tbl_word(), so the first printed physical line supplies their point.
    for (name, input, expected_cells) in [
        (
            "empty-row.1",
            b".TH CELLS 1\n.SH DATA\n.TS\ntab(;);\nr r.\nx;y\n;\n.TE\n".as_slice(),
            4,
        ),
        (
            "rule-cells.1",
            b".TH CELLS 1\n.SH DATA\n.TS\ntab(;);\nl l.\nx;y\n\\_;\\=\n;\n.TE\n".as_slice(),
            6,
        ),
    ] {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, input.to_vec()).unwrap();
        let document = render_prelude(name, &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("native fixed table with empty/rule cells");
        assert_eq!(document.table_cells.len(), expected_cells);
        for cell in &document.table_cells {
            let point = cell.point.expect("table cell point");
            let placement = document
                .placements
                .iter()
                .find(|placement| placement.target_kind == 2 && placement.point == Some(point))
                .expect("table cell position in native geometry");
            let line = &document.fixed_lines[placement.line as usize - 1];
            assert!(placement.columns.end <= line.terminal_columns);
        }
    }
}

#[test]
fn fixed_table_spans_and_text_blocks_do_not_duplicate_cell_points() {
    // Both exact inputs were run through fixed CVS UTF-8/78 first.
    // tbl_term.c::term_tbl emits a cell-position event only on the first
    // physical row line and skips columns covered by horizontal spans.
    for (name, input) in [
        (
            "vertical.1",
            b".TH SPAN 1\n.SH DATA\n.TS\nbox tab(;);\nl l,\n^ l.\na;b\n;c\n.TE\n".as_slice(),
        ),
        (
            "block.1",
            b".TH SPAN 1\n.SH DATA\n.TS\nbox tab(;);\nl l.\nT{\nfirst\nsecond\nT};right\n.TE\n"
                .as_slice(),
        ),
    ] {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, input.to_vec()).unwrap();
        let document = render_prelude(name, &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("fixed span/block table survives native validation");
        for cell in &document.table_cells {
            let point = cell.point.expect("native cell point");
            assert_eq!(
                document
                    .placements
                    .iter()
                    .filter(|placement| placement.target_kind == 2
                        && placement.point == Some(point))
                    .count(),
                1,
                "{name}: one first-line placement per logical cell"
            );
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)] // Mutate and restore one live handle across the relation matrix.
fn fixed_table_corruption_is_rejected_before_owned_transfer() {
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "box.1",
            b".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nl l.\nleft;right\n.TE\n".to_vec(),
        )
        .unwrap();
    let limits = Limits::default();
    let storage = InputStorage::new("box.1", &bundle, InputFormat::Man, &limits).unwrap();
    let (status, pointer, failure) = raw_render(&storage.view(78, PROFILE_UTF8), &limits);
    assert_eq!(status, STATUS_OK, "{failure:?}");
    let handle = ResultHandle(NonNull::new(pointer).unwrap());
    let mut view = ResultView::default();
    assert_eq!(
        unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) },
        STATUS_OK
    );
    let fixed = unsafe {
        std::slice::from_raw_parts_mut(
            view.fixed_views.ptr.cast::<FixedView>().cast_mut(),
            view.fixed_views.count as usize,
        )
    };
    let placements = unsafe {
        std::slice::from_raw_parts_mut(
            view.placements.ptr.cast::<PlacementView>().cast_mut(),
            view.placements.count as usize,
        )
    };
    let cells = unsafe {
        std::slice::from_raw_parts(
            view.table_cells.ptr.cast::<TableCellView>(),
            view.table_cells.count as usize,
        )
    };
    assert_eq!(fixed.len(), 1);
    assert!(!placements.is_empty());
    let mut failure = FailureView::default();
    let original_table = fixed[0].table;
    fixed[0].table = 0;
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION
    );
    assert!(copy_structured_document(&handle, &view, &limits).is_err());
    fixed[0].table = original_table;
    let original_end = placements[0].byte_end;
    placements[0].byte_end = u32::MAX;
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION
    );
    assert!(copy_structured_document(&handle, &view, &limits).is_err());
    placements[0].byte_end = original_end;
    let point_placement = placements
        .iter()
        .position(|placement| placement.target_kind == 2 && placement.point == cells[0].point)
        .expect("first cell point placement");
    let original_point = placements[point_placement].point;
    placements[point_placement].point = cells[1].point;
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION,
        "native result check rejects a cell point with the wrong identity"
    );
    assert!(
        copy_structured_document(&handle, &view, &limits).is_err(),
        "handle-bound validation rejects a duplicated/missing cell point"
    );
    placements[point_placement].point = original_point;
    let second_point_placement = placements
        .iter()
        .position(|placement| placement.target_kind == 2 && placement.point == cells[1].point)
        .expect("second cell point placement");
    let first = (
        placements[point_placement].point,
        placements[point_placement].cell,
    );
    let second = (
        placements[second_point_placement].point,
        placements[second_point_placement].cell,
    );
    placements[point_placement].point = second.0;
    placements[point_placement].cell = second.1;
    placements[second_point_placement].point = first.0;
    placements[second_point_placement].cell = first.1;
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION,
        "point/cell identities cannot be transposed across physical locations"
    );
    assert!(copy_structured_document(&handle, &view, &limits).is_err());
    placements[point_placement].point = first.0;
    placements[point_placement].cell = first.1;
    placements[second_point_placement].point = second.0;
    placements[second_point_placement].cell = second.1;
    placements[point_placement].cell = 0;
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION,
        "a table-cell point cannot masquerade as an unowned anchor"
    );
    assert!(copy_structured_document(&handle, &view, &limits).is_err());
    placements[point_placement].cell = first.1;
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_OK
    );
}

#[test]
fn tbl_style_and_rule_rows_keep_separate_semantics() {
    // This exact source was run through the fixed CVS UTF-8/78 renderer.
    // tbl_term.c::tbl_word enters font scope for data cells; tbl_hrule and
    // tbl_fill_border produce geometry without authored cell text.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "rules.1",
            b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl l.\n\\fBbold\\fP;plain\n_\n\\_;x\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude("rules.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("rule and data rows remain a valid structured result");
    assert_eq!(document.table_rows.len(), 3);
    assert_eq!(document.table_rows[1].kind, 2);
    assert!(
        document
            .table_cells
            .iter()
            .all(|cell| cell.row != document.table_rows[1].key)
    );
    assert!(document.table_cells.iter().any(|cell| cell.kind == 4));
    assert!(
        document
            .content_atoms
            .iter()
            .any(|atom| atom.text == "bold" && atom.style_flags & 1 != 0)
    );
}

#[test]
fn horizontal_span_keeps_one_cell_identity() {
    // Exact input checked with the fixed CVS UTF-8/78 reference. In
    // tbl_term.c::term_tbl, layout `s` skips a column while `hspans`
    // extends the preceding data cell; it is not another content owner.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "span.1",
            b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl s l.\nwide;tail\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude("span.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("spanned tbl row remains valid");
    assert_eq!(document.table_cells.len(), 2);
    assert_eq!(document.table_cells[0].column_span, 2);
    assert_eq!(document.table_cells[1].column, 2);
}

#[test]
fn table_boundary_does_not_reuse_surrounding_prose_root() {
    // Exact input checked with the fixed CVS UTF-8/78 reference. The
    // man_term.c::print_man_node traversal places tbl between two prose
    // fragments; tbl_term.c emits its cell content through a separate scope.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "mixed.1",
            b".TH T 1\n.SH DATA\nbefore\n.TS\ntab(;);\nl l.\na;b\n.TE\nafter\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude("mixed.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("prose and tbl coexist without borrowing one another's root");
    let before = document
        .content_atoms
        .iter()
        .find(|atom| atom.text == "before")
        .unwrap();
    let after = document
        .content_atoms
        .iter()
        .find(|atom| atom.text == "after")
        .unwrap();
    assert_ne!(before.root, after.root);
    assert_eq!(document.tables.len(), 1);
    assert_eq!(document.table_cells.len(), 2);
}

#[test]
fn table_cell_owner_and_point_corruption_is_rejected_on_both_sides() {
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "table.1",
            b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl l.\na;b\n.TE\n".to_vec(),
        )
        .unwrap();
    let limits = Limits::default();
    let storage = InputStorage::new("table.1", &bundle, InputFormat::Man, &limits).unwrap();
    let (status, pointer, failure) = raw_render(&storage.view(78, PROFILE_UTF8), &limits);
    assert_eq!(status, STATUS_OK, "{failure:?}");
    let handle = ResultHandle(NonNull::new(pointer).unwrap());
    let mut view = ResultView::default();
    assert_eq!(
        unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) },
        STATUS_OK
    );
    let cells = unsafe {
        std::slice::from_raw_parts_mut(
            view.table_cells.ptr.cast::<TableCellView>().cast_mut(),
            view.table_cells.count as usize,
        )
    };
    let saved_owner = cells[1].owner;
    cells[1].owner = cells[0].owner;
    let mut failure = FailureView::default();
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION
    );
    assert!(copy_structured_document(&handle, &view, &limits).is_err());
    cells[1].owner = saved_owner;
    let saved_point = cells[1].point;
    cells[1].point = 0;
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION
    );
    assert!(copy_structured_document(&handle, &view, &limits).is_err());
    cells[1].point = saved_point;
}

#[test]
fn layout_rule_precedes_ignored_data_cells() {
    // Exact source checked with fixed CVS UTF-8/78. tbl_term.c::tbl_data
    // tests IS_HORIZ(layout) before inspecting dp, so x/y are not visible.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "layout-rule.1",
            b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl l\n_ =.\na;b\nx;y\n.TE\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "layout-rule.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("layout-only rule row remains distinct");
    assert_eq!(document.table_rows.len(), 2);
    assert_eq!(document.table_rows[1].kind, 4);
    assert_eq!(document.table_cells[2].kind, 2);
    assert_eq!(document.table_cells[3].kind, 3);
    assert!(
        !document
            .content_atoms
            .iter()
            .any(|atom| atom.text == "x" || atom.text == "y")
    );
}
