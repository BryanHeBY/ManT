//! Stable source owners and accepted rows reach the reader's actual cells.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

const WORD: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_receipts/accepted-word.1");
const HARD_ROW: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_receipts/late-hard-row.1");
const LINK: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_receipts/accepted-unicode-link.1");
const LABEL: &str = "界e\u{301}字";
const BACKLINE: &str = include_str!(
    "../../mant-engine/tests/roff_lowering/word_projection/backline-after-zero-width.1"
);
const UNPRINTED: &str = include_str!(
    "../../mant-engine/tests/roff_lowering/word_projection/source-prefix-cross-pass.1"
);
const REARM: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_projection/rearm-prefix-space.1");
const LINK_REARM: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_projection/link-rearm.1");
const DEFERRED_MARKER: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_passes/deferred-marker.1");
const LINKED_MARKER: &str =
    include_str!("../../mant-engine/tests/roff_lowering/word_passes/linked-marker.1");
const EMPTY_LINK_BEFORE_PP: &str = include_str!(
    "../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-link-before-pp.1"
);
const LINK_SPACES: [&str; 3] = [
    include_str!("../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-1.1"),
    include_str!("../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-2.1"),
    include_str!("../../mant-engine/tests/roff_lowering/word_projection/link-source-spaces-3.1"),
];

fn only_position(buffer: &Buffer, text: &str) -> (u16, u16) {
    let hits = positions(buffer, text);
    assert_eq!(hits.len(), 1, "{text:?}: {hits:?}");
    hits[0]
}

fn paint(content: &ResolvedContent, width: u16) -> (App, Terminal<TestBackend>) {
    let mut app = App::new(content);
    app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(width + 4, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    (app, terminal)
}

fn copy(app: &mut App, at: (u16, u16), expected: &str) {
    let (column, row) = at;
    let last = column + u16::try_from(expected.width()).unwrap() - 1;
    select_span(app, column, row, last, None);
    assert_eq!(
        copied_selections(app, |request| match request {
            CopyRequest::Selection { text } => text,
            _ => panic!("expected reader selection"),
        }),
        [expected]
    );
}

fn assert_rows(
    content: &ResolvedContent,
    width: u16,
    expected: &[&str],
    word: &str,
    link_blanks: Option<u16>,
) {
    let (mut app, terminal) = paint(content, width);
    let buffer = terminal.backend().buffer();
    let (origin, first) = only_position(buffer, expected[0]);
    for (offset, expected) in expected.iter().enumerate() {
        let at = only_position(buffer, expected);
        assert_eq!(at, (origin, first + u16::try_from(offset).unwrap()));
        let visible_end = origin + u16::try_from(expected.width()).unwrap();
        // The last two cells are outside the body. Every cell after the
        // expected row must be empty, including any unexpected suffix.
        assert!(
            (visible_end..width + 2).all(|x| buffer[(x, at.1)].symbol() == " "),
            "{expected:?}: unexpected right-hand cells at width {width}"
        );
    }
    let (column, row) = only_position(buffer, word);
    let rendered = DocumentView::new(content).render(width);
    let hits = rendered.search(word);
    assert_eq!(hits.len(), 1);
    let heading = rendered.anchor_row("description").unwrap();
    assert_eq!(
        hits[0].row - heading,
        usize::from(row - only_position(buffer, "DESCRIPTION").1)
    );
    assert_eq!(hits[0].end_column - hits[0].start_column, word.width());
    if let Some(link_blanks) = link_blanks {
        let first = column - link_blanks;
        let after = column + u16::try_from(word.width()).unwrap();
        for cell in first - 1..=after {
            let expected = (first..after).contains(&cell).then_some("https://x.io");
            click(&mut app, cell, row);
            assert_eq!(
                opened_targets(&mut app),
                expected.into_iter().collect::<Vec<_>>(),
                "click at column {cell}, visible link at {first}..{after}"
            );
        }
        let label = format!("{}{word}", " ".repeat(usize::from(link_blanks)));
        copy(&mut app, (first, row), &label);
    } else {
        copy(&mut app, (column, row), word);
    }
}

#[test]
fn accepted_source_rows_survive_json_at_two_actual_reader_widths() {
    // Exact .1 sources ran against all five pristine profiles. The two
    // columns of lead space come from distinct term_word() writes. In the
    // delayed case term_flushln() ends ALPHA before X is published.
    for (source, expected, word, blanks) in [
        (WORD, &["ALPHAX  BETA", "OMEGA"][..], "BETA", None),
        (HARD_ROW, &["ALPHA", "XBETA", "OMEGA"][..], "XBETA", None),
        (
            LINK,
            &["ALPHAX  界e\u{301}字: https://x.io", "OMEGA"][..],
            LABEL,
            Some(1),
        ),
    ] {
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
        );
        for width in [40, 78] {
            assert_rows(&content, width, expected, word, blanks);
        }
    }
}

