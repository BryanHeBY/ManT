use super::*;

#[test]
fn gcc_inventory_in_a_generic_groups_description_keeps_its_real_owner() {
    // GCC's Option Summary is a styled, filled paragraph in a generic
    // definition's description, independently from that group's title form.
    let nodes = vec![
        strong("-c -S -E -o"),
        text(" "),
        emphasis("file"),
        text(" "),
        strong("-dumpbase"),
        text(" "),
        emphasis("dumpbase"),
        text(" "),
        strong("-### -nostdinc++ @"),
        emphasis("response_file"),
        text(" "),
        strong("--help[="),
        emphasis("class"),
        strong("[,...]]"),
    ];
    let mut original = content(vec![]);
    original.document.as_mut().unwrap().sections[0].blocks = vec![definition(
        vec![emphasis("Overall Options")],
        vec![paragraph(nodes)],
        facts(EntryKind::Term),
    )];
    assert_eq!(
        mant_ir::validate_document(original.document.as_ref().unwrap()),
        []
    );
    let before = original.clone();
    let view = DocumentView::new(&original);
    let rendered = view.render(240);
    for option in [
        "-c",
        "-S",
        "-E",
        "-o",
        "-dumpbase",
        "-###",
        "-nostdinc++",
        "--help",
    ] {
        let style = style_at(&rendered, option);
        assert_eq!(style.fg, Some(theme::GREEN), "{option}");
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }
    for parameter in ["file", "response_file", "class"] {
        let style = style_at(&rendered, parameter);
        assert_eq!(style.fg, Some(theme::SAPPHIRE), "{parameter}");
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }
    assert_eq!(
        style_at_occurrence(&rendered, "dumpbase", 1).fg,
        Some(theme::SAPPHIRE)
    );
    assert_eq!(style_at(&rendered, "@").fg, Some(theme::BLUE));
    assert_eq!(style_at(&rendered, "[=").fg, Some(theme::MAUVE));
    assert_eq!(style_at(&rendered, "=").fg, Some(theme::BLUE));
    assert_eq!(style_at(&rendered, "Overall Options").fg, Some(theme::TEXT));
    assert!(
        view.lines
            .iter()
            .all(|line| line.surface == LineSurface::Normal)
    );
    assert!(
        view.lines
            .iter()
            .all(|line| line.wrap_mode == WrapMode::Word)
    );
    assert_eq!(
        original, before,
        "coloring must not synthesize semantic entries"
    );
}

#[test]
fn gcc_attached_argument_templates_use_only_the_original_emphasis_ranges() {
    let original = content(vec![
        strong("-fplugin-arg-"),
        emphasis("name"),
        strong("="),
        emphasis("arg"),
        text(" "),
        strong("-ffile-prefix-map="),
        emphasis("old"),
        strong("="),
        emphasis("new"),
        text(" "),
        strong("-fno-builtin-"),
        emphasis("function"),
        text(" "),
        strong("-fcontract-evaluation-semantic=[ignore|observe|quick_enforce]"),
    ]);
    let rendered = DocumentView::new(&original).render(240);
    for option in [
        "-fplugin-arg-",
        "-ffile-prefix-map",
        "-fno-builtin-",
        "-fcontract-evaluation-semantic",
    ] {
        assert_eq!(
            style_at(&rendered, option).fg,
            Some(theme::GREEN),
            "{option}"
        );
    }
    for parameter in ["name", "arg ", "old", "new", "function"] {
        let style = style_at(&rendered, parameter);
        assert_eq!(style.fg, Some(theme::SAPPHIRE), "{parameter}");
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }
    for literal in ["ignore", "observe", "quick_enforce"] {
        assert_eq!(
            style_at(&rendered, literal).fg,
            Some(theme::STRONG),
            "{literal}"
        );
    }
    assert_eq!(style_at(&rendered, "=").fg, Some(theme::BLUE));
    assert_eq!(style_at(&rendered, "|").fg, Some(theme::MAUVE));
}

#[test]
fn body_references_accept_curl_italics_git_bold_and_explicit_inline_commands() {
    let original = content(vec![
        text("Don't combine "),
        emphasis("--parallel"),
        text(" with "),
        strong("--help"),
        text("; run "),
        code("ssh -f host xterm"),
        text(" instead."),
    ]);
    let rendered = DocumentView::new(&original).render(240);
    assert_eq!(style_at(&rendered, "--parallel").fg, Some(theme::GREEN));
    assert!(
        style_at(&rendered, "--parallel")
            .add_modifier
            .contains(Modifier::ITALIC)
    );
    assert_eq!(style_at(&rendered, "--help").fg, Some(theme::GREEN));
    assert!(
        style_at(&rendered, "--help")
            .add_modifier
            .contains(Modifier::BOLD)
    );
    assert_eq!(style_at(&rendered, "ssh").fg, Some(theme::PEACH));
    assert_eq!(style_at(&rendered, "-f").fg, Some(theme::GREEN));
    assert_eq!(style_at(&rendered, "ssh").bg, Some(theme::SURFACE));
    assert_eq!(style_at(&rendered, "Don't").fg, Some(theme::TEXT));
    assert_ne!(style_at(&rendered, "--parallel").bg, Some(theme::SURFACE));
}

#[test]
fn inline_compiler_hash_and_plus_options_do_not_open_a_shell_comment() {
    let original = content(vec![code("gcc -### -nostdinc++ # note")]);
    let rendered = DocumentView::new(&original).render(120);
    assert_eq!(style_at(&rendered, "gcc").fg, Some(theme::PEACH));
    for option in ["-###", "-nostdinc++"] {
        let style = style_at(&rendered, option);
        assert_eq!(style.fg, Some(theme::GREEN), "{option}");
        assert_eq!(style.bg, Some(theme::SURFACE));
    }
    assert_eq!(style_at(&rendered, "# note").fg, Some(theme::SUBTEXT));
    assert_eq!(style_at(&rendered, "note").fg, Some(theme::SUBTEXT));
}

#[test]
fn option_inventories_inside_table_cells_keep_their_normal_surface() {
    let mut original = content(vec![]);
    original.document.as_mut().unwrap().sections[0].blocks = vec![Block::Table {
        borders: mant_ir::TableBorders::default(),
        alignment: None,
        column_count: None,
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            column_borders: vec![],
            cells: vec![TableCell {
                preferred_width_columns: None,
                break_after: false,
                kind: mant_ir::TableCellKind::Text,
                column_span: 1,
                row_span: 1,
                alignment: None,
                blocks: vec![paragraph(vec![
                    strong("-c -S -o"),
                    text(" "),
                    emphasis("file"),
                ])],
            }],
        }],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&original).render(120);
    assert_eq!(style_at(&rendered, "-c").fg, Some(theme::GREEN));
    assert_eq!(style_at(&rendered, "file").fg, Some(theme::SAPPHIRE));
    assert_ne!(style_at(&rendered, "file").bg, Some(theme::SURFACE));
}
