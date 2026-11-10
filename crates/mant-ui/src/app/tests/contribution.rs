//! Source contribution, reveal, hit-testing and copying after live reflow.
use super::*;
use mant_ir::{DefinitionLayout, HeadBodyRelation, InlineLayout, LinkTarget};
use unicode_width::UnicodeWidthStr;

const QUERY: &str = "NEEDLE";
const LABEL: &str = "NEEDLE 界e\u{301} TAIL END";
const URI: &str = "https://example.test/needle";
const WIDTHS: [u16; 5] = [78, 20, 8, 40, 78];

#[derive(Clone, Copy, Debug)]
enum Contribution {
    Anchor,
    EmptyLink,
    Zero,
    Gap,
    Hard,
    Literal,
}

impl Contribution {
    fn block(self) -> AstBlock {
        match self {
            Self::Anchor => paragraph(vec![Inline::Anchor {
                id: "target".into(),
                fragment_aliases: vec![],
                owner_source: None,
            }]),
            Self::EmptyLink => paragraph(vec![link("https://example.test/empty", vec![])]),
            Self::Zero | Self::Gap => AstBlock::VerticalSpace {
                lines: u16::from(matches!(self, Self::Gap)),
                source: None,
            },
            Self::Hard => paragraph(vec![Inline::line_break()]),
            Self::Literal => AstBlock::Preformatted {
                children: vec![inline("")],
                inline_layout: InlineLayout::default(),
                language: None,
                layout: LayoutHint::default(),
                source: None,
            },
        }
    }

    fn rows(self) -> usize {
        usize::from(matches!(self, Self::Gap | Self::Hard | Self::Literal))
    }
}

#[derive(Clone, Copy, Debug)]
enum Owner {
    Root,
    Section,
    Definition,
}

