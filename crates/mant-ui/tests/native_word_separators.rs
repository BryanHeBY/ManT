//! Native no-fill separators survive real JSON and viewport rendering.

use mant_ir::ResolvedContent;
use mant_ui::DocumentView;

const MDOC: &str =
    ".Dd October 1, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
const MAN: &str = ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n";

fn round_trip(source: &str) -> ResolvedContent {
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&loaded, false).unwrap();
    assert!(!json.contains("\\u0000mant:"), "private word owner leaked");
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

fn assert_row_cells(actual: &str, expected: &str, source: &str) {
    assert!(
        actual.starts_with(expected),
        "{source}\n{actual:?} != {expected:?}"
    );
    // Literal render rows reserve a rectangle with background padding. The
    // authored prefix is exact; only cells after its end may be padding.
    assert!(
        actual[expected.len()..]
            .chars()
            .all(|character| character == ' '),
        "unexpected cells after authored row: {source}\n{actual:?}"
    );
}

#[test]
fn empty_and_zero_width_native_words_keep_literal_columns_after_json() {
    // Every exact source ran pristine before these assertions. term_word()
    // buffers its separator before decoding; encode1() can then consume it
    // (term.c:573-589, 901-908). Compare columns against the same no-fill
    // origin, retaining actual source rows rather than folded whitespace.
    let baseline = round_trip(&format!("{MDOC}.nf\n.No B\n.fi\n.Sh ENDTEST\n.No FINISH\n"));
    for (body, expected, extra_rows) in [
        (".nf\n.No \\& No B\n.fi\n", " B", 0),
        (".nf\n.No \"\" No B\n.fi\n", " B", 0),
        (".nf\n.No \\fB No B\n.fi\n", " B", 0),
        (".nf\n.No \\z No B\n.fi\n", " B", 0),
        (".nf\n.No \\zX No B\n.fi\n", "XB", 0),
        (".nf\n.No \"\" No \"\" No B\n.fi\n", "  B", 0),
        (".nf\n.No \\&\n.No B\n.fi\n", "B", 1),
        (".nf\n.No \\&\\c\n.No B\n.fi\n", "B", 0),
        (".nf\n.No \\&\\c\n.Sm off\n.No B\n.fi\n", "B", 0),
        (".nf\n.Bk -words\n.No \\zX No B\n.Ek\n.fi\n", "XB", 0),
        (".nf\n.An \"\" An B\n.fi\n", " B", 0),
        (".nf\n.No \\& Aq B\n.fi\n", " ⟨B⟩", 0),
    ] {
        let source = format!("{MDOC}{body}.Sh ENDTEST\n.No FINISH\n");
        let query = round_trip(&source);
        for width in [80, 120, 20] {
            let origin = DocumentView::new(&baseline).render(width);
            let origin_heading = &origin.search("DESCRIPTION")[0];
            let origin_hits = origin.search("B");
            let origin_hit = origin_hits
                .iter()
                .find(|hit| hit.row > origin_heading.row)
                .unwrap();
            let prefix: String = origin.text.lines[origin_hit.row]
                .to_string()
                .chars()
                .take(origin_hit.start_column)
                .collect();
            let rendered = DocumentView::new(&query).render(width);
            let heading = &rendered.search("DESCRIPTION")[0];
            let end_heading = &rendered.search("ENDTEST")[0];
            let hits = rendered
                .search("B")
                .into_iter()
                .filter(|hit| hit.row > heading.row && hit.row < end_heading.row)
                .collect::<Vec<_>>();
            assert_eq!(hits.len(), 1, "{source}\n{:?}", rendered.text);
            let hit = &hits[0];
            assert_row_cells(
                &rendered.text.lines[hit.row].to_string(),
                &format!("{prefix}{expected}"),
                &format!("{source}, width={width}"),
            );
            assert_eq!(
                hit.row - heading.row,
                origin_hit.row - origin_heading.row + extra_rows,
                "{source}, width={width}"
            );
            assert!(!rendered.text.to_string().contains('\u{fffd}'));
        }
    }
}

#[test]
fn generated_link_post_words_keep_control_body_columns_after_json() {
    // Exact UR/MT sources ran pristine first. post_UR() executes the native
    // '<' word after BODY controls without a synthetic source-row boundary
    // (man_term.c:896-910). Its column is independent of target eligibility.
    for (start, end) in [("UR", "UE"), ("MT", "ME")] {
        let baseline = round_trip(&format!(
            "{MAN}.nf\n.{start} https://example.org\n.{end}\nafter\n.fi\n.SH ENDTEST\nFINISH\n"
        ));
        for (body, expected) in [
            ("\\&\n", " <https://example.org>"),
            ("\\&\\c\n", "<https://example.org>"),
            ("\\z\n", " https://example.org>"),
            ("\\zX\n", "X<https://example.org>"),
            ("\\fB\n", " <https://example.org>"),
        ] {
            let source = format!(
                "{MAN}.nf\n.{start} https://example.org\n{body}.{end}\nafter\n.fi\n.SH ENDTEST\nFINISH\n"
            );
            let query = round_trip(&source);
            for width in [80, 120] {
                let origin = DocumentView::new(&baseline).render(width);
                let origin_hit = &origin.search("https://example.org")[0];
                let prefix: String = origin.text.lines[origin_hit.row]
                    .to_string()
                    .chars()
                    .take(origin_hit.start_column - 1)
                    .collect();
                let rendered = DocumentView::new(&query).render(width);
                let hits = rendered.search("https://example.org");
                assert_eq!(hits.len(), 1, "{source}\n{:?}", rendered.text);
                let hit = &hits[0];
                assert_row_cells(
                    &rendered.text.lines[hit.row].to_string(),
                    &format!("{prefix}{expected}"),
                    &format!("{source}, width={width}"),
                );
                assert_eq!(rendered.search("after")[0].row, hit.row + 1);
            }
        }
    }
}
