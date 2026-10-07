//! Outline matching is independent of folds, visual rows and host effects.
use super::super::outline_search::SearchTarget;
use super::*;

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn query(app: &mut App, value: &str) {
    app.open_outline_search();
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for character in value.chars() {
        key(app, KeyCode::Char(character));
    }
    key(app, KeyCode::Enter);
}
fn markdown(source: &str) -> ResolvedContent {
    let mut bundle = mant_loader::load_markdown_text(source, None).unwrap();
    bundle.address = Some(DocumentAddress::Markdown {
        path: "outline".into(),
        origin: MarkdownOrigin::Documents,
    });
    bundle
}
fn entries() -> ResolvedContent {
    let mut bundle = navigation_bundle();
    let AstBlock::DefinitionList { items, .. } =
        &mut bundle.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        panic!("definitions");
    };
    items[0].terms = vec![
        vec![Inline::Text {
            value: "--help <LANG>".into(),
        }]
        .into(),
    ];
    items[0].entry.as_mut().unwrap().forms = vec![mant_ir::EntryForm::term(0)];
    bundle
}
fn highlighted(app: &App, background: ratatui::style::Color) -> String {
    let rows = app.outline_rows(&app.visible_navigation_indices(), 28, usize::MAX);
    rows.iter()
        .flat_map(|row| &row.line.spans)
        .filter(|span| span.style.bg == Some(background))
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

#[test]
fn shortcut_and_menu_open_the_shared_field_without_stealing_modal_input() {
    let mut app = App::new(&entries());
    app.show_sidebar = false;
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    assert!(app.show_sidebar);
    assert_eq!(app.search_target, SearchTarget::Outline);
    assert!(app.search_is_open());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("Find Outline:"));
    app.close_search();
    app.activate_menu_action(MenuAction::FindOutline);
    assert!(app.outline_search.input.is_open());
    for overlay in [Overlay::Help, Overlay::DocumentFinder] {
        app.overlay = overlay;
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
        assert!(!matches!(app.overlay, Overlay::Menu { .. }));
    }
}

#[test]
fn closed_entries_match_names_aliases_and_full_forms_once_per_node() {
    let mut app = App::new(&entries());
    app.expanded.clear();
    for value in ["--help", "-h", "lang"] {
        query(&mut app, value);
        assert_eq!(app.outline_search.matches.len(), 1, "{value}");
        assert_eq!(
            app.session.document.navigation()[app.selected].id,
            "help-option"
        );
        assert!(app.visible_navigation_indices().contains(&app.selected));
        assert!(!app.outline_search.revealed.is_empty());
        assert!(app.take_open_request().is_none());
    }
    assert_eq!(highlighted(&app, theme::SEARCH_ACTIVE), "LANG");
}

#[test]
fn synthetic_labels_counts_and_tree_markers_are_not_searchable_content() {
    let mut app = App::new(&markdown(
        "# Catalog\n\n## Entries\n\n[ALPHA](target.md#part) and [BETA](target.md#part).\n",
    ));
    query(&mut app, "entries");
    assert_eq!(app.outline_search.matches.len(), 1);
    assert_eq!(
        app.session.document.navigation()[app.selected].kind,
        NavKind::Section
    );
    for value in ["Doc Refs", "locations", "↗", "inventory limited"] {
        query(&mut app, value);
        assert!(app.outline_search.matches.is_empty(), "{value}");
    }
    let mut app = App::new(&entries());
    query(&mut app, "Entries");
    assert!(app.outline_search.matches.is_empty());
}

