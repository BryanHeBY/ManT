use super::{Block, MAN, inline_text, lower};

#[test]
fn preceding_tail_depends_on_surviving_old_glyphs() {
    // Each full source ran pinned ASCII/UTF-8/HTML/tree/lint first.
    // term.c::term_fill can accept an invisible NBRZW pass. encode1
    // writes X in the old word; if the heading covers it, the old blank
    // is still a tail row. If X survives, that blank is now interior.
    // print_man_node writes TABREF for the filled SS BLOCK before
    // term_word applies HEAD's HORIZ (term.c:678–714,875–879).
    for mode in ["", ".nf\n"] {
        for (old, title, old_text, gap, heading) in [
            (r"\zX\c", "NEXT", "ALPHA", 1, "NEXT"),
            (r"\zX\c", r"\&", "ALPHA\n\nX", 0, ""),
            (r"\zX", r"\h'-1n'NEXT", "ALPHA\n\nX", 0, "NEXT"),
            (r"\p BETA", r"\h'-2n'NEXT", "ALPHA", 2, ""),
        ] {
            let source = format!(
                "{MAN}{mode}ALPHA\\p \\p\\& {old}\n.SS {title}\n\
                 AFTERMARK\n.SH ENDTEST\nFINISH\n"
            );
            let doc = lower(&source);
            let first = &doc.sections[1];
            let [old, tail @ ..] = first.blocks.as_slice() else {
                panic!("{source}: no preceding BODY");
            };
            let (children, span) = match old {
                Block::Paragraph {
                    children, source, ..
                } if mode.is_empty() => (children, source),
                Block::Preformatted {
                    children, source, ..
                } if !mode.is_empty() => (children, source),
                _ => panic!("{source}: wrong preceding owner: {old:?}"),
            };
            assert_eq!(inline_text(children), old_text, "{source}");
            assert_eq!(span.unwrap().line, if mode.is_empty() { 6 } else { 7 });
            if gap == 0 {
                assert_eq!(tail, [], "{source}");
            } else {
                assert!(matches!(tail, [Block::VerticalSpace { lines, .. }] if *lines == gap));
            }
            let [next] = first.children.as_slice() else {
                panic!("{source}: missing authored subsection");
            };
            assert_eq!(
                inline_text(&next.heading.content).trim_start(),
                heading,
                "{source}"
            );
            assert_eq!(next.spacing_before_lines, 0, "{source}");
            assert!(
                matches!(
                    next.blocks.as_slice(), [Block::Paragraph { children, .. }]
                        if inline_text(children) == "AFTERMARK"
                ),
                "{source}"
            );
        }
    }
}
