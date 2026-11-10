//! Read the old owner's interior and trailing rows as separate facts.

use super::super::contribution::{copy_label, draw, key};
use super::reader::{assert_painted_glyphs, source_cells, viewport_cell};
use super::*;
use mant_codec::encode::{
    MarkdownArtifact, MarkdownOptions, render_addressable_markdown_with_options,
};

const HEADER: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
const FOOTER: &str = ".SH ENDTEST\nFINISH\n";
const PREFIX: &str = r"ALPHA\p \p\&";
const LINKED: &str =
    ".nf\n.UR https://audit.test/\nALPHA\\p \\p\\& BETA\n.UE\n.SS NEXT\nAFTERMARK\n";

fn read(body: &str) -> ResolvedContent {
    let source = format!("{HEADER}{body}{FOOTER}");
    mant_loader::load_roff_bytes(source.as_bytes()).unwrap()
}

#[test]
fn earlier_blank_passes_paint_once_and_search_stays_with_the_source_section() {
    // Pinned CVS term_fill/term_flushln and man_term.c::post_SH:
    // a blank may precede BETA, trail ALPHA, or survive rejection.
    // Width affects wrapping, not the number or owner of these rows.
    for mode in ["", ".nf\n"] {
        for (suffix, head, target, rows) in [
            (
                " BETA",
                "NEXT",
                "next",
                &["ALPHA", "", "BETA", "NEXT", BODY][..],
            ),
            (
                r" \p\& BETA",
                "NEXT",
                "next",
                &["ALPHA", "", "", "BETA", "NEXT", BODY],
            ),
            ("", "NEXT", "next", &["ALPHA", "", "NEXT", BODY]),
            (r" \p BETA", "NEXT", "next", &["ALPHA", "", "", BODY]),
            (r" \zX", "NEXT", "next", &["ALPHA", "", "X", "NEXT", BODY]),
            (" BETA", r"\&", "section", &["ALPHA", "", "BETA", BODY]),
            (
                " BETA",
                r"NEXT\p WORD",
                "next-p-word",
                &["ALPHA", "", "BETA", "NEXT", "WORD", BODY],
            ),
        ] {
            let body = format!("{mode}{PREFIX}{suffix}\n.SS {head}\n{BODY}\n");
            let mut app = App::new(&read(&body));
            key(&mut app, KeyCode::Char('B'));
            let mut terminal = Terminal::new(TestBackend::new(82, 24)).unwrap();
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
                            "{body} width {width}: expected blank row {row}"
                        );
                        row += 1;
                    } else {
                        // X alone is also a substring of NEXT. Use its actual
                        // row here; that row is proven separately by the
                        // owner's rendered cells, rather than matching first.
                        let mut hits = rendered
                            .search(word)
                            .into_iter()
                            .filter(|hit| *word != "X" || hit.row == row);
                        let hit = hits.next().expect("one source row for the word");
                        assert_eq!(hit.row, row, "{body} width {width} {word:?}");
                        assert_eq!(hits.count(), 0, "{body} {word}");
                        if *word == "X" {
                            let (x, y) = viewport_cell(&app, row, hit.start_column);
                            assert_eq!(buffer[(x, y)].symbol(), "X", "{body} width {width}");
                        } else {
                            assert_painted_glyphs(&app, buffer, &source_cells(rendered, word));
                        }
                        row += hit.fragment_count();
                    }
                }
                assert_eq!(
                    rendered.search("ENDTEST")[0].row,
                    row,
                    "{body} width {width}"
                );
                for word in ["BETA", "NEXT"] {
                    if !rows.contains(&word) {
                        assert_eq!(rendered.search(word), [], "{body} width {width}");
                    }
                }
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
                target,
                "{body}"
            );
        }
    }
}

#[test]
fn a_link_releases_its_last_row_without_extending_its_hit_range() {
    // Pinned man_term.c::post_UR leaves BETA and the generated URL in the
    // old section's last row. Only BETA belongs to the link label.
    let mut app = App::new(&read(LINKED));
    key(&mut app, KeyCode::Char('B'));
    let mut terminal = Terminal::new(TestBackend::new(82, 24)).unwrap();
    draw(&mut app, &mut terminal);
    let rendered = &app.session.rendered_cache[&78];
    let cells = source_cells(rendered, "BETA");
    assert_painted_glyphs(&app, terminal.backend().buffer(), &cells);
    let &(_, row, first) = cells.first().unwrap();
    let &(_, last_row, last) = cells.last().unwrap();
    assert_eq!(last_row, row);
    assert_eq!(rendered.search("ALPHA")[0].row + 2, row);
    let next = source_cells(rendered, "NEXT")[0];
    assert_eq!(next.1, row + 1);
    assert_eq!(rendered.search(BODY)[0].row, row + 2);
    let (outside, url) = {
        let url = &rendered.search("<https://audit.test/>")[0];
        assert_eq!(url.row, row);
        (url.start_column, "https://audit.test/")
    };
    for column in [first, last] {
        click_document_cell(&mut app, column, row);
        assert_eq!(
            app.take_external_request()
                .as_ref()
                .map(crate::ExternalUri::as_str),
            Some(url)
        );
    }
    click_document_cell(&mut app, outside, row);
    assert!(app.take_external_request().is_none());
    click_document_cell(&mut app, next.2, next.1);
    assert!(app.take_external_request().is_none());
    let first = viewport_cell(&app, row, first);
    let last = viewport_cell(&app, row, last);
    assert_eq!(copy_label(&mut app, first, last), "BETA");
}