#[test]
fn target_groups_do_not_duplicate_real_reference_results_or_open_targets() {
    let mut app = App::new(&markdown(
        "# Catalog\n\n## Links\n\n[ALPHA](target.md#part) and [BETA](target.md#part).\n",
    ));
    query(&mut app, "target");
    assert_eq!(app.outline_search.matches.len(), 2);
    assert_eq!(
        app.session.document.navigation()[app.selected].kind,
        NavKind::Reference
    );
    assert!(highlighted(&app, theme::SEARCH_ACTIVE).contains("target"));
    assert!(app.take_open_request().is_none());
    assert!(app.take_external_request().is_none());
    key(&mut app, KeyCode::Down);
    assert_eq!(app.outline_search.active_match, 1);
    assert!(app.take_open_request().is_none());
}

#[test]
fn badge_targets_match_the_owner_without_creating_reference_rows() {
    let mut app = App::new(&markdown(
        "# Catalog\n\n## [Label](target.md#needle)\n\nBody\n",
    ));
    query(&mut app, "needle");
    assert_eq!(app.outline_search.matches.len(), 1);
    assert_eq!(
        app.session.document.navigation()[app.selected].kind,
        NavKind::Section
    );
    assert!(
        !app.session
            .document
            .navigation()
            .iter()
            .any(|node| node.kind == NavKind::Reference)
    );
    assert_eq!(highlighted(&app, theme::SEARCH_ACTIVE), "needle");
    assert!(app.take_open_request().is_none());
}

#[test]
fn temporary_paths_stay_visible_until_exit_without_expanding_matched_subtrees() {
    let mut app = App::new(&markdown(
        "# Catalog\n\n## First\n\n### Needle A\n\n#### Child A\n\n## Second\n\n### Needle B\n\n#### Child B\n",
    ));
    app.expanded.clear();
    query(&mut app, "needle");
    let first = app.navigation_ancestors(app.selected);
    let first_id = app.session.document.navigation()[app.selected].id.clone();
    assert!(!app.navigation_is_expanded(&first_id));
    key(&mut app, KeyCode::Down);
    let second = app.navigation_ancestors(app.selected);
    assert!(first.iter().all(|id| app.navigation_is_expanded(id)));
    assert!(second.iter().all(|id| app.navigation_is_expanded(id)));
    assert!(app.expanded.is_empty());
    key(&mut app, KeyCode::Esc);
    assert!(second.iter().all(|id| app.expanded.contains(id)));
    for id in first {
        if !second.contains(&id) {
            assert!(!app.expanded.contains(&id));
        }
    }
    assert!(app.outline_search.revealed.is_empty());
    assert!(app.visible_navigation_indices().contains(&app.selected));
}

#[test]
fn arrows_enter_and_n_cycle_nodes_with_wraparound() {
    let mut app = App::new(&markdown(
        "# Catalog\n\n## Needle A\n\n## Needle B\n\n## Needle C\n",
    ));
    query(&mut app, "needle");
    assert_eq!(app.outline_search.matches.len(), 3);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.outline_search.active_match, 2);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.outline_search.active_match, 0);
    key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.outline_search.active_match, 1);
    key(&mut app, KeyCode::Char('N'));
    assert_eq!(app.outline_search.active_match, 0);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('N'));
    assert_eq!(app.outline_search.active_match, 2);
    assert!(!app.search_is_open());
}

#[test]
fn scopes_keep_separate_drafts_queries_and_cursor_positions() {
    let mut app = App::new(&entries());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    for character in "help".chars() {
        key(&mut app, KeyCode::Char(character));
    }
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Char('!'));
    let page_cursor = app.search.cursor;
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    for character in "LANG".chars() {
        key(&mut app, KeyCode::Char(character));
    }
    key(&mut app, KeyCode::Left);
    let outline_cursor = app.outline_search.input.cursor;
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert_eq!(app.search.query, "help");
    assert_eq!(app.search.draft, "hel!p");
    assert_eq!(app.search.cursor, page_cursor);
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    assert_eq!(app.outline_search.input.draft, "LANG");
    assert_eq!(app.outline_search.input.cursor, outline_cursor);
    assert!(app.outline_search.input.is_editing());
}

