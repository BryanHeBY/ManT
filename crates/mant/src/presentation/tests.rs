//! Presentation contracts: style must not alter visible semantic output.

#[test]
fn semantic_term_and_value_are_not_muted_metadata() {
    use super::terminal::{TerminalRole, terminal_style};
    let term = terminal_style(TerminalRole::Entry(mant_ir::EntryKind::Term));
    let value = terminal_style(TerminalRole::Entry(mant_ir::EntryKind::Value));
    let muted = terminal_style(TerminalRole::Muted);
    assert_ne!(term, muted);
    assert_ne!(value, muted);
    assert!(!term.to_string().contains("90m"));
    assert!(value.to_string().contains("94m"));
    assert_ne!(
        terminal_style(TerminalRole::Match),
        terminal_style(TerminalRole::Entry(mant_ir::EntryKind::Command))
    );
}
use mant_engine::{project_query_view, query_markdown_text};
use mant_protocol::{EntryProjection, QueryView};

use super::{OutputTarget, QueryFormat, RenderOptions, render_query_result};

const PAGE: &str = r"# demo

## Options

<!-- mant:entries role=option -->
- `--color WHEN`: Select terminal colour behavior.

The selected color is visible in terminal output.
";

#[test]
fn terminal_styles_do_not_change_visible_query_text() {
    for view in [
        QueryView::Full {},
        QueryView::Outline {
            entries: EntryProjection::All,
            root: None,
        },
        QueryView::Explain {
            entry: "--color".to_owned(),
            options: mant_protocol::ExplanationOptions::default(),
        },
        QueryView::Search {
            pattern: "color".to_owned(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Insensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 1,
            limit: 100,
            offset: 0,
        },
    ] {
        let query = query_markdown_text(PAGE, None).expect("Markdown query");
        let result = project_query_view(query, &view).expect("query projection");
        let plain = render_query_result(
            &result,
            options(QueryFormat::Text, false, OutputTarget::Stream),
        )
        .expect("plain terminal text");
        let colored = render_query_result(
            &result,
            options(QueryFormat::Text, true, OutputTarget::Terminal),
        )
        .expect("colored terminal text");
        assert!(colored.contains("\x1b["));
        assert_eq!(strip_ansi(&colored), plain);
    }
}

#[test]
fn terminal_outline_reports_an_empty_kind_projection() {
    let query = query_markdown_text(PAGE, None).expect("Markdown query");
    let result = project_query_view(
        query,
        &QueryView::Outline {
            entries: EntryProjection::Kinds {
                kinds: vec![mant_ir::EntryKind::EnvironmentVariable],
            },
            root: None,
        },
    )
    .expect("empty environment outline");

    let rendered = render_query_result(
        &result,
        options(QueryFormat::Text, true, OutputTarget::Terminal),
    )
    .expect("terminal outline");
    assert_eq!(
        rendered,
        "stdin\n0 matching semantic entries for: environment variables"
    );
}

#[test]
fn structured_and_markdown_formats_never_receive_terminal_styles() {
    let query = query_markdown_text(PAGE, None).expect("Markdown query");
    let result = project_query_view(
        query,
        &QueryView::Explain {
            entry: "--color".to_owned(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .expect("explanation");
    for format in [QueryFormat::Markdown, QueryFormat::Json] {
        let rendered = render_query_result(&result, options(format, true, OutputTarget::Stream))
            .expect("deterministic output");
        assert!(!rendered.contains("\x1b["));
    }
}

#[test]
fn uncoloured_terminal_presentations_mask_controls_in_direct_input_labels() {
    let source_path = "ev\u{1b}[31mil.md".to_owned();
    let views = [
        QueryView::Full {},
        QueryView::Outline {
            entries: EntryProjection::All,
            root: None,
        },
        QueryView::Explain {
            entry: "--color".to_owned(),
            options: mant_protocol::ExplanationOptions::default(),
        },
        QueryView::Search {
            pattern: "color".to_owned(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Insensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 1,
            limit: 100,
            offset: 0,
        },
    ];

    for view in views {
        let query = query_markdown_text(PAGE, Some(source_path.clone()))
            .expect("Markdown query with hostile label");
        let result = project_query_view(query, &view).expect("query projection");
        let rendered = render_query_result(
            &result,
            options(QueryFormat::Text, false, OutputTarget::Terminal),
        )
        .expect("plain terminal text");

        assert!(!rendered.contains('\u{1b}'));
        assert!(rendered.contains("ev�[31mil.md"));
    }
}

#[test]
fn terminal_markdown_masks_dynamic_controls_without_rewriting_redirected_data() {
    for view in [
        QueryView::Full {},
        QueryView::Outline {
            entries: EntryProjection::All,
            root: None,
        },
        QueryView::Explain {
            entry: "--color".to_owned(),
            options: mant_protocol::ExplanationOptions::default(),
        },
        QueryView::Search {
            pattern: "color".to_owned(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Insensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 1,
            limit: 100,
            offset: 0,
        },
    ] {
        let mut query = query_markdown_text(PAGE, Some("ris\u{1b}c.md".to_owned()))
            .expect("Markdown query with hostile label");
        query.document.as_mut().expect("parsed document").meta.title =
            Some("ris\u{1b}c".to_owned());
        let result = project_query_view(query, &view).expect("query projection");
        let redirected = render_query_result(
            &result,
            options(QueryFormat::Markdown, false, OutputTarget::Stream),
        )
        .expect("redirected Markdown");
        let terminal = render_query_result(
            &result,
            options(QueryFormat::Markdown, false, OutputTarget::Terminal),
        )
        .expect("terminal Markdown");

        if matches!(view, QueryView::Explain { .. }) {
            // The new evidence presentation is safe even for unchecked public IR.
            assert!(!redirected.contains('\u{1b}'), "{redirected:?}");
        } else {
            assert!(redirected.contains('\u{1b}'), "{view:?}: {redirected:?}");
        }
        assert!(!terminal.contains('\u{1b}'), "{view:?}: {terminal:?}");
        assert!(terminal.contains("ris�c"));
    }
}

const fn options(format: QueryFormat, color: bool, target: OutputTarget) -> RenderOptions {
    RenderOptions {
        format,
        pretty: true,
        preserve_anchors: false,
        color,
        target,
    }
}

fn strip_ansi(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
            index += 2;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if byte.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            let character = value[index..].chars().next().expect("UTF-8 character");
            output.push(character);
            index += character.len_utf8();
        }
    }
    output
}