fn span(block: &AstBlock) -> std::ops::Range<usize> {
    let range = geometry::block_source(block).unwrap().byte_range.unwrap();
    range.start.get() as usize..range.end.get() as usize
}

fn authored_blocks<'a>(
    mut blocks: &'a [AstBlock],
    artifact: &MarkdownArtifact<'_>,
    carried: &mut Option<String>,
) -> &'a [AstBlock] {
    // For this small fixture, addressable output may attach a standalone
    // destination to FIRST. Exclude only that proven trailing artifact span
    // and an adjacent escaped copy of the already-proven previous marker.
    let Some((last, preceding)) = blocks.split_last() else {
        return blocks;
    };
    let tail = span(last);
    let text = artifact.text();
    let Some(marker) = artifact.anchor_ranges().iter().find(|range| {
        range.start == tail.start
            && range.end <= tail.end
            && text[range.end..tail.end].bytes().all(|c| c == b'\n')
    }) else {
        assert!(carried.is_none());
        return blocks;
    };
    blocks = preceding;
    if let Some(old) = carried.replace(text[marker.clone()].to_owned()) {
        let (escaped, rest) = blocks.split_last().unwrap();
        let escaped = span(escaped);
        let quoted = old.replace('<', r"\<").replace('>', r"\>");
        assert_eq!(text[escaped.clone()].trim_end_matches('\n'), quoted);
        assert_eq!(&text[escaped.end..tail.start], "\n");
        blocks = rest;
    }
    blocks
}

fn block_text(block: &AstBlock) -> String {
    match block {
        AstBlock::Paragraph { children, .. } | AstBlock::Preformatted { children, .. } => {
            inline_plain_text(children)
        }
        _ => panic!("unexpected readback block: {block:?}"),
    }
}

#[test]
fn both_markdown_rounds_keep_original_interior_and_trailing_hard_lines() {
    // Heading separators in the two Markdown routes are format framing.
    // The old owner's authored hard rows must still agree with the source
    // independently in each readback, including a true trailing empty pass.
    for mode in ["", ".nf\n"] {
        for (suffix, expected) in [
            (" BETA", "ALPHA\n\nBETA"),
            (r" \zX", "ALPHA\n\nX"),
            ("", "ALPHA"),
        ] {
            let body = format!("{mode}{PREFIX}{suffix}\n.SS NEXT\n{BODY}\n");
            for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
                let mut current = read(&body);
                let mut carried = None;
                let mut body_carried = None;
                for cycle in 1..=2 {
                    let artifact = render_addressable_markdown_with_options(&current, options);
                    let next = mant_loader::load_markdown_text(artifact.text(), None).unwrap();
                    let document = next.document.as_ref().unwrap();
                    assert_eq!(mant_ir::validate_document(document), []);
                    let first = document
                        .sections
                        .iter()
                        .find(|s| s.id.as_str() == "first")
                        .unwrap();
                    let [target] = first.children.as_slice() else {
                        panic!("{body}, cycle {cycle}");
                    };
                    let actual: Vec<_> = authored_blocks(&first.blocks, &artifact, &mut carried)
                        .iter()
                        .map(block_text)
                        .collect();
                    if options.preserve_anchors {
                        assert_eq!(carried.as_deref(), Some(r#"<a id="next"></a>"#));
                    }
                    let expected = if suffix.is_empty() && mode.is_empty() {
                        vec!["ALPHA\n\n"]
                    } else if suffix.is_empty() {
                        vec!["ALPHA", "\n"]
                    } else {
                        vec![expected]
                    };
                    assert_eq!(actual, expected, "{body}, cycle {cycle}, {options:?}");
                    assert_eq!(target.heading.plain_text().trim_start_matches(' '), "NEXT");
                    // The following sibling's generated marker can be
                    // attached to target, independently of its source body.
                    let next_body: Vec<_> =
                        authored_blocks(&target.blocks, &artifact, &mut body_carried)
                            .iter()
                            .map(block_text)
                            .collect();
                    assert_eq!(next_body, [BODY]);
                    current = next;
                }
            }
        }
    }
}
