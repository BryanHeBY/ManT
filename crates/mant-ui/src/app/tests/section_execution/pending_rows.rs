//! Verify resolved section seams in actual viewport cells and Markdown readbacks.

use super::super::contribution::{copy_label, draw, key};
use super::reader::{assert_painted_glyphs, source_cells, viewport_cell};
use super::*;
use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};

const HEADER: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
const FOOTER: &str = ".SH ENDTEST\nFINISH\n";
const ACCEPTED: &str = ".nf\nALPHA\\p\n.SS NEXT\nAFTERMARK\n";
const REJECTED: &str = ".nf\n\\p BETA\n.SS NEXT\nAFTERMARK\n";
const RETAINED_BR: &str = "\\p BETA\n.br\n\\&\n.SS NEXT\nAFTERMARK\n";
const LINK: &str = ".nf\n.UR https://example.com/\nALPHA\\p\n.UE\n.SS NEXT\nAFTERMARK\n";
const LINK_REJECTED: &str =
    ".nf\n.UR https://example.com/\nALPHA\n.UE \\p \\p BETA\n.SS NEXT\nAFTERMARK\n";

fn read(body: &str) -> ResolvedContent {
    let source = format!("{HEADER}{body}{FOOTER}");
    mant_loader::load_roff_bytes(source.as_bytes()).unwrap()
}

#[test]
fn pending_section_rows_paint_once_and_do_not_steal_search_ownership() {
    // Fixed CVS man_term.c::pre_SS / post_SH and term.c::term_flushln:
    // a pending old-owner marker can close one printed row or reject
    // the heading before any graph; a retained .br closes the old row.
    for (body, rows, rejected_title) in [
        (ACCEPTED, &["ALPHA", "NEXT", BODY][..], false),
        (REJECTED, &["", BODY][..], true),
        (RETAINED_BR, &["", "NEXT", BODY][..], false),
    ] {
        let mut app = App::new(&read(body));
        key(&mut app, KeyCode::Char('B'));
        let mut terminal = Terminal::new(TestBackend::new(124, 24)).unwrap();
        for width in [12, 78] {
            terminal.backend_mut().resize(width + 4, 24);
            draw(&mut app, &mut terminal);
            assert_eq!(app.geometry.content.width, width);
            let rendered = &app.session.rendered_cache[&width];
            let buffer = terminal.backend().buffer();
            let mut row = rendered.search("FIRST")[0].row + 1;
            for word in rows {
                if word.is_empty() {
                    let (left, y) = viewport_cell(&app, row, 0);
                    assert!(
                        (left..app.geometry.content.right())
                            .all(|x| buffer[(x, y)].symbol() == " "),
                        "{body} width {width}: expected blank row {row}",
                    );
                    row += 1;
                } else {
                    let [hit] = rendered.search(word).try_into().unwrap();
                    assert_eq!(hit.row, row, "{body} width {width} {word:?}");
                    assert_painted_glyphs(&app, buffer, &source_cells(rendered, word));
                    row += hit.fragment_count();
                }
            }
            assert_eq!(rendered.search("ENDTEST")[0].row, row, "{body}");
            assert!(rendered.search("BETA").is_empty(), "{body}");
            assert_eq!(rendered.search("NEXT").is_empty(), rejected_title, "{body}");
        }
        key(&mut app, KeyCode::Char('/'));
        for letter in BODY.chars() {
            key(&mut app, KeyCode::Char(letter));
        }
        key(&mut app, KeyCode::Enter);
        draw(&mut app, &mut terminal);
        assert_eq!(app.search.scope_matches.len(), 1, "{body}");
        assert_eq!(
            app.session.document.navigation()[app.selected].id,
            "next",
            "{body}"
        );
    }
}

#[test]
fn literal_link_before_the_section_keeps_its_hit_range_and_copy_text() {
    // CVS renders <URL> and NEXT on a shared device row in the first
    // case, and rejects BETA and NEXT in the second. In both, the reader
    // retains ALPHA (and only ALPHA) as the old link.
    assert_old_link(LINK, false);
    assert_old_link(LINK_REJECTED, true);
}