fn visit_readbacks(source: &str, mut visit: impl FnMut(&ResolvedContent)) {
    let original = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
    );
    visit(&original);
    for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
        let mut content = original.clone();
        for _ in 0..2 {
            let markdown = render_markdown_with_options(&content, options);
            content = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
            visit(&content);
        }
    }
}

#[test]
fn overwritten_cells_and_hard_rows_remain_searchable_after_reader_readbacks() {
    // Pinned term_field() prints advances only before real output cells.
    // These exact sources ran all five pristine reference profiles.
    for (source, expected, word) in [
        (BACKLINE, &["ALPHAXBETA", "OMEGA"][..], "ALPHAXBETA"),
        (UNPRINTED, &["ALPHAX", "BETA", "OMEGA"][..], "BETA"),
        (REARM, &["ALPHAX  ETA", "OMEGA"][..], "ETA"),
    ] {
        visit_readbacks(source, |content| {
            for width in [32, 40, 78] {
                assert_rows(content, width, expected, word, None);
            }
        });
    }
}

#[test]
fn successive_passes_keep_the_printed_rows_and_link_cells_after_readback() {
    // The pinned term_field() advances only before an actual output
    // character. The word_passes sources retain the exact native rows.
    for (source, rows, word, link_blanks) in [
        (
            DEFERRED_MARKER,
            &["ALPHAX", " BETA", "OMEGA"][..],
            "BETA",
            None,
        ),
        (
            LINKED_MARKER,
            &["ALPHAX", "YBETA:", "https://x.io", "OMEGA"][..],
            "YBETA",
            Some(0),
        ),
    ] {
        visit_readbacks(source, |content| {
            for width in [20, 40, 78] {
                assert_rows(content, width, rows, word, link_blanks);
            }
        });
    }
}

#[test]
fn reader_keeps_the_rejected_tail_row_before_a_positive_paragraph_gap() {
    // The pinned formatter emits an empty term_flushln() tail and then
    // a separate term_vspace(). Neither gives the rejected link a label.
    visit_readbacks(EMPTY_LINK_BEFORE_PP, |content| {
        for width in [20, 40, 78] {
            let (mut app, terminal) = paint(content, width);
            let buffer = terminal.backend().buffer();
            let (column, first_row) = only_position(buffer, "ALPHAX");
            let (next_column, next_row) = only_position(buffer, "OMEGA");
            assert_eq!((next_column, next_row), (column, first_row + 3));
            assert!(
                (first_row + 1..next_row)
                    .all(|row| (column..width + 2).all(|x| buffer[(x, row)].symbol() == " "))
            );
            click(&mut app, column + 6, first_row);
            assert_eq!(opened_targets(&mut app), Vec::<String>::new());
            copy(&mut app, (next_column, next_row), "OMEGA");
            let rendered = DocumentView::new(content).render(width);
            let heading = rendered.anchor_row("description").unwrap();
            let hits = rendered.search("OMEGA");
            assert_eq!(hits.len(), 1);
            assert_eq!(
                hits[0].row - heading,
                usize::from(next_row - only_position(buffer, "DESCRIPTION").1)
            );
        }
    });
}

#[test]
fn printed_link_blanks_keep_their_click_and_copy_ranges_after_reader_readbacks() {
    // An automatic term_word() advance can represent one retreated raw
    // blank in its original link. Other automatic advances stay outside
    // that label. The click probe includes both neighbors of each label.
    for (index, source) in LINK_SPACES.into_iter().enumerate() {
        let line = format!("ALPHAX{}{LABEL}: https://x.io", " ".repeat(index + 1));
        let link_blanks = u16::try_from(index.max(1)).unwrap();
        visit_readbacks(source, |content| {
            for width in [32, 40, 78] {
                assert_rows(content, width, &[&line, "OMEGA"], LABEL, Some(link_blanks));
            }
        });
    }
    visit_readbacks(LINK_REARM, |content| {
        for width in [32, 40, 78] {
            assert_rows(
                content,
                width,
                &["ALPHAX ETA: https://x.io", "OMEGA"],
                "ETA",
                Some(0),
            );
        }
    });
}
