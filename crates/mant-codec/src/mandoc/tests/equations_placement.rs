//! Formula ownership follows executed origin before presentation begins.
use super::*;

#[test]
fn line_initial_inline_equations_share_their_surrounding_paragraph() {
    // Exact sources ran through pinned tree/HTML/UTF-8. roff_eqndelim
    // creates inline EQ nodes even when roff_EQ separately sets NODE_LINE.
    let body =
        ".EQ\ndelim $$\n.EN\nBefore $a$ and\n$b$ after.\n.EQ\nx over y\n.EN\nThen\n$c$ done.\n";
    for header in [
        ".TH MATH 1\n.SH DESCRIPTION\n",
        ".Dd November 9, 2026\n.Dt MATH 1\n.Os\n.Sh DESCRIPTION\n",
    ] {
        let source = format!("{header}{body}");
        let document =
            parse_manual_bytes(std::path::Path::new("placement.1"), source.as_bytes()).unwrap();
        let [
            Block::Paragraph {
                children: first, ..
            },
            Block::Equation { display: true, .. },
            Block::Paragraph { children: last, .. },
        ] = document.sections[0].blocks.as_slice()
        else {
            panic!(
                "complete paragraph and explicit display ownership: {:?}",
                document.sections[0].blocks
            );
        };
        // Compare the filled word stream while retaining protected blanks
        // in the IR. Paragraph and formula ownership are asserted separately.
        assert_eq!(
            mant_ir::inline_plain_text(first)
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["Before", "a", "and", "b", "after."]
        );
        assert_eq!(
            mant_ir::inline_plain_text(last)
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["Then", "c", "done."]
        );
        assert_eq!(
            first
                .iter()
                .filter(|n| matches!(n, Inline::Equation { .. }))
                .count(),
            2
        );
        let query = crate::ResolvedContent {
            label: "math".into(),
            address: None,
            document: Some(document),
            tldr: None,
        };
        let markdown = crate::encode::render_markdown(&query);
        let events: Vec<_> =
            pulldown_cmark::Parser::new_ext(&markdown, pulldown_cmark::Options::ENABLE_MATH)
                .filter_map(|event| match event {
                    pulldown_cmark::Event::InlineMath(_) => Some(false),
                    pulldown_cmark::Event::DisplayMath(_) => Some(true),
                    _ => None,
                })
                .collect();
        assert_eq!(events, [false, false, true, false]);
    }
}

#[test]
fn no_fill_inline_equations_keep_their_literal_owner() {
    // Pinned man_term.c's NODE_NOFILL/NODE_LINE gate still preserves hard
    // source rows. The exact input ran through tree/HTML/UTF-8 first.
    let source = b".TH MATH 1\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\n.nf\nBefore\n$a$ after\n.fi\n";
    let document = parse_manual_bytes(std::path::Path::new("literal.1"), source).unwrap();
    assert!(
        document.sections[0]
            .blocks
            .iter()
            .all(|b| !matches!(b, Block::Equation { .. }))
    );
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("literal owner");
    };
    assert_eq!(mant_ir::inline_plain_text(children), "Before\na after");
}
