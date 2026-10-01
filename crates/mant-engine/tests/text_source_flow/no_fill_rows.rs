//! Native invisible cells still own literal source rows after field retirement.

use mant_ir::{Block, ResolvedContent, inline_plain_text};

fn source(line: &str) -> String {
    format!(".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.nf\nALPHA\n{line}\nBETA\n.fi\nNEXT\n")
}

fn description(text: &str) -> &str {
    text.split_once("DESCRIPTION\n").unwrap().1
}

fn native_rows(text: &str) -> Vec<&str> {
    // term_field defers trailing ordinary blanks; retain every delimiter and
    // all non-breaking spaces. This is not a trim/filter of physical rows.
    text.split('\n')
        .map(|row| row.trim_end_matches(' '))
        .collect()
}

fn verify_portable_literal(query: &ResolvedContent, literal: &str, source: &str) {
    let markdown = mant_codec::encode::render_markdown(query);
    assert!(
        markdown.contains(&format!("```\n{literal}\n```")),
        "{source}\n{markdown}"
    );
    let portable = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let json = mant_render::render_query_json(&portable, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let portable: ResolvedContent = restored.into();
    let blocks = &portable.document.as_ref().unwrap().sections[0].blocks;
    let [
        Block::Preformatted { children, .. },
        Block::Paragraph {
            children: next,
            layout,
            ..
        },
    ] = blocks.as_slice()
    else {
        panic!("literal then one following paragraph expected: {source}\n{json}");
    };
    // encode::blocks::mapped_blocks joins constructs with a blank source
    // line. markdown::layout preserves that fence-to-paragraph gap and
    // removes only the fence framing newline, not literal row delimiters.
    assert_eq!(inline_plain_text(children), literal, "{source}");
    assert_eq!(inline_plain_text(next), "NEXT", "{source}");
    assert_eq!(layout.spacing_before_lines, 1, "{source}");
    assert_eq!(
        description(&mant_render::render_query_man(&portable)),
        format!("{literal}\n\nNEXT"),
        "{source}"
    );
}

#[test]
fn invisible_literal_cells_keep_rows_through_wire_text_and_portable_consumers() {
    // Every exact complete source ran pristine CVS ASCII/UTF-8/HTML/tree/lint
    // first (all 35 profiles successful, no lint). term.c::term_fill treats
    // ASCII_NBRZW as graph even at width zero; NODE_NOFILL | NODE_LINE in
    // man_term.c::print_man_node closes that real buffered row before BETA.
    // Raw empty TEXT instead executes term_vspace. A bare \z has no cell:
    // the next B is overwritten by E, leaving ETA without an invented row.
    // term_field delays ordinary blanks until a later printed graph. These
    // trailing blanks have no printed prefix but still own a real row; do not
    // restore their discarded tail text to manufacture its occupancy.
    for (line, literal) in [
        ("\\&        ", "ALPHA\n\nBETA"),
        ("\\&", "ALPHA\n\nBETA"),
        ("        ", "ALPHA\n\nBETA"),
        (
            "\\~\\~\\~\\~\\~\\~\\~\\~",
            "ALPHA\n\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\nBETA",
        ),
        ("\\z", "ALPHA\nETA"),
        ("\\z        ", "ALPHA\n\nBETA"),
        ("", "ALPHA\n\nBETA"),
    ] {
        let source = source(line);
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(!json.contains("\\u0000mant:"), "{source}\n{json}");
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let document = query.document.as_ref().unwrap();
        let literals = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Preformatted { children, .. } => Some(inline_plain_text(children)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(literals.len(), 1, "{source}\n{json}");
        if !matches!(line, "        " | "\\z        ") {
            // NBRZW's accepted prefix ends before its trailing blanks. The
            // new row witness must not revive that unprinted scalar suffix.
            assert_eq!(literals[0], literal, "{source}\n{json}");
        }
        assert_eq!(
            native_rows(&literals[0]),
            native_rows(literal),
            "{source}\n{json}"
        );

        let expected = format!("{literal}\nNEXT");
        let plain = mant_render::render_query_text(&query);
        assert_eq!(
            native_rows(description(&plain)),
            native_rows(&expected),
            "{source}\n{plain:?}"
        );
        assert_eq!(mant_render::render_query_man(&query), plain, "{source}");
        let styled = mant_render::render_query_text_with(&query, |_, value| {
            format!("\x1b[1m{value}\x1b[0m")
        });
        assert_eq!(
            styled.replace("\x1b[1m", "").replace("\x1b[0m", ""),
            plain,
            "{source}"
        );

        verify_portable_literal(&query, &literals[0], &source);
    }
}
