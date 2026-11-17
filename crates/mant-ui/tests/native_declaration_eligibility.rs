//! Declaration name filtering preserves painted and selectable HEAD text.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend};
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

#[test]
fn placeholder_and_later_declaration_remain_readable_and_selectable() {
    // Fixed CVS keeps the quoted comma inside Ar's operand. The later
    // delimiter is printed outside Ar. Neither disappears from the HEAD.
    let source = concat!(
        ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n",
        ".Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev ENV_ Ns Ar \"KEY, HIDDEN\" , Ev VALID\nBODY\n.El\n",
    );
    let native = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
    );
    let markdown = mant_codec::encode::render_markdown(&native);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&native, &readback] {
        for width in [12, 40, 78] {
            let rendered = DocumentView::new(content).render(width);
            let mut app = App::new(content);
            app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
            let mut terminal = Terminal::new(TestBackend::new(width + 4, 48)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            for text in ["ENV_KEY", "HIDDEN", "VALID", "BODY"] {
                let found = positions(buffer, text);
                assert_eq!(found.len(), 1, "width {width}: one painted {text}");
                assert_eq!(rendered.search(text).len(), 1, "width {width}: {text}");
                let (column, row) = found[0];
                click(&mut app, column, row);
                assert!(opened_targets(&mut app).is_empty(), "width {width}: {text}");
                select_span(
                    &mut app,
                    column,
                    row,
                    column + u16::try_from(text.width()).unwrap() - 1,
                    None,
                );
                assert_eq!(
                    copied_selections(&mut app, |request| {
                        let CopyRequest::Selection { text } = request else {
                            panic!("expected pointer selection");
                        };
                        text
                    }),
                    [text]
                );
            }
        }
    }
}

#[test]
fn a_search_template_and_its_following_command_group_keep_original_reader_targets() {
    // Pinned mdoc_term.c carries Sm off across Sh. The joined sign and
    // argument still form one visible label, and only N owns the prose.
    let source = concat!(
        ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n",
        ".Sm off\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It No ? Ar RE\n",
        ".It Cm n\n.It Cm N\nGROUP_BODY\n.El\n",
    );
    check_search_form_projection(source, "?RE");
}

#[test]
fn styled_and_encoded_search_operands_survive_reader_roundtrips() {
    let prefix = ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n.Sh COMMANDS\n.Sm off\n.Bl -tag -width Ds\n";
    for (head, form) in [
        (r".It Em \[u003F] Ar RE", "?RE"),
        (r".It Bq Li \z\N'47' Ar RE", "[RE]"),
    ] {
        let source = format!("{prefix}{head}\n.It Cm n\n.It Cm N\nGROUP_BODY\n.El\n");
        check_search_form_projection(&source, form);
    }
}

fn check_search_form_projection(source: &str, form: &str) {
    let content = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
    );
    let markdown = mant_codec::encode::render_markdown(&content);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&content, &readback] {
        for width in [20, 40, 78] {
            let rendered = DocumentView::new(content).render(width);
            for term in [form, "GROUP_BODY"] {
                assert_eq!(rendered.search(term).len(), 1, "width {width}: {term}");
            }
        }
    }
}

#[test]
fn parameter_and_independent_option_remain_visible_with_the_same_spelling() {
    // termp_fl_pre and Ar each write their own word in this Bq. The
    // native form contains both identical spellings.
    let source = concat!(
        ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n",
        ".Sh OPTIONS\n.Sm off\n.Bl -tag -width Ds\n.It Bq Sy \\z/ Ar \"RE,-q\" , Fl q\n",
        ".It Fl n\n.It Fl N\nGROUP_BODY\n.El\n",
    );
    let content = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
    );
    let markdown = mant_codec::encode::render_markdown(&content);
    let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&content, &imported] {
        for width in [20, 40, 78] {
            let view = DocumentView::new(content).render(width);
            assert_eq!(view.search("-q").len(), 2, "width {width}");
            assert_eq!(view.search("GROUP_BODY").len(), 1, "width {width}");
        }
    }
}