#[test]
fn no_match_and_empty_queries_do_not_move_selection_or_load_a_document() {
    let mut app = App::new(&entries());
    let selected = app.selected;
    let scroll = app.session.content_scroll;
    query(&mut app, "not here");
    assert!(app.outline_search.matches.is_empty());
    assert_eq!(app.selected, selected);
    assert_eq!(app.session.content_scroll, scroll);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("No matches"));
    query(&mut app, "");
    assert!(app.outline_search.matches.is_empty());
    assert!(app.take_open_request().is_none());
}

#[test]
fn logical_results_survive_sidebar_reflow_and_full_label_mode() {
    let mut app = App::new(&markdown(&format!(
        "# Catalog\n\n## {}needle suffix\n",
        "long prefix ".repeat(18)
    )));
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    query(&mut app, "needle");
    let node = app.outline_search.matches[0].node_index;
    for width in [20, 32, 24] {
        app.commit_sidebar_width(width);
        app.full_outline_labels = !app.full_outline_labels;
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert_eq!(app.outline_search.matches.len(), 1);
        assert_eq!(app.outline_search.matches[0].node_index, node);
        assert_eq!(app.outline_search.active_match, 0);
        let buffer = terminal.backend().buffer();
        let area = app.geometry.navigation;
        assert!((area.y..area.bottom()).any(|row| {
            (area.x..area.right())
                .any(|column| buffer.cell((column, row)).unwrap().bg == theme::SEARCH_ACTIVE)
        }));
    }
}

#[test]
fn unicode_fold_and_grapheme_highlights_share_the_page_matcher() {
    let mut app = App::new(&markdown(
        "# Catalog\n\n## İSTANBUL 界 e\u{301} 👩\u{200d}💻\n\n## İSTANBUL 界 e\u{301} 👩\u{200d}💻 again\n",
    ));
    for text in ["i\u{307}", "界", "e\u{301}", "👩\u{200d}💻"] {
        query(&mut app, text);
        assert_eq!(app.outline_search.matches.len(), 2, "{text}");
        let text_highlight = highlighted(&app, theme::SEARCH_ACTIVE);
        assert_ne!(text_highlight, "");
        if text == "👩\u{200d}💻" {
            assert_eq!(text_highlight, "👩\u{200d}💻");
        }
    }
}

#[test]
fn hidden_match_context_is_visible_without_expanding_every_full_label() {
    let bundle = markdown(&format!(
        "# Catalog\n\n## {}needle {}\n\n## Second needle\n",
        "before ".repeat(15),
        "after ".repeat(15)
    ));
    let mut app = App::new(&bundle);
    query(&mut app, "needle");
    key(&mut app, KeyCode::Down);
    assert!(highlighted(&app, theme::SEARCH_MATCH).contains("needle"));
    assert!(!app.full_outline_labels);
}

#[test]
fn menu_help_and_mouse_cursor_use_the_outline_scope() {
    let mut app = App::new(&entries());
    query(&mut app, "LANG");
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen = terminal.backend().to_string();
    assert!(screen.contains("1/1 nodes"));
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.geometry.status.x + 17,
        row: app.geometry.status.y,
        modifiers: KeyModifiers::NONE,
    });
    key(&mut app, KeyCode::Char('!'));
    assert_eq!(app.outline_search.input.draft, "LA!NG");
    app.open_menu(MenuId::Search);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("Find in Outline"));
    app.overlay = Overlay::Help;
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("Ctrl+G"));
}

#[test]
fn a_new_snapshot_cannot_reuse_old_outline_indices_or_node_matches() {
    let mut app = App::new(&entries());
    query(&mut app, "LANG");
    assert_eq!(app.outline_search.matches.len(), 1);
    app.replace_document(
        std::sync::Arc::new(markdown("# Other\n\n## Different\n")),
        super::super::session::DocumentChangeReason::Open,
    );
    assert!(app.outline_search.matches.is_empty());
    assert!(app.outline_search.records.is_empty());
    query(&mut app, "LANG");
    assert!(app.outline_search.matches.is_empty());
}

