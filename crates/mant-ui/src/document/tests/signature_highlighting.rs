//! Authored IR mirrors real manual owners, independently of native execution.
use super::*;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn content(children: Vec<Inline>) -> ResolvedContent {
    let mut content = bundle();
    let section = &mut content.document.as_mut().unwrap().sections[0];
    section.id = "synopsis".into();
    section.heading = "SYNOPSIS".into();
    section.blocks = vec![paragraph(children)];
    content
}

fn style_at(rendered: &RenderedDocument, needle: &str) -> Style {
    let hit = &rendered.search(needle)[0];
    let area = Rect::new(0, 0, 240, 1);
    let mut buffer = Buffer::empty(area);
    rendered.text.lines[hit.row]
        .clone()
        .render(area, &mut buffer);
    buffer[(u16::try_from(hit.start_column).unwrap(), 0)].style()
}

#[test]
fn lua_filled_synopsis_keeps_original_fonts_and_normal_surface() {
    // Lua's lua.1 uses B/I in filled text. Pinned man_term.c::pre_B/pre_I
    // only select fonts; they do not turn the paragraph into a literal display.
    let children = vec![
        Inline::Strong {
            children: vec![text("lua")],
        },
        text(" [ "),
        Inline::Emphasis {
            children: vec![text("options")],
        },
        text(" ] [ "),
        Inline::Emphasis {
            children: vec![text("script")],
        },
        text(" [ "),
        Inline::Emphasis {
            children: vec![text("args")],
        },
        text(" ] ]"),
    ];
    let original = content(children);
    let encoded = serde_json::to_string(original.document.as_ref().unwrap()).unwrap();
    let mut decoded = original.clone();
    decoded.document = Some(serde_json::from_str(&encoded).unwrap());
    let view = DocumentView::new(&decoded);
    let rendered = view.render(120);
    assert_eq!(style_at(&rendered, "lua").fg, Some(theme::PEACH));
    assert!(
        style_at(&rendered, "lua")
            .add_modifier
            .contains(Modifier::BOLD)
    );
    for name in ["options", "script", "args"] {
        let style = style_at(&rendered, name);
        assert_eq!(style.fg, Some(theme::SAPPHIRE), "{name}");
        assert!(style.add_modifier.contains(Modifier::ITALIC));
        assert_ne!(style.bg, Some(theme::SURFACE));
    }
    for delimiter in ["[", "]"] {
        assert_eq!(style_at(&rendered, delimiter).fg, Some(theme::MAUVE));
    }
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
    assert_eq!(decoded, original);
    assert_eq!(
        serde_json::to_string(decoded.document.as_ref().unwrap()).unwrap(),
        encoded
    );
}