fn assert_old_link(source: &str, rejects_heading: bool) {
    let mut app = App::new(&read(source));
    key(&mut app, KeyCode::Char('B'));
    let mut terminal = Terminal::new(TestBackend::new(82, 24)).unwrap();
    draw(&mut app, &mut terminal);
    let rendered = &app.session.rendered_cache[&78];
    let cells = source_cells(rendered, "ALPHA");
    assert_painted_glyphs(&app, terminal.backend().buffer(), &cells);
    let &(_, row, start) = cells.first().unwrap();
    let &(_, last_row, last) = cells.last().unwrap();
    assert_eq!(row, last_row);
    let url = &rendered.search("<https://example.com/>")[0];
    assert_eq!(url.row, row + usize::from(!rejects_heading));
    let outside_link = if rejects_heading {
        assert_eq!(rendered.search("NEXT"), []);
        assert_eq!(rendered.search("BETA"), []);
        assert_eq!(rendered.search(BODY)[0].row, row + 2);
        (url.row, url.start_column)
    } else {
        let next = source_cells(rendered, "NEXT")[0];
        assert_eq!(next.1, row + 2);
        (next.1, next.2)
    };
    for column in [start, last] {
        click_document_cell(&mut app, column, row);
        assert_eq!(
            app.take_external_request()
                .as_ref()
                .map(crate::ExternalUri::as_str),
            Some("https://example.com/"),
        );
    }
    click_document_cell(&mut app, outside_link.1, outside_link.0);
    assert!(app.take_external_request().is_none());
    let start = viewport_cell(&app, row, start);
    let end = viewport_cell(&app, row, last);
    assert_eq!(copy_label(&mut app, start, end), "ALPHA");
}

#[test]
fn pending_section_rows_do_not_resurrect_rejected_text_after_markdown_readback() {
    for (body, title) in [(ACCEPTED, "NEXT"), (REJECTED, ""), (RETAINED_BR, "NEXT")] {
        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
            let mut input = read(body);
            let mut generated_markers = std::collections::BTreeSet::new();
            for round in 0..2 {
                let artifact = render_addressable_markdown_with_options(&input, options);
                generated_markers.extend(
                    artifact
                        .anchor_ranges()
                        .iter()
                        .map(|range| artifact.text()[range.clone()].to_owned()),
                );
                if round == 0 && options.preserve_anchors {
                    assert_eq!(
                        artifact
                            .anchor_ranges()
                            .iter()
                            .filter(|range| {
                                &artifact.text()[(*range).clone()] == r#"<a id="next"></a>"#
                            })
                            .count(),
                        1,
                    );
                }
                input = mant_loader::load_markdown_text(artifact.text(), None).unwrap();
                let document = input.document.as_ref().unwrap();
                assert_eq!(mant_ir::validate_document(document), []);
                let first = document
                    .sections
                    .iter()
                    .find(|section| section.id.as_str() == "first")
                    .unwrap();
                let [next] = first.children.as_slice() else {
                    panic!("{body}")
                };
                assert_eq!(
                    next.heading.plain_text().trim_start_matches(' '),
                    title,
                    "{body}"
                );
                let authored = |blocks: &[AstBlock]| {
                    blocks
                        .iter()
                        .filter_map(|block| match block {
                            AstBlock::Paragraph { children, .. }
                            | AstBlock::Preformatted { children, .. } => {
                                let text = inline_plain_text(children);
                                // The Markdown fallback may retain a proven generated
                                // anchor as a literal, including an escaped carry-over
                                // in the second export. It is not authored roff text.
                                (!text.trim().is_empty() && !generated_markers.contains(&text))
                                    .then_some(text)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                };
                let old = if body == ACCEPTED {
                    &["ALPHA"][..]
                } else {
                    &[][..]
                };
                assert_eq!(authored(&first.blocks), old, "{body}");
                assert_eq!(authored(&next.blocks), [BODY], "{body}");
            }
        }
    }
}
