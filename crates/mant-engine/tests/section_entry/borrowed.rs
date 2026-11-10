use mant_ir::Block;

#[test]
fn a_heading_flushes_the_old_input_without_stealing_its_glyphs() {
    // The exact full inputs ran against pinned CVS ASCII/UTF-8/HTML/tree/lint.
    // man_term.c::pre_SS leaves the old row live at PD=0. term.c::encode1
    // writes a \z glyph before the next word releases or overwrites it.
    // Reading gives each section its own row even when the device shares
    // ALPHAX/NEXT or X/NEXT; the old word's glyph stays in its source owner.
    for mode in ["", ".nf\n"] {
        for (old, head, old_rows) in [
            (r"ALPHA", r"\&\p NEXT", &["ALPHA"][..]),
            (r"ALPHA\c", r"\p NEXT", &["ALPHA"]),
            (r"ALPHA\p\zX", "NEXT", &["ALPHAX"]),
            (r"ALPHA\p \zX", "NEXT", &["ALPHA", "X"]),
            (r"\zX\p", r"\p NEXT", &["X"]),
            (r"\zX\p", r"\&\p NEXT", &["X"]),
        ] {
            let source = format!(
                ".TH CHECK 1 \"November 10, 2026\"\n.SH NAME\ncheck \\- audit\n\
                 .SH FIRST\n.PD 0\n{mode}{old}\n.SS {head}\nAFTERMARK\n.SH ENDTEST\nFINISH\n"
            );
            let bundle = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let document = bundle.document.as_ref().unwrap();
            let old = &document.sections[1];
            let next = &old.children[0];
            assert_eq!(next.heading.plain_text(), "NEXT", "{source}");
            assert!(matches!(
                next.blocks.as_slice(),
                [Block::Paragraph { children, .. }]
                    if mant_ir::inline_plain_text(children) == "AFTERMARK"
            ));
            let old_text: String = old
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                        Some(mant_ir::inline_plain_text(children))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(old_text, old_rows.join("\n"), "{source}");
            let reading = mant_render::render_query_man(&bundle);
            let rows: Vec<_> = reading
                .lines()
                .skip_while(|line| *line != "FIRST")
                .skip(1)
                .take_while(|line| *line != "ENDTEST")
                .map(str::trim_start)
                .collect();
            let expected: Vec<_> = old_rows
                .iter()
                .copied()
                .chain(["NEXT", "AFTERMARK"])
                .collect();
            assert_eq!(rows, expected, "{source}");
        }
    }
}
