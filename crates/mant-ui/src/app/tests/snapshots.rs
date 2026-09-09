//! Snapshot identity is allocation provenance, never a logical address.
use std::sync::Arc;

use super::*;
use crate::app::{
    ReaderOptions,
    navigation_state::{HistoryDirection, LocalTarget},
};

fn snapshot(text: &str, addressed: bool) -> Arc<ResolvedContent> {
    let mut bundle = navigation_bundle();
    bundle.address = addressed.then(|| DocumentAddress::Markdown {
        path: "same-source".into(),
        origin: MarkdownOrigin::Documents,
    });
    let document = bundle.document.as_mut().expect("document");
    document.sections.clear();
    document.blocks = vec![AstBlock::Paragraph {
        children: vec![Inline::Text { value: text.into() }],
        layout: LayoutHint::default(),
        source: None,
    }];
    Arc::new(bundle)
}

#[test]
fn shared_startup_and_node_copy_reuse_the_exact_snapshot() {
    let current = Arc::new(navigation_bundle());
    let scope_member = snapshot("another revision", true);
    let mut app = App::from_shared(ReaderOptions {
        current: Arc::clone(&current),
        catalog: DocumentCatalog::default(),
        scope: vec![Arc::clone(&scope_member), Arc::clone(&current)],
    });
    assert!(Arc::ptr_eq(&app.session.current_bundle, &current));
    assert_eq!(app.scope_documents.len(), 2);
    assert!(Arc::ptr_eq(&app.scope_documents[0], &scope_member));
    assert!(Arc::ptr_eq(&app.scope_documents[1], &current));
    app.selected = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == NavKind::Section)
        .expect("section");
    app.copy_selected_node(CopyFormat::Text);
    let CopyRequest::Node { content, .. } = app.take_copy_request().expect("copy") else {
        panic!("node copy");
    };
    assert!(Arc::ptr_eq(&content, &current));
}

#[test]
fn borrowed_startup_copies_once_when_current_is_a_scope_member() {
    let scope = vec![navigation_bundle(), navigation_bundle()];
    let app = App::with_catalog_and_scope(&scope[1], DocumentCatalog::default(), &scope);
    assert_eq!(app.scope_documents.len(), 2);
    assert!(Arc::ptr_eq(
        &app.session.current_bundle,
        &app.scope_documents[1]
    ));
    assert!(!std::ptr::eq(
        app.session.current_bundle.as_ref(),
        &raw const scope[1]
    ));
}

#[test]
fn same_address_and_direct_snapshots_keep_search_geometry_separate() {
    for addressed in [true, false] {
        let first = snapshot("first page", addressed);
        let second = snapshot(
            "界 needle second page with a different line width",
            addressed,
        );
        let mut app = App::from_shared(ReaderOptions {
            current: Arc::clone(&first),
            catalog: DocumentCatalog::default(),
            scope: vec![Arc::clone(&second)],
        });
        // The equal address (including None) must not hide the current source.
        assert_eq!(app.scope_documents.len(), 2);
        assert!(Arc::ptr_eq(&app.scope_documents[0], &first));
        assert!(Arc::ptr_eq(&app.scope_documents[1], &second));
        let mut terminal = Terminal::new(TestBackend::new(64, 16)).expect("terminal");
        terminal
            .draw(|frame| app.draw(frame))
            .expect("draw initial");
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for character in "needle".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(Arc::ptr_eq(&app.session.current_bundle, &second));
        assert_eq!(app.search.scope_matches.len(), 1);
        assert_eq!(app.search.matches.len(), 1);
        assert_eq!(app.active_rendered_search_match(), Some(0));
        let expected = crate::DocumentView::new(&second)
            .render(app.geometry.content.width.max(1))
            .search("needle");
        assert_eq!(app.search.matches[0].row, expected[0].row);
        assert_eq!(app.search.matches[0].start_column, expected[0].start_column);
        terminal
            .draw(|frame| app.draw(frame))
            .expect("draw selected revision");
        assert!(terminal.backend().to_string().contains("needle"));
        app.navigate_history(true);
        if addressed {
            // A same-address older snapshot is not a local jump in this revision.
            assert!(app.take_open_request().is_some());
            assert!(Arc::ptr_eq(&app.session.current_bundle, &second));
            assert_eq!(app.navigation.history_lengths(), (1, 0));
        } else {
            assert!(app.take_open_request().is_none());
            assert!(Arc::ptr_eq(&app.session.current_bundle, &first));
            assert_eq!(app.navigation.history_lengths(), (0, 1));
        }
    }
}

#[test]
fn candidate_failure_keeps_shared_owner_and_qualified_history_is_weak() {
    let first = snapshot("original", true);
    let weak = Arc::downgrade(&first);
    let mut app = App::from_shared(ReaderOptions::new(Arc::clone(&first)));
    let candidate = snapshot("replacement", true);
    app.complete_loaded_navigation(
        Arc::clone(&candidate),
        LocalTarget::ReferenceOccurrence("unavailable-private-occurrence".into()),
        HistoryDirection::New,
    );
    assert!(Arc::ptr_eq(&app.session.current_bundle, &first));
    assert_eq!(app.navigation.history_lengths(), (0, 0));
    app.complete_loaded_navigation(
        Arc::clone(&candidate),
        LocalTarget::Default,
        HistoryDirection::New,
    );
    assert!(Arc::ptr_eq(&app.session.current_bundle, &candidate));
    let (historical, _) = app.navigation.plan_history(true).expect("history");
    assert!(historical.belongs_to(&first));
    assert!(!historical.belongs_to(&candidate));
    assert!(historical.fallback().is_none());
    // Release explicit owners; qualified history/tab provenance cannot retain IR.
    app.scope_documents.clear();
    drop(first);
    assert!(weak.upgrade().is_none());
}