#[test]
fn common_filled_synopses_share_command_option_and_placeholder_roles() {
    // Observed in coreutils, curl, bash, systemd and OpenSSH manuals. The
    // complete owner is classified before any original inline boundaries.
    for (source, command, parameter) in [
        ("ls [OPTION]... [FILE]...", "ls", "FILE"),
        ("dd [OPERAND]...\ndd OPTION", "dd", "OPERAND"),
        ("curl [options / URLs]", "curl", "URLs"),
        (
            "bash [options] [command_string | file]",
            "bash",
            "command_string",
        ),
        ("make [OPTION]... [TARGET]...", "make", "TARGET"),
        (
            "systemctl [OPTIONS...] COMMAND [UNIT...]",
            "systemctl",
            "COMMAND",
        ),
        (
            "ssh [-p port] destination [command [argument ...]]",
            "ssh",
            "port",
        ),
        ("git [-C <path>] [--git-dir=<path>]", "git", "path"),
    ] {
        let children = vec![Inline::Strong {
            children: vec![text(source)],
        }];
        let rendered = DocumentView::new(&content(children)).render(240);
        assert_eq!(
            style_at(&rendered, command).fg,
            Some(theme::PEACH),
            "{source}"
        );
        assert_eq!(
            style_at(&rendered, parameter).fg,
            Some(theme::SAPPHIRE),
            "{source}"
        );
        assert!(
            style_at(&rendered, parameter)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }
}

#[test]
fn literal_and_filled_signatures_agree_without_sharing_geometry() {
    let source = "sed [-e script] [--expression=script] [-i[SUFFIX]] [script-if-no-other-script]";
    let mut literal = content(vec![]);
    literal.document.as_mut().unwrap().sections[0].blocks = vec![Block::Preformatted {
        children: vec![text(source)],
        language: None,
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }];
    let filled = content(vec![text(source)]);
    let left = DocumentView::new(&literal).render(240);
    let right = DocumentView::new(&filled).render(240);
    for (needle, color) in [
        ("sed", theme::PEACH),
        ("-e", theme::GREEN),
        ("--expression", theme::GREEN),
        ("script", theme::SAPPHIRE),
        ("SUFFIX", theme::SAPPHIRE),
        ("script-if-no-other-script", theme::SAPPHIRE),
        ("[", theme::MAUVE),
        ("]", theme::MAUVE),
        ("=", theme::BLUE),
    ] {
        assert_eq!(style_at(&left, needle).fg, Some(color), "literal {needle}");
        assert_eq!(style_at(&right, needle).fg, Some(color), "filled {needle}");
    }
    assert_eq!(style_at(&left, "script").bg, Some(theme::SURFACE));
    assert_ne!(style_at(&right, "script").bg, Some(theme::SURFACE));
}

#[test]
fn tar_subsections_inherit_scope_and_description_restores_prose() {
    let mut content = content(vec![]);
    let source = "tar [-f archive] [FILE...]";
    let document = content.document.as_mut().unwrap();
    let mut child = document.sections[0].clone();
    child.id = "unix-style".into();
    child.heading = "UNIX-style usage".into();
    child.blocks = vec![paragraph(vec![text(source)])];
    document.sections[0].children.push(child);
    let mut description = document.sections[0].children[0].clone();
    description.id = "description".into();
    description.heading = "DESCRIPTION".into();
    document.sections.push(description);
    let rendered = DocumentView::new(&content).render(120);
    let hits = rendered.search(source);
    assert_eq!(hits.len(), 2);
    assert!(
        rendered.text.lines[hits[0].row]
            .spans
            .iter()
            .any(|span| span.style.fg == Some(theme::PEACH))
    );
    assert!(
        rendered.text.lines[hits[1].row]
            .spans
            .iter()
            .filter(|span| !span.content.trim().is_empty())
            .all(|span| span.style.fg == Some(theme::TEXT))
    );
    assert_eq!(
        style_at(&rendered, "UNIX-style usage").fg,
        Some(theme::HEADING)
    );
}

#[test]
fn synopsis_prose_function_declarations_and_option_suffixes_stay_conservative() {
    for source in [
        "Feature Test Macro Requirements for glibc (see feature_test_macros(7)):",
        "dprintf(), vdprintf():",
        "int printf(const char *format, ...);",
        "This tool is part of the cifs-utils suite.",
        "The option [FILE] is optional.",
    ] {
        let rendered = DocumentView::new(&content(vec![text(source)])).render(240);
        for span in rendered.text.lines[rendered.search(source)[0].row]
            .spans
            .iter()
            .filter(|span| !span.content.trim().is_empty())
        {
            assert_eq!(span.style.fg, Some(theme::TEXT), "{source}: {span:?}");
        }
    }
    let rendered = DocumentView::new(&content(vec![text(
        "OPTIONS := { -V[ersion] | -h[uman-readable] }",
    )]))
    .render(240);
    assert_ne!(style_at(&rendered, "OPTIONS").fg, Some(theme::PEACH));
    for suffix in ["ersion", "uman-readable"] {
        assert_ne!(style_at(&rendered, suffix).fg, Some(theme::SAPPHIRE));
    }
}

#[test]
fn signature_colors_survive_every_utf8_inline_split() {
    let source = "lua [options] [<文件e\u{301}>]";
    let baseline = DocumentView::new(&content(vec![text(source)])).render(120);
    for split in source
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(source.len()))
    {
        let actual = DocumentView::new(&content(vec![
            text(&source[..split]),
            text(&source[split..]),
        ]))
        .render(120);
        assert_eq!(actual.text, baseline.text, "split at {split}");
    }
}

#[test]
fn linked_parameters_keep_search_copy_targets_and_fonts_after_reflow() {
    let label = "文件e\u{301}👩‍💻";
    let original = content(vec![
        text("lua [<"),
        Inline::Link {
            target: mant_ir::LinkTarget::Section {
                id: "synopsis".into(),
            },
            title: None,
            children: vec![Inline::Emphasis {
                children: vec![text(label)],
            }],
        },
        text(">]"),
        Inline::anchor("after-argument"),
    ]);
    let view = DocumentView::new(&original);
    for width in [12, 24, 120, 12] {
        let rendered = view.render(width);
        let hit = &rendered.search(label)[0];
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&LinkTarget::Section("synopsis".into()))
        );
        let style = style_at(&rendered, label);
        assert_eq!(style.fg, Some(theme::LINK));
        assert!(
            style
                .add_modifier
                .contains(Modifier::ITALIC | Modifier::UNDERLINED)
        );
        let (row, column) = hit
            .additional_fragments
            .last()
            .map_or((hit.row, hit.end_column), |fragment| {
                (fragment.row, fragment.end_column)
            });
        let copied = rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: hit.row,
                column: hit.start_column,
            },
            focus: TextPosition {
                row,
                column: column - 1,
            },
        });
        assert_eq!(
            copied.lines().map(str::trim_start).collect::<String>(),
            label
        );
        assert!(rendered.anchor_row("after-argument").is_some());
        assert_eq!(rendered.text, view.render(width).text);
    }
}

