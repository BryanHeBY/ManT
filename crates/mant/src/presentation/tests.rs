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
fn explanation_ansi_uses_the_exact_same_unframed_report_as_plain_text() {
    // Keep the packaged unit test independent of sibling integration fixtures.
    let source = b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -x, --language=LANG\nSelect language.\n.TP\n.B -Q\nUse -x as well.\n.SH NOTES\n-xylophone is not the same as -x.\n";
    let content = mant_engine::query_roff_bytes(source).unwrap();
    let result = mant_engine::explain_query(
        &content,
        &mant_protocol::ExplanationQuery {
            entry: "-x".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .unwrap();
    let plain = super::terminal::render_terminal_explanation(&result, false);
    let colored = super::terminal::render_terminal_explanation(&result, true);
    assert_eq!(strip_ansi(&colored), plain);
    assert!(plain.contains("\nForms:\n-Q"));
    assert!(plain.contains("\nDefinition:\n-x, --language=LANG"));
    assert!(!plain.lines().any(|line| line.starts_with("| ")));
    let colors = visible_colors(&colored);
    for (offset, _) in plain.match_indices("-xylophone") {
        assert_ne!(colors[plain[..offset].chars().count()].1, Some(92));
    }
    let decoded = serde_json::from_slice(&serde_json::to_vec(&result).unwrap()).unwrap();
    assert_eq!(
        super::terminal::render_terminal_explanation(&decoded, true),
        colored
    );
}

#[test]
fn full_and_node_color_validated_names_without_prefix_guessing() {
    let source = b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -x, --language=LANG\nSelect language.\n.SH NOTES\n-xylophone is not an option.\n";
    for view in [
        QueryView::Full {},
        QueryView::Excerpt {
            selectors: vec!["1".into(), "2".into()],
        },
    ] {
        let query = mant_engine::query_roff_bytes(source).unwrap();
        let result = project_query_view(query, &view).unwrap();
        let plain = render_query_result(
            &result,
            options(QueryFormat::Text, false, OutputTarget::Stream),
        )
        .unwrap();
        let colored = render_query_result(
            &result,
            options(QueryFormat::Text, true, OutputTarget::Terminal),
        )
        .unwrap();
        assert_eq!(strip_ansi(&colored), plain);
        let colors = visible_colors(&colored);
        for name in ["-x,", "--language=", "LANG", "-xylophone"] {
            let byte = plain.find(name).unwrap();
            let start = plain[..byte].chars().count();
            let expected = matches!(name, "-x," | "--language=").then_some(92);
            assert_eq!(colors[start].1, expected, "{name}: {colored:?}");
        }
        assert!(colored.contains("-xylophone is not an option."));
    }
}

#[test]
fn source_styling_preserves_whitespace_only_blocks_nested_terms_and_tables() {
    let source = ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --help\n.RS 4\n.sp 2\n.B nested\n.RE\n.TS\nl l.\nleft\tright\n.TE\n.nf\n  code\n\n    tail\n.fi\n";
    let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
    let plain = mant_engine::render_query_text(&query);
    let colored = mant_engine::render_query_text_with(&query, |style, text| {
        super::content::decorate(style, text, true)
    });
    assert_eq!(strip_ansi(&colored), plain);
    assert_eq!(
        mant_engine::render_query_text_with(&query, |_, text| text.to_owned()),
        plain
    );
}

fn visible_colors(text: &str) -> Vec<(char, Option<u16>)> {
    let mut chars = text.chars();
    let mut color = None;
    let mut output = Vec::new();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            assert_eq!(chars.next(), Some('['));
            let params: String = chars.by_ref().take_while(|c| *c != 'm').collect();
            for parameter in params.split(';') {
                match parameter.parse::<u16>().unwrap_or(0) {
                    0 | 39 => color = None,
                    foreground @ (30..=37 | 90..=97) => color = Some(foreground),
                    _ => {}
                }
            }
        } else {
            output.push((ch, color));
        }
    }
    output
}

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
        strip_ansi(&rendered),
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
