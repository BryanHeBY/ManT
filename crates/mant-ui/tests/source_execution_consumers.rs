//! Accepted native words and physical rows retain their reader boundaries.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, ResolvedContent, inline_plain_text};
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

const MAN: &str = ".TH CHECK 1 \"2026-11-09\"\n.SH NAME\ncheck \\- source audit\n.SH DESCRIPTION\n";
const MDOC: &str = ".Dd November 9, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd source audit\n.Sh DESCRIPTION\n";

fn visit_projections(source: &str, mut check: impl FnMut(&ResolvedContent, bool)) {
    let original = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
    );
    check(&original, false);
    for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
        let mut content = original.clone();
        for _ in 0..2 {
            let markdown = render_markdown_with_options(&content, options);
            content = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
            check(&content, true);
        }
    }
}

fn paint(content: &ResolvedContent, width: u16) -> (App, Terminal<TestBackend>) {
    let mut app = App::new(content);
    app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
    // Two outer cells and the scrollbar gutter are outside the document.
    let mut terminal = Terminal::new(TestBackend::new(width + 4, 48)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    (app, terminal)
}

fn position(buffer: &Buffer, text: &str) -> (u16, u16) {
    let found = positions(buffer, text);
    assert_eq!(found.len(), 1, "one painted {text}: {found:?}");
    found[0]
}

fn copy_span(app: &mut App, (column, row): (u16, u16), text: &str) {
    let last = column + u16::try_from(text.width()).unwrap() - 1;
    select_span(app, column, row, last, None);
    assert_eq!(
        copied_selections(app, |request| {
            let CopyRequest::Selection { text } = request else {
                panic!("expected pointer selection");
            };
            text
        }),
        [text]
    );
}

#[test]
fn empty_operands_keep_the_accepted_word_and_generated_colon() {
    // The exact fixtures ran pristine in all five profiles. term_word() can
    // queue spaces on empty operands; term_field() prints them with a later
    // glyph. termp_lk_pre executes the label and colon even without a URI.
    for (body, expected) in [
        (
            "BEFORE\n.Lk \"\" \"\" LABEL\n.br\nAFTER\n",
            "BEFORE  LABEL:",
        ),
        ("BEFORE\n.Lk \"\" \"\"\n.br\nAFTER\n", "BEFORE :"),
        (
            "BEFORE\n.No \"\"\n.No LABEL Ns :\n.No \"\"\n.br\nAFTER\n",
            "BEFORE  LABEL:",
        ),
        (
            "BEFORE\n.No \"\" No LABEL Ns : No \"\"\n.br\nAFTER\n",
            "BEFORE  LABEL:",
        ),
    ] {
        visit_projections(&format!("{MDOC}{body}"), |content, _| {
            for width in [40, 78] {
                let (mut app, terminal) = paint(content, width);
                let buffer = terminal.backend().buffer();
                let start = position(buffer, expected);
                assert_eq!(position(buffer, "AFTER").1, start.1 + 1);
                assert_eq!(
                    DocumentView::new(content)
                        .render(width)
                        .search(expected)
                        .len(),
                    1
                );
                copy_span(&mut app, start, expected);
            }
        });
    }
}

#[test]
fn accepted_label_keeps_precise_activation_and_copy_boundaries() {
    // The complete pristine source has two spaces between BEFORE and LABEL.
    // An empty word preceding Lk belongs outside the link. Only LABEL owns a
    // typed hit range, and the generated colon and URI stay separate.
    let source = format!("{MDOC}BEFORE\n.No \"\"\n.Lk https://x.io LABEL\n.No \"\"\n.br\nAFTER\n");
    visit_projections(&source, |content, _| {
        for width in [40, 78] {
            let (mut app, terminal) = paint(content, width);
            let buffer = terminal.backend().buffer();
            let before = position(buffer, "BEFORE  LABEL: https://x.io");
            let (label, row) = position(buffer, "LABEL");
            assert_eq!((label, row), (before.0 + 8, before.1));
            assert_eq!(position(buffer, "AFTER").1, row + 1);
            for column in [label, label + 4] {
                click(&mut app, column, row);
                assert_eq!(opened_targets(&mut app), ["https://x.io"]);
            }
            for column in [label - 2, label - 1, label + 5, label + 7] {
                click(&mut app, column, row);
                assert!(opened_targets(&mut app).is_empty(), "{column} of {label}");
            }
            copy_span(&mut app, (label, row), "LABEL");
        }
    });
}

#[test]
fn empty_group_flush_resets_zero_advance_but_empty_newline_does_not() {
    // CVS roff_term_pre_ce unconditionally calls term_flushln after each
    // group, clearing BACKAFTER even without buffered cells. Ordinary .br
    // calls term_newln, whose empty-buffer gate preserves the same state.
    for (header, body, first, next, next_offset, last, last_offset) in [
        (
            MAN,
            "BEFORE\n.ce 2\n\\z\n NEXT\nAFTER\n",
            "BEFORE",
            "NEXT",
            2,
            "AFTER",
            3,
        ),
        (
            MAN,
            "BEFORE\n.ce 1\n\\z\n NEXT\nAFTER\n",
            "BEFORE",
            "NEXT",
            2,
            "AFTER",
            2,
        ),
        (
            MAN,
            "BEFORE\n.rj 2\n\\z\n NEXT\nAFTER\n",
            "BEFORE",
            "NEXT",
            2,
            "AFTER",
            3,
        ),
        (
            MAN,
            "BEFORE\n.ce 2\n\\z\n NEXT\n.PP\nAFTER\n",
            "BEFORE",
            "NEXT",
            2,
            "AFTER",
            4,
        ),
        (
            MDOC,
            "BEFORE\n.Bo\n.ce 2\n\\z\n NEXT\n.Bc\nAFTER\n",
            "BEFORE [",
            "NEXT",
            2,
            "] AFTER",
            3,
        ),
        (
            MAN,
            "BEFORE\n.br\n\\z\n.br\n NEXT\nAFTER\n",
            "BEFORE",
            "EXT AFTER",
            1,
            "AFTER",
            1,
        ),
    ] {
        visit_projections(&format!("{header}{body}"), |content, _| {
            for width in [40, 78] {
                let (_, terminal) = paint(content, width);
                let buffer = terminal.backend().buffer();
                let (_, start) = position(buffer, first);
                assert_eq!(position(buffer, next).1, start + next_offset, "{body}");
                let trailing_row = position(buffer, last).1;
                assert_eq!(trailing_row, start + last_offset, "{body}");
                if next.starts_with("EXT") {
                    assert_eq!(positions(buffer, "NEXT"), [] as [(u16, u16); 0]);
                }
            }
        });
    }
}

#[test]
fn completed_empty_alignment_and_paragraph_gap_survive_reader_resize() {
    // The same complete source was rendered by pinned CVS in the engine
    // fixture. roff_term_pre_ce() flushes one empty physical row before the
    // distinct PP spacing request. Exercise both Markdown readbacks too.
    let source =
        include_str!("../../mant-engine/tests/roff_lowering/grouped_line_flush/empty-alignment.1");
    visit_projections(source, |content, _| {
        let (mut app, mut terminal) = paint(content, 78);
        for width in [78, 20, 40, 78] {
            terminal.backend_mut().resize(width + 4, 48);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            let (column, before) = position(buffer, "BEFOREMARK");
            let after = position(buffer, "AFTERMARK");
            assert_eq!(after.1, before + 3, "reader width {width}");
            for row in before + 1..after.1 {
                for cell in column..column + width {
                    assert_eq!(buffer[(cell, row)].symbol(), " ", "{cell},{row}");
                }
            }
            copy_span(&mut app, after, "AFTERMARK");
        }
    });
}

#[test]
fn literal_tab_groups_close_each_physical_row_once() {
    // Each pristine ce/rj group closes one native row. TUI may expand a
    // source TAB across cells, but an additional synthetic row is forbidden.
    // The ordinary TEXT controls also protect two consecutive literal rows.
    for (body, blanks) in [
        (".ce 2\n\t\n NEXT\n", 1),
        (".rj 2\n\t\n NEXT\n", 1),
        (".ce 3\n\t\n\t\n NEXT\n", 2),
        (".ce 1\n\t\n.br\n NEXT\n", 2),
        ("\t\n NEXT\n", 1),
        ("\t\n\t\n NEXT\n", 2),
    ] {
        let source = format!("{MAN}.nf\nBEFORE\n{body}AFTER\n");
        visit_projections(&source, |content, readback| {
            let tabs: usize = content
                .document
                .as_ref()
                .unwrap()
                .sections
                .iter()
                .flat_map(|section| &section.blocks)
                .filter_map(|block| match block {
                    Block::Preformatted { children, .. } => Some(inline_plain_text(children)),
                    _ => None,
                })
                .map(|text| text.matches('\t').count())
                .sum();
            assert_eq!(tabs, body.matches('\t').count(), "{source}");
            for width in [40, 78] {
                let (_, terminal) = paint(content, width);
                let buffer = terminal.backend().buffer();
                let (_, before) = position(buffer, "BEFORE");
                let (_, next) = position(buffer, "NEXT");
                assert_eq!(
                    next,
                    before + blanks + 1,
                    "{source}, Markdown readback: {readback}"
                );
                assert_eq!(
                    position(buffer, "AFTER").1,
                    next + 1,
                    "{source}, Markdown readback: {readback}"
                );
                for row in before + 1..next {
                    assert!(
                        (1..width).all(|column| buffer[(column, row)].symbol() == " "),
                        "{source}: painted content in blank row {row}"
                    );
                }
            }
        });
    }
}

#[test]
fn empty_mdoc_sibling_remains_a_word_while_visited_empty_lines_remain_vertical() {
    // In closes after one argument in mdoc_macro.c::in_line_argn. Its empty
    // sibling has no NODE_LINE and term_word supplies its separator. Empty
    // physical TEXT has NODE_LINE and requests a vertical row. Native
    // term_vspace consumes an outstanding negative spacing debt first.
    for (header, body, left, offset) in [
        (
            MDOC,
            "BEFORE\n.In header.h \"\"\nAFTER\n",
            "BEFORE <header.h>  AFTER",
            0,
        ),
        (MDOC, "BEFORE\n\nAFTER\n", "BEFORE", 2),
        (MAN, "BEFORE\n\nAFTER\n", "BEFORE", 2),
        (MDOC, "BEFORE\n.sp -1\n\nAFTER\n", "BEFORE", 1),
    ] {
        let source = format!("{header}{body}");
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
        );
        for width in [40, 78] {
            let (_, terminal) = paint(&content, width);
            let buffer = terminal.backend().buffer();
            assert_eq!(
                position(buffer, "AFTER").1,
                position(buffer, left).1 + offset,
                "{source}"
            );
        }
        if offset == 0 {
            visit_projections(&source, |content, _| {
                for width in [40, 78] {
                    let (mut app, terminal) = paint(content, width);
                    let buffer = terminal.backend().buffer();
                    let before = position(buffer, "BEFORE");
                    let include = position(buffer, "<header.h>");
                    let after = position(buffer, "AFTER");
                    assert_eq!((before.1, after.1), (include.1, include.1), "{source}");
                    assert!(before.0 < include.0 && include.0 < after.0, "{source}");
                    copy_span(&mut app, include, "<header.h>");
                }
            });
        }
    }
}