fn inline(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}
fn link(uri: &str, children: Vec<Inline>) -> Inline {
    Inline::Link {
        target: LinkTarget::External { uri: uri.into() },
        title: None,
        children,
    }
}
fn paragraph(children: Vec<Inline>) -> AstBlock {
    AstBlock::Paragraph {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn input(owner: Owner, prefix: Option<Contribution>, from_end: bool) -> ResolvedContent {
    let mut blocks = vec![paragraph(vec![inline("BEFORE")])];
    blocks.extend(prefix.map(Contribution::block));
    blocks.push(paragraph(vec![
        inline("A "),
        link(URI, vec![inline(LABEL)]),
        inline(" OUT"),
    ]));
    let alternate = paragraph(vec![inline("NEEDLE ALT")]);
    let filler = (0..24)
        .map(|_| paragraph(vec![inline("Filler")]))
        .collect::<Vec<_>>();
    if from_end {
        blocks.insert(0, alternate);
        blocks.extend(filler);
    } else {
        blocks.splice(0..0, filler);
        blocks.push(alternate);
    }
    let mut value = mant_loader::load_markdown_text("# Catalog\n", None).unwrap();
    let doc = value.document.as_mut().unwrap();
    match owner {
        Owner::Root => doc.blocks = blocks,
        Owner::Section => doc.sections.push(Section {
            id: "subject".into(),
            heading: "Subject".into(),
            fragment_aliases: vec![],
            spacing_before_lines: 0,
            blocks,
            children: vec![],
            source: None,
        }),
        Owner::Definition => doc.blocks.push(AstBlock::DefinitionList {
            items: vec![DefinitionItem {
                terms: vec![vec![inline("HEAD")].into()],
                description: blocks,
                head_body_relation: HeadBodyRelation::Separate,
                layout: DefinitionLayout {
                    body_indent_columns: 2,
                    spacing_before_lines: Some(0),
                    ..DefinitionLayout::default()
                },
                entry: None,
                source: None,
            }],
            declaration_groups: vec![],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }),
    }
    assert_eq!(mant_ir::validate_document(doc), []);
    value
}

pub(super) fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn settle(app: &mut App) {
    let mut now = Instant::now();
    for _ in 0..16 {
        let Some(delay) = app.next_wakeup(now) else {
            break;
        };
        now += delay;
        app.tick(now);
    }
}

pub(super) fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    terminal.draw(|frame| app.draw(frame)).unwrap();
    settle(app);
    terminal.draw(|frame| app.draw(frame)).unwrap();
}

fn pointer(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn verify(app: &mut App, terminal: &mut Terminal<TestBackend>, from_end: bool, id: &str) {
    assert_eq!(app.search.query, QUERY, "confirmed query {id}");
    assert_eq!(app.search.scope_matches.len(), 2, "two occurrences {id}");
    let primary = usize::from(from_end);
    assert_eq!(
        app.search.active_match, primary,
        "active source occurrence {id}"
    );
    let first = app.search.scope_matches[primary].rendered.clone();
    let area = app.geometry.content;
    assert!(
        (app.session.content_scroll..app.session.content_scroll + usize::from(area.height))
            .contains(&first.row),
        "search reveal {id}"
    );
    let rendered = &app.session.rendered_cache[&area.width];
    let start = rendered
        .search("A ")
        .into_iter()
        .find(|hit| hit.row.abs_diff(first.row) <= 1)
        .unwrap();
    let outside = rendered.search("OUT").into_iter().next().unwrap();
    let glyph_cells = label_cells(rendered);
    // Search reveals the matching row; its label may extend below the
    // viewport. Scroll by real reader events to observe the whole label
    // with one row of margin (top cells initiate selection auto-scroll).
    let target = start.row.saturating_sub(1);
    let initial = app.session.content_scroll;
    for _ in 0..initial.abs_diff(target) {
        key(
            app,
            if initial > target {
                KeyCode::Up
            } else {
                KeyCode::Down
            },
        );
    }
    draw(app, terminal);
    let scroll = app.session.content_scroll;
    assert_eq!(scroll, target, "whole linked label scroll {id}");
    let coordinates = |row: usize, column: usize| {
        assert!(
            (scroll..scroll + usize::from(area.height)).contains(&row),
            "source row {row} visible from {scroll} {id}"
        );
        (
            area.x + u16::try_from(column).unwrap(),
            area.y + u16::try_from(row - scroll).unwrap(),
        )
    };
    let first_cell = coordinates(first.row, first.start_column);
    let last = glyph_cells.last().unwrap().1.clone();
    let last_cell = coordinates(last.row, last.end_column - 1);
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[first_cell].symbol(),
        "N",
        "painted first label cell {id}"
    );
    assert_eq!(
        buffer[last_cell].symbol(),
        "D",
        "painted last label cell {id}"
    );
    assert_eq!(
        first.fragment_count(),
        glyph_cells[QUERY.len() - 1].1.row - first.row + 1,
        "match visual range fragments {id}"
    );
    let expected_copy = visual_label(&glyph_cells, buffer, coordinates, id);
    assert_eq!(
        buffer[first_cell].bg,
        theme::interact(
            ratatui::style::Style::default(),
            theme::InteractionRole::SearchActive
        )
        .bg
        .unwrap(),
        "active match is painted at the revealed label {id}"
    );
    for (row, column, expected) in [
        (first.row, first.start_column, Some(URI)),
        (last.row, last.end_column - 1, Some(URI)),
        (start.row, start.start_column, None),
        (outside.row, outside.start_column, None),
    ] {
        click_document_cell(app, column, row);
        assert_eq!(
            app.take_external_request()
                .as_ref()
                .map(crate::ExternalUri::as_str),
            expected,
            "click at {row}:{column} {id}"
        );
    }
    let text = copy_label(app, first_cell, last_cell);
    assert_eq!(text, expected_copy, "copy across visual continuations {id}");
}

fn label_cells(
    rendered: &crate::RenderedDocument,
) -> Vec<(&'static str, crate::RenderedSearchMatch)> {
    let (record, mut byte) = (0..rendered.search_record_count())
        .find_map(|index| {
            rendered
                .search_record_text(index)
                .find(LABEL)
                .map(|byte| (index, byte))
        })
        .expect("one source record owns the whole label");
    let glyphs = [
        "N", "E", "E", "D", "L", "E", " ", "界", "e\u{301}", " ", "T", "A", "I", "L", " ", "E",
        "N", "D",
    ];
    assert_eq!(glyphs.concat(), LABEL);
    glyphs
        .into_iter()
        .filter_map(|glyph| {
            let range = byte..byte + glyph.len();
            byte = range.end;
            (glyph != " ").then(|| (glyph, rendered.project_search_range(record, range).unwrap()))
        })
        .collect()
}

fn visual_label(
    glyph_cells: &[(&str, crate::RenderedSearchMatch)],
    buffer: &ratatui::buffer::Buffer,
    coordinates: impl Fn(usize, usize) -> (u16, u16),
    id: &str,
) -> String {
    // Visual copy preserves continuation line boundaries and structural
    // origin padding. Verify each source grapheme against actual buffer cells
    // before composing that presentation-only expectation.
    let mut expected_copy = String::new();
    let first = &glyph_cells[0].1;
    let mut previous_row = first.row;
    let mut previous_end = first.start_column;
    for (glyph, span) in glyph_cells {
        assert_eq!(
            buffer[coordinates(span.row, span.start_column)].symbol(),
            *glyph,
            "source glyph at expected buffer cell {id}"
        );
        assert_eq!(
            span.end_column - span.start_column,
            glyph.width(),
            "source glyph width {id}"
        );
        if span.row == previous_row {
            expected_copy.extend(std::iter::repeat_n(' ', span.start_column - previous_end));
        } else {
            assert_eq!(
                span.row,
                previous_row + 1,
                "adjacent continuation rows {id}"
            );
            expected_copy.push('\n');
            expected_copy.extend(std::iter::repeat_n(' ', span.start_column));
        }
        expected_copy.push_str(glyph);
        previous_row = span.row;
        previous_end = span.end_column;
    }
    expected_copy
}

pub(super) fn copy_label(app: &mut App, first_cell: (u16, u16), last_cell: (u16, u16)) -> String {
    pointer(
        app,
        MouseEventKind::Down(MouseButton::Left),
        first_cell.0,
        first_cell.1,
    );
    pointer(
        app,
        MouseEventKind::Drag(MouseButton::Left),
        first_cell.0 + 1,
        first_cell.1,
    );
    pointer(
        app,
        MouseEventKind::Drag(MouseButton::Left),
        last_cell.0,
        last_cell.1,
    );
    pointer(
        app,
        MouseEventKind::Up(MouseButton::Left),
        last_cell.0,
        last_cell.1,
    );
    pointer(
        app,
        MouseEventKind::Down(MouseButton::Right),
        first_cell.0,
        first_cell.1,
    );
    let Some(CopyRequest::Selection { text }) = app.take_copy_request() else {
        panic!("selection did not emit a clipboard request");
    };
    text
}

#[test]
fn physical_contribution_search_reveal_uses_live_app_reflow() {
    let mut states = 0;
    for contribution in [
        Contribution::Anchor,
        Contribution::EmptyLink,
        Contribution::Zero,
        Contribution::Gap,
        Contribution::Hard,
        Contribution::Literal,
    ] {
        for owner in [Owner::Root, Owner::Section, Owner::Definition] {
            for from_end in [false, true] {
                let bundle = input(owner, Some(contribution), from_end);
                let control = input(owner, None, from_end);
                let mut app = App::new(&bundle);
                key(&mut app, KeyCode::Char('B'));
                let mut terminal = Terminal::new(TestBackend::new(82, 12)).unwrap();
                draw(&mut app, &mut terminal);
                assert_eq!(app.geometry.content.width, 78);
                if from_end {
                    key(&mut app, KeyCode::Char('G'));
                    draw(&mut app, &mut terminal);
                }
                let initial_scroll = app.session.content_scroll;
                let initial = &app.session.rendered_cache[&78].search(QUERY)[usize::from(from_end)];
                assert!(
                    !(initial_scroll..initial_scroll + 8).contains(&initial.row),
                    "initial occurrence is offscreen {contribution:?}/{owner:?}/{from_end}"
                );
                key(&mut app, KeyCode::Char(if from_end { '?' } else { '/' }));
                for ch in QUERY.chars() {
                    key(&mut app, KeyCode::Char(ch));
                }
                key(&mut app, KeyCode::Enter);
                draw(&mut app, &mut terminal);
                assert_eq!(app.search.active_match, usize::from(from_end));
                key(&mut app, KeyCode::Char('n'));
                draw(&mut app, &mut terminal);
                assert_eq!(app.search.active_match, usize::from(!from_end));
                key(&mut app, KeyCode::Char('N'));
                draw(&mut app, &mut terminal);
                let row = app.search.scope_matches[usize::from(from_end)].rendered.row;
                let record = app.session.rendered_cache[&78]
                    .viewport_anchor(row)
                    .unwrap()
                    .0;
                for width in WIDTHS {
                    let id = format!("{contribution:?}/{owner:?}/{from_end}/{width}");
                    terminal.backend_mut().resize(width + 4, 12);
                    draw(&mut app, &mut terminal);
                    assert_eq!(app.geometry.content.width, width, "{id}");
                    assert_eq!(app.geometry.content.height, 8, "{id}");
                    let row = app.search.scope_matches[usize::from(from_end)].rendered.row;
                    assert_eq!(
                        app.session.rendered_cache[&width]
                            .viewport_anchor(row)
                            .unwrap()
                            .0,
                        record,
                        "same source record {id}"
                    );
                    verify(&mut app, &mut terminal, from_end, &id);
                    let control = crate::DocumentView::new(&control).render(width);
                    let rendered = &app.session.rendered_cache[&width];
                    assert_eq!(
                        rendered.row_count,
                        control.row_count + contribution.rows(),
                        "physical contribution {id}"
                    );
                    if std::env::var_os("MANT_CONSUMER_MATRIX_REPORT").is_some() {
                        println!(
                            "MATRIX\tCG-E\t{contribution:?}/{owner:?}/{from_end}/{states}/{width}\tapp-reveal/reflow/buffer/link/copy\tpass"
                        );
                    }
                    states += 1;
                }
                key(&mut app, KeyCode::Char('B'));
                draw(&mut app, &mut terminal);
                assert!(app.geometry.content.width < 78);
                verify(&mut app, &mut terminal, from_end, "sidebar expanded");
                key(&mut app, KeyCode::Char('B'));
                draw(&mut app, &mut terminal);
                assert_eq!(app.geometry.content.width, 78);
                verify(&mut app, &mut terminal, from_end, "sidebar collapsed");
            }
        }
    }
    assert_eq!(states, 180);
}
