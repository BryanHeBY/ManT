//! HANG HEAD/BODY word boundaries use the same device origin.

use mant_loader::load_roff_bytes;
use mant_protocol::QueryBundle;

#[test]
fn nested_hang_preserves_only_proven_word_boundaries() {
    // Both ASCII and UTF-8 on the pinned CVS reference print B and C as
    // separate words in the first five cases, but join BBBBB and C in the
    // overrun case. mdoc_term.c::termp_it_pre sets rmargin to offset + width;
    // term.c::term_flushln compares it with the same device's viscol.
    let cases = [
        ("nested short", true, "2n", ".sp 1", "B", "B C D"),
        ("unnested", false, "2n", ".sp 1", "B", "B C D"),
        ("nested wide", true, "8n", ".sp 1", "B", "B C D"),
        ("nested br", true, "2n", ".br", "B", "A B C D"),
        ("nested sp zero", true, "2n", ".sp 0", "B", "A B C D"),
        ("nested overrun", true, "2n", ".sp 1", "BBBBB", "BBBBBC D"),
    ];
    for (name, nested, width, event, last_head, expected) in cases {
        let outer_open = if nested {
            ".Bl -hang -width 2n -compact\n.It O\n"
        } else {
            ""
        };
        let outer_close = if nested { ".El\n" } else { "" };
        let source = format!(
            ".Dd November 26, 2026\n.Dt HANG 1\n.Os\n.Sh DESCRIPTION\n{outer_open}.Bl -hang -width {width} -compact\n.It Xo\n.No A\n{event}\n.No {last_head}\n.Xc\nC D\n.El\n{outer_close}"
        );
        let original = load_roff_bytes(source.as_bytes()).unwrap();
        let wire = mant_render::render_query_json(&original, false).unwrap();
        let restored: QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored: mant_ir::ResolvedContent = restored.into();
        assert_eq!(
            original.document, restored.document,
            "{name}: JSON round trip"
        );
        for content in [&original, &restored] {
            let rendered = mant_render::render_query_man(content);
            let body = rendered.split_once("DESCRIPTION\n").unwrap().1;
            let actual = body.lines().find_map(|line| {
                let words = line.split_whitespace().collect::<Vec<_>>().join(" ");
                words.contains('C').then_some(words)
            });
            assert_eq!(actual.as_deref(), Some(expected), "{name}: {rendered:?}");
        }
    }
}