#[path = "support/terminal_cells.rs"]
mod terminal_cells;

#[test]
fn hanging_alignment_retains_the_live_paragraph_and_its_viewport_origins() {
    // Both exact sources ran with the pristine five profiles. pre_HP sets
    // the hanging width; roff_term_pre_ce calls pre_br, then closes the
    // two aligned rows without requesting new paragraph spacing.
    let cases = [
        (
            include_str!(
                "../../mant-engine/tests/roff_lowering/grouped_line_flush/hanging-wrap-ce.1"
            ),
            true,
        ),
        (
            include_str!(
                "../../mant-engine/tests/roff_lowering/grouped_line_flush/hanging-wrap-rj.1"
            ),
            false,
        ),
    ];
    for (source, centered) in cases {
        visit_projections(source, |content, readback| {
            for width in [20, 40, 78] {
                let rendered = DocumentView::new(content).render(width);
                let buffer = terminal_cells::paint(&rendered, width);
                let first = rendered.anchor_row("description").unwrap() + 1;
                let origin = 3; // top-level section body margin
                let hanging = origin + if readback { 0 } else { 4 };
                let width_usize = usize::from(width);
                let aligned = if readback {
                    origin
                } else if centered {
                    hanging + (width_usize - hanging - "ROWONE".len()) / 2
                } else {
                    width_usize - "ROWONE".len()
                };
                let mut rows = vec![(origin, "FIRSTMARK second continuation")];
                if width == 20 {
                    rows = vec![(origin, "FIRSTMARK second"), (hanging, "continuation")];
                }
                rows.extend([
                    (aligned, "ROWONE"),
                    (aligned, "ROWTWO"),
                    (hanging, "AFTERMARK"),
                ]);
                for (offset, (origin, text)) in rows.into_iter().enumerate() {
                    terminal_cells::assert_row(
                        &buffer,
                        first + offset,
                        origin,
                        text,
                        &format!("{width}/{centered}/{readback}"),
                    );
                }
                assert_eq!(rendered.search("AFTERMARK").len(), 1);
                let (mut app, terminal) = paint(content, width);
                copy_span(
                    &mut app,
                    position(terminal.backend().buffer(), "AFTERMARK"),
                    "AFTERMARK",
                );
            }
        });
    }
}