#[test]
fn manual_folds_and_collapse_all_are_not_overwritten_by_temporary_reveals() {
    let mut app = App::new(&markdown("# Catalog\n\n## Parent\n\n### Needle\n"));
    app.expanded.clear();
    query(&mut app, "needle");
    let ancestor = app
        .navigation_ancestors(app.selected)
        .last()
        .unwrap()
        .clone();
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.id == ancestor)
        .unwrap();
    app.toggle_selected();
    assert!(!app.navigation_is_expanded(&ancestor));
    app.close_outline_search();
    assert!(!app.expanded.contains(&ancestor));
    query(&mut app, "needle");
    app.activate_menu_action(MenuAction::CollapseAll);
    assert!(app.expanded.is_empty());
    assert!(app.outline_search.revealed.is_empty());
    assert!(app.visible_navigation_indices().contains(&app.selected));
}

#[test]
fn global_n_follows_the_last_confirmed_scope_and_page_navigation_can_resume() {
    let mut app = App::new(&entries());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.open_search();
    for character in "help".chars() {
        key(&mut app, KeyCode::Char(character));
    }
    key(&mut app, KeyCode::Enter);
    query(&mut app, "LANG");
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('n'));
    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "help-option"
    );
    app.open_search();
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.last_search_target, SearchTarget::Page);
    key(&mut app, KeyCode::Esc);
    let previous = app.search.active_match;
    key(&mut app, KeyCode::Char('n'));
    assert_eq!(
        app.search.active_match,
        (previous + 1) % app.search.scope_matches.len()
    );
}

#[test]
fn fields_do_not_create_cross_field_matches_or_match_body_descriptions() {
    let mut app = App::new(&entries());
    for value in ["LANG-h", "Show help", "__mant-entries__"] {
        query(&mut app, value);
        assert!(app.outline_search.matches.is_empty(), "{value}");
    }
}

#[test]
fn partial_index_coverage_is_disclosed_even_behind_a_long_query() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().unwrap().sections[0].heading =
        format!("{} OUTSIDE", "x".repeat(65540)).into();
    let mut app = App::new(&bundle);
    query(&mut app, "OUTSIDE");
    assert!(app.outline_search.limited);
    assert!(app.outline_search.matches.is_empty());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("partial"));
    query(&mut app, &"missing".repeat(20));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(terminal.backend().to_string().contains("partial"));
}

#[test]
fn truncated_reference_markers_are_not_matches_and_report_partial_coverage() {
    let bundle = markdown(&format!(
        "# Catalog\n\n## Links\n\n[{}](target.md)\n",
        "label".repeat(1200)
    ));
    let mut app = App::new(&bundle);
    query(&mut app, "…");
    assert!(app.outline_search.matches.is_empty());
    assert!(app.outline_search.limited);
    query(&mut app, "target");
    assert_eq!(app.outline_search.matches.len(), 1);
}

#[test]
fn source_highlights_remain_bounded_at_zero_one_and_two_column_widths() {
    let mut app = App::new(&markdown("# Catalog\n\n## 界 needle 👩\u{200d}💻\n"));
    query(&mut app, "needle");
    for width in [0, 1, 2] {
        let rows = app.outline_rows(&app.visible_navigation_indices(), width, usize::MAX);
        for row in rows {
            assert!(row.line.width() <= usize::from(width));
        }
    }
}

#[test]
fn generated_unlabelled_reference_text_does_not_become_search_evidence() {
    let mut app = App::new(&markdown("# Catalog\n\n## Links\n\n[](target.md)\n"));
    query(&mut app, "unlabelled");
    assert!(app.outline_search.matches.is_empty());
    query(&mut app, "target");
    assert_eq!(app.outline_search.matches.len(), 1);
}
