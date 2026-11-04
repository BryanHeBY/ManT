use super::*;
use crate::encode::inline::context::work::{PARSED_BYTES, WHOLE_PARSES};

#[test]
fn final_row_validation_has_separate_flat_long_piece_and_link_budgets() {
    let framing = Framing::Rows {
        manual_links: false,
        interruptible: true,
    };
    // The only complete parse belongs to the final byte sequence. Each
    // independent expected child is visited once; no prefix is reparsed.
    for count in [64, 256, 1024] {
        let mut pieces = (0..count)
            .map(|_| InlinePiece::plain("word ".into()))
            .collect::<Vec<_>>();
        let bytes: usize = pieces.iter().map(|piece| piece.rendered.len()).sum();
        PARSED_BYTES.with(|work| work.set(0));
        WHOLE_PARSES.with(|work| work.set(0));
        assert!(render_inline_pieces(&mut pieces, framing).is_some());
        assert!(PARSED_BYTES.with(std::cell::Cell::get) <= 2 * bytes + 4 * count + 8);
        assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
    }
    for bytes in [64, 1024, 16_384] {
        let mut pieces = [InlinePiece::plain("x".repeat(bytes))];
        PARSED_BYTES.with(|work| work.set(0));
        WHOLE_PARSES.with(|work| work.set(0));
        assert!(render_inline_pieces(&mut pieces, framing).is_some());
        assert!(PARSED_BYTES.with(std::cell::Cell::get) <= 2 * bytes + 4);
        assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
    }
    for count in [16, 64, 256] {
        let mut pieces = (0..count)
            .map(|_| {
                InlinePiece::plain(format!("[a](https://a.test/{} \"owner\")", "x".repeat(128)))
            })
            .collect::<Vec<_>>();
        let bytes: usize = pieces.iter().map(|piece| piece.rendered.len()).sum();
        PARSED_BYTES.with(|work| work.set(0));
        WHOLE_PARSES.with(|work| work.set(0));
        assert!(render_inline_pieces(&mut pieces, framing).is_some());
        assert!(PARSED_BYTES.with(std::cell::Cell::get) <= 2 * bytes + 4 * count);
        assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
    }
}

#[test]
fn final_recovery_keeps_accepted_children_with_at_most_two_whole_parses() {
    let mut pieces = [
        InlinePiece::styled("a _**b**_".into(), "*", "_"),
        InlinePiece::plain(" ".into()),
    ];
    PARSED_BYTES.with(|work| work.set(0));
    WHOLE_PARSES.with(|work| work.set(0));
    let markdown = render_inline_pieces(
        &mut pieces,
        Framing::Rows {
            manual_links: false,
            interruptible: true,
        },
    )
    .unwrap();
    assert_eq!(markdown, "a _**b**_&#32;");
    assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 2);
    assert!(PARSED_BYTES.with(std::cell::Cell::get) < 2048);
    // Canonical paragraph and interruptible roots have different leading
    // spellings; both obey the exact same independently checked hard rows.
    for interruptible in [false, true] {
        let mut pieces = [InlinePiece::plain("\n \t\n".into())];
        WHOLE_PARSES.with(|work| work.set(0));
        let markdown = render_inline_pieces(
            &mut pieces,
            Framing::Rows {
                manual_links: false,
                interruptible,
            },
        )
        .unwrap();
        assert!(markdown.starts_with(if interruptible { "&#10;" } else { "<br />" }));
        assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
    }
}

#[test]
fn retained_link_witnesses_have_linear_local_and_final_parse_work() {
    use mant_ir::{Inline, LinkTarget};

    fn measure(count: usize, label: &[Inline], title: &str) -> (usize, usize) {
        let nodes = (0..count)
            .map(|index| Inline::Link {
                target: LinkTarget::External {
                    uri: format!("https://a.test/{index}/&amp;"),
                },
                title: Some(title.into()),
                children: label.to_vec(),
            })
            .collect::<Vec<_>>();
        PARSED_BYTES.with(|work| work.set(0));
        WHOLE_PARSES.with(|work| work.set(0));
        let markdown = super::super::render_inline_nodes(
            nodes.iter(),
            crate::encode::MarkdownOptions::default(),
            false,
            true,
        );
        let work = (
            PARSED_BYTES.with(std::cell::Cell::get),
            WHOLE_PARSES.with(std::cell::Cell::get),
        );
        // Retained typed Links, not InlinePiece::plain strings: every local
        // bracket witness, metadata escape and final Rows parse is exercised.
        let actual = pulldown_cmark::Parser::new(&markdown)
            .filter_map(|event| match event {
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
                    dest_url, title, ..
                }) => Some((dest_url.into_string(), title.into_string())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), count);
        for (index, (target, restored_title)) in actual.iter().enumerate() {
            assert_eq!(target, &format!("https://a.test/{index}/&amp;"));
            assert_eq!(restored_title, title);
        }
        work
    }

    let styled = [Inline::Strong {
        children: vec![Inline::Text {
            value: "label".into(),
        }],
    }];
    let recovered = crate::parse_markdown("_! ***a***_&#32;", None)
        .unwrap()
        .document
        .blocks
        .remove(0);
    let mant_ir::Block::Paragraph {
        children: recovered,
        ..
    } = recovered
    else {
        panic!("one independently parsed recovery label")
    };
    for label in [styled.as_slice(), recovered.as_slice()] {
        let mut previous = None;
        for count in [16, 64, 256] {
            let work = measure(count, label, "metadata &amp; \\\nnext");
            if let Some((bytes, parses)) = previous {
                // Input grows fourfold. Deterministic work counts allow a
                // generous fivefold slope (plus fixed overhead), catching an
                // accumulated-prefix scan without timing or exact call counts.
                assert!(work.0 <= bytes * 5 + 1024, "{count} Links: {work:?}");
                assert!(work.1 <= parses * 5 + 4, "{count} Links: {work:?}");
            }
            assert!(work.1 <= 8 * count + 4, "bounded local/final passes");
            previous = Some(work);
        }
    }
    for size in [64, 1024, 16_384] {
        let label = [Inline::Text {
            value: "x".repeat(size),
        }];
        let title = format!("{}\\&amp;\nend", "t".repeat(size));
        let work = measure(1, &label, &title);
        assert!(work.0 <= 64 * (size * 2 + 128), "long Link/title: {work:?}");
        assert!(work.1 <= 8, "long metadata does not add parsing passes");
    }
}
