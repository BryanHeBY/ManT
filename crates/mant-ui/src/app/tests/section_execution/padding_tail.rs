//! Preserve the completed rows inside a literal while its reading tail survives.

use super::super::contribution::{copy_label, draw, key};
use super::reader::{assert_painted_glyphs, source_cells, viewport_cell};
use super::tail_receipts::{authored_blocks, block_text};
use super::*;
use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};

const HEADER: &str =
    ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n.nf\n";
const FOOTER: &str = "\n.SS NEXT\nAFTERMARK\n.SH ENDTEST\nFINISH\n";
const LINK: &str = "https://audit.test/";
const LINKED: &str = ".UR https://audit.test/\nLEAD\n.UE\nALPHA\\p \\p\\& \\h'1n'";

fn read(old: &str) -> ResolvedContent {
    let source = format!("{HEADER}{old}{FOOTER}");
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = serde_json::to_string(&QueryBundle::from(&original)).unwrap();
    let input = ResolvedContent::from(serde_json::from_str::<QueryBundle>(&wire).unwrap());
    assert_eq!(input, original);
    assert_eq!(
        mant_ir::validate_document(input.document.as_ref().unwrap()),
        []
    );
    input
}

#[test]
fn a_literal_reading_space_does_not_paint_a_second_completed_blank() {
    // The exact sources ran fixed CVS ASCII/UTF-8/HTML/tree/lint first.
    // term_fill admits one/two NBRZW passes. The reading-only tail from
    // term.c::ESCAPE_HORIZ is a distinct literal row; the old completion
    // must not appear as a further blank. NBSP is an authored control.
    for (old, completed) in [
        ("ALPHA\\p \\p\\& \u{a0}", 1),
        (r"ALPHA\p \p\& \h'1n'", 1),
        (r"ALPHA\p \p\& \p\& \h'2n'\c", 2),
        (LINKED, 1),
    ] {
        let mut app = App::new(&read(old));
        key(&mut app, KeyCode::Char('B'));
        let mut terminal = Terminal::new(TestBackend::new(124, 24)).unwrap();
        for width in WIDTHS.into_iter().chain([12]) {
            terminal.backend_mut().resize(width + 4, 24);
            draw(&mut app, &mut terminal);
            assert_eq!(app.geometry.content.width, width);
            let rendered = &app.session.rendered_cache[&width];
            let buffer = terminal.backend().buffer();
            let unique_row = |word| {
                let hits = rendered.search(word);
                assert_eq!(hits.len(), 1, "{old} width {width}: {word}");
                assert_painted_glyphs(&app, buffer, &source_cells(rendered, word));
                hits[0].row
            };
            let alpha = unique_row("ALPHA");
            let next = unique_row("NEXT");
            assert_eq!(next, alpha + completed + 2, "{old} width {width}");
            assert_eq!(unique_row(BODY), next + 1, "{old} width {width}");
            for row in alpha + 1..next {
                let (left, y) = viewport_cell(&app, row, 0);
                let nonpadding: Vec<_> = (left..app.geometry.content.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .filter(|symbol| *symbol != " ")
                    .collect();
                let expected = if row + 1 == next && old.ends_with('\u{a0}') {
                    &["\u{a0}"][..]
                } else {
                    &[]
                };
                assert_eq!(nonpadding, expected, "{old} width {width}: row {row}");
            }
        }
        key(&mut app, KeyCode::Char('/'));
        for letter in BODY.chars() {
            key(&mut app, KeyCode::Char(letter));
        }
        key(&mut app, KeyCode::Enter);
        draw(&mut app, &mut terminal);
        assert_eq!(app.search.scope_matches.len(), 1);
        assert_eq!(app.session.document.navigation()[app.selected].id, "next");
    }
}

#[test]
fn an_old_link_does_not_acquire_the_reading_tail_or_new_heading() {
    // The exact old UR example ran pristine first. post_UR prints
    // its own target; neither the later source rows nor HEAD are labels.
    let mut app = App::new(&read(LINKED));
    key(&mut app, KeyCode::Char('B'));
    let mut terminal = Terminal::new(TestBackend::new(82, 24)).unwrap();
    draw(&mut app, &mut terminal);
    let rendered = &app.session.rendered_cache[&78];
    let label = source_cells(rendered, "LEAD");
    assert_painted_glyphs(&app, terminal.backend().buffer(), &label);
    let &(_, row, first) = label.first().unwrap();
    let &(_, _, last) = label.last().unwrap();
    let alpha = source_cells(rendered, "ALPHA")[0];
    let next = source_cells(rendered, "NEXT")[0];
    let target_column = rendered.search("<https://audit.test/>")[0].start_column;
    for column in [first, last] {
        click_document_cell(&mut app, column, row);
        assert_eq!(
            app.take_external_request()
                .as_ref()
                .map(crate::ExternalUri::as_str),
            Some(LINK)
        );
    }
    for (column, row) in [
        (target_column, row),
        (alpha.2, alpha.1),
        (0, next.1 - 1),
        (next.2, next.1),
    ] {
        click_document_cell(&mut app, column, row);
        assert!(app.take_external_request().is_none());
    }
    let first = viewport_cell(&app, row, first);
    let last = viewport_cell(&app, row, last);
    assert_eq!(copy_label(&mut app, first, last), "LEAD");
}

#[test]
fn markdown_keeps_the_literal_bytes_and_never_exports_a_second_gap() {
    // Pinned term.c classifies HORIZ as an advance, not a printable
    // glyph. Its supported reading space belongs inside the fence. A
    // fence separator is formatting; a separate <br /> is another row.
    for (old, literal, heading) in [
        (r"ALPHA\p \p\& \h'1n'", "ALPHA\n\n ", " NEXT"),
        (r"ALPHA\p \p\& \p\& \h'2n'\c", "ALPHA\n\n\n ", "NEXT"),
        ("ALPHA\\p \\p\\& \u{a0}", "ALPHA\n\n\u{a0}", " NEXT"),
    ] {
        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
            let mut input = read(old);
            let mut carried = None;
            let mut body_carried = None;
            for cycle in 1..=2 {
                let artifact = render_addressable_markdown_with_options(&input, options);
                let text = artifact.text();
                assert!(
                    text.contains(&format!("```\n{literal}\n```")),
                    "{old} round {cycle}: {text:?}"
                );
                assert!(!text.contains("<br />"), "{old} round {cycle}: {text:?}");
                let readback = mant_loader::load_markdown_text(text, None).unwrap();
                let doc = readback.document.as_ref().unwrap();
                assert_eq!(mant_ir::validate_document(doc), []);
                let first = doc
                    .sections
                    .iter()
                    .find(|s| s.id.as_str() == "first")
                    .unwrap();
                let [next] = first.children.as_slice() else {
                    panic!("{old} round {cycle}: missing subsection");
                };
                let [old_block @ AstBlock::Preformatted { .. }] =
                    authored_blocks(&first.blocks, &artifact, &mut carried)
                else {
                    panic!(
                        "{old} round {cycle}: extra authored old block: {:?}",
                        first.blocks
                    );
                };
                assert_eq!(block_text(old_block), literal);
                assert_eq!(next.heading.plain_text(), heading);
                assert_eq!(next.id.as_str(), "next");
                let [body @ AstBlock::Paragraph { .. }] =
                    authored_blocks(&next.blocks, &artifact, &mut body_carried)
                else {
                    panic!("{old} round {cycle}: unexpected BODY rows");
                };
                assert_eq!(block_text(body), BODY);
                if options.preserve_anchors {
                    assert_eq!(carried.as_deref(), Some(r#"<a id="next"></a>"#));
                }
                input = readback;
            }
        }
    }
}
