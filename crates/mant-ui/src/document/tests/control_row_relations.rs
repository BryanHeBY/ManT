//! Executed word joins survive responsive tab geometry and reader projection.
use super::*;

#[test]
fn tab_head_word_seams_survive_real_json_ui_resize_and_native_readback() {
    // Exact sources are pristine F core1159/1160/1191 and widths0093/94/95.
    // term.c:113-116/374-444 computes accepted device padding; HEAD post runs
    // after Xo restores offset (mdoc_term.c:437-439,946-955). Literal tabs
    // stay literal in IR, so responsive glyph widths cannot reinsert a gap.
    let head =
        ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (width, word, controls, seam) in [
        (8, "\t", ".nf\n.No MID\n.fi\n", "AFTERBodyWord"),
        (32, "\t", ".nf\n.No MID\n.fi\n", "AFTERBodyWord"),
        (4, "\t", ".mc |\n.nf\n.fi\n.mc\n", "AFTER BodyWord"),
        (8, "A\tB", ".br\n", "AFTERBodyWord"),
        (8, "A\tB", ".nf\n.fi\n", "AFTERBodyWord"),
        (8, "A\tB", ".ti\n", "AFTERBodyWord"),
    ] {
        let source = format!(
            "{head}.Bl -hang -width {width}n\n.It Xo\n.No \"{word}\"\n{controls}.No AFTER\n.Xc\n.No BodyWord\n.El\n.Sh NEXT\n.No END\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let encoded = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&encoded).unwrap();
        let query: ResolvedContent = decoded.into();
        let markdown = mant_codec::encode::render_markdown_with_options(
            &query,
            mant_codec::encode::MarkdownOptions::default(),
        );
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for content in [&query, &readback] {
            let view = DocumentView::new(content);
            let initial = view.render(40).text;
            for terminal_width in [20, 40, 80, 120, 40] {
                let rendered = view.render(terminal_width);
                let hits = rendered.search(seam);
                assert_eq!(
                    hits.len(),
                    1,
                    "{source}width={terminal_width}\n{markdown}\n{}",
                    rendered.text
                );
                let hit = &hits[0];
                let first = RenderedSearchFragment {
                    row: hit.row,
                    start_column: hit.start_column,
                    end_column: hit.end_column,
                };
                let copied = std::iter::once(first)
                    .chain(hit.additional_fragments.iter().copied())
                    .map(|part| {
                        rendered.selected_text(RenderedSelection {
                            anchor: TextPosition {
                                row: part.row,
                                column: part.start_column,
                            },
                            focus: TextPosition {
                                row: part.row,
                                column: part.end_column - 1,
                            },
                        })
                    })
                    .collect::<String>();
                // Selection is visual: wrapping may consume the MC case's
                // one separator. Logical search above preserves its word seam;
                // every selected glyph must still map to its original owner.
                if seam.contains(' ') {
                    assert_eq!(copied.replace(' ', ""), seam.replace(' ', ""));
                } else {
                    assert_eq!(copied, seam);
                }
            }
            assert_eq!(view.render(40).text, initial);
        }
    }
}