#[test]
fn table_cells_inherit_synopsis_evidence_without_coloring_explanations() {
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
                blocks: vec![
                    paragraph(vec![text("lua [script]")]),
                    paragraph(vec![text("Run the selected script.")]),
                ],
            }],
        }],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&original).render(120);
    assert_eq!(style_at(&rendered, "lua").fg, Some(theme::PEACH));
    assert_eq!(style_at(&rendered, "script]").fg, Some(theme::SAPPHIRE));
    assert_eq!(style_at(&rendered, "selected script").fg, Some(theme::TEXT));
}

#[test]
fn definition_heads_can_carry_filled_command_synopses() {
    // GNU find uses a tagged synopsis owner rather than a literal block.
    let mut original = content(vec![]);
    original.document.as_mut().unwrap().sections[0].blocks = vec![Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![vec![text("find [-H] [path...] [expression]")].into()],
            description: vec![paragraph(vec![text("Follow symbolic links.")])],
            entry: None,
            layout: mant_ir::DefinitionLayout::default(),
            source: None,
            head_body_relation: mant_ir::HeadBodyRelation::from(false),
        }],
        compact: true,
        declaration_groups: vec![],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&original).render(120);
    assert_eq!(style_at(&rendered, "find").fg, Some(theme::PEACH));
    assert_eq!(style_at(&rendered, "-H").fg, Some(theme::GREEN));
    assert_eq!(style_at(&rendered, "path").fg, Some(theme::SAPPHIRE));
    assert_eq!(style_at(&rendered, "Follow symbolic").fg, Some(theme::TEXT));
}

#[test]
fn excessive_synopsis_owners_keep_the_original_appearance_as_a_whole() {
    for source in [
        format!("lua [script] {}", "x".repeat(crate::code::MAX_BLOCK_BYTES)),
        format!("lua {}script{}", "[".repeat(65), "]".repeat(65)),
    ] {
        let rendered = DocumentView::new(&content(vec![text(&source)])).render(120);
        for needle in ["lua", "script", "["] {
            assert_eq!(style_at(&rendered, needle).fg, Some(theme::TEXT));
        }
    }
}

#[test]
fn large_signature_lookahead_preserves_visible_text_and_link_coordinates() {
    let label = "文件e\u{301}";
    let target = LinkTarget::Section("synopsis".into());
    for (repeats, expected_color) in [
        (4_096, theme::SAPPHIRE),
        (crate::code::MAX_BLOCK_BYTES / 4, theme::TEXT),
    ] {
        let run = "a...".repeat(repeats);
        let split = run.len() / 2;
        let children = vec![
            text("tool [-f "),
            Inline::Emphasis {
                children: vec![text("F"), text("ILE")],
            },
            text("] ["),
            text(&run[..split]),
            text(&run[split..]),
            text("] [<"),
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "synopsis".into(),
                },
                title: None,
                children: vec![text(label)],
            },
            text(">]"),
        ];
        let content = content(children);
        let document = content.document.as_ref().unwrap();
        assert_eq!(mant_ir::validate_document(document), []);
        let snapshot = serde_json::to_vec(document).unwrap();
        let expected = format!("tool [-f FILE] [{run}] [<{label}>]");
        let view = DocumentView::new(&content);
        for width in [24, 120] {
            let rendered = view.render(width);
            assert!(
                (0..rendered.search_record_count())
                    .any(|index| rendered.search_record_text(index) == expected)
            );
            let file = style_at(&rendered, "FILE");
            assert_eq!(file.fg, Some(expected_color));
            assert!(file.add_modifier.contains(Modifier::ITALIC));
            let hits = rendered.search(label);
            let [hit] = hits.as_slice() else {
                panic!("expected one link label match, got {hits:?}");
            };
            assert_eq!(
                rendered.link_target_at(hit.row, hit.start_column),
                Some(&target)
            );
            assert_eq!(style_at(&rendered, label).fg, Some(theme::LINK));
            let (row, column) = hit
                .additional_fragments
                .last()
                .map_or((hit.row, hit.end_column), |fragment| {
                    (fragment.row, fragment.end_column)
                });
            let selected = rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column,
                },
                focus: TextPosition {
                    row,
                    column: column - 1,
                },
            });
            assert_eq!(
                selected.lines().map(str::trim_start).collect::<String>(),
                label
            );
        }
        assert_eq!(serde_json::to_vec(document).unwrap(), snapshot);
    }
}

#[test]
fn unrecognized_and_unaccented_synopsis_text_keeps_prose_font_precedence() {
    for source in ["explanatory prose", "curl [options / URLs]"] {
        let children = vec![Inline::Strong {
            children: vec![Inline::Code {
                value: source.into(),
            }],
        }];
        let original = content(children);
        let rendered = DocumentView::new(&original).render(120);
        let needle = if source.starts_with("curl") {
            "/"
        } else {
            source
        };
        let style = style_at(&rendered, needle);
        assert_eq!(style.fg, Some(theme::SUBTEXT_BRIGHT));
        assert_eq!(style.bg, Some(theme::SURFACE));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        let mut description = original;
        description.document.as_mut().unwrap().sections[0].heading = "DESCRIPTION".into();
        assert_eq!(
            style_at(&DocumentView::new(&description).render(120), needle),
            style
        );
    }
}
