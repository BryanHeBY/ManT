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
