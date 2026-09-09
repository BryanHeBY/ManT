//! Device alignment is omitted, but native captured rows remain exact.
use super::*;

#[test]
fn aligned_payload_and_empty_flush_rows_match_cli_at_each_viewport() {
    for (header, open, close) in [
        (".TH PROBE 1\n.SH DESCRIPTION", "", ""),
        (".TH PROBE 1\n.SH DESCRIPTION", ".nf\n", ".fi\n"),
        (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            "",
            "",
        ),
        (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal -compact\n",
            ".Ed\n",
        ),
        (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -unfilled -compact\n",
            ".Ed\n",
        ),
    ] {
        for request in ["ce", "rj"] {
            for (body, expected) in [
                ("17\n23", "BEFORE\n17\n23\nAFTER"),
                (".ft B\n17\n23", "BEFORE\n\n17\n23\nAFTER"),
                ("ALPHA\n.sp 1\nBETA", "BEFORE\nALPHA\n\n\nBETA\nAFTER"),
                ("ALPHA\\c\nBETA", "BEFORE\nALPHA\nBETA\nAFTER"),
                ("ALPHA\n.br\nBETA", "BEFORE\nALPHA\n\nBETA\nAFTER"),
                ("ALPHA\n.fi\nBETA", "BEFORE\nALPHA\n\nBETA\nAFTER"),
                ("ALPHA\n.nf\nBETA", "BEFORE\nALPHA\n\nBETA\nAFTER"),
                ("ALPHA\n.br\n.br\nBETA", "BEFORE\nALPHA\n\nBETA\nAFTER"),
                ("ALPHA\n.fi\n.nf\nBETA", "BEFORE\nALPHA\n\nBETA\nAFTER"),
                ("ALPHA\\c\n.br\nBETA", "BEFORE\nALPHA\n\nBETA\nAFTER"),
                ("ALPHA\n.ti 3n\nBETA", "BEFORE\nALPHA\n\nBETA\nAFTER"),
                (".br\nALPHA\nBETA", "BEFORE\n\nALPHA\nBETA\nAFTER"),
            ] {
                let source =
                    format!("{header}\n{open}BEFORE\n.{request} 2\n{body}\nAFTER\n{close}");
                let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
                let before = query.clone();
                for text in std::iter::once(mant_engine::render_query_text(&query)).chain(
                    [40, 80, 120].map(|width| {
                        DocumentView::new(&query)
                            .render(width)
                            .text
                            .lines
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("\n")
                    }),
                ) {
                    let rows: Vec<_> = text.lines().map(str::trim).collect();
                    let start = rows.iter().position(|row| *row == "BEFORE").unwrap();
                    let end = rows.iter().position(|row| *row == "AFTER").unwrap();
                    assert_eq!(rows[start..=end].join("\n"), expected, "{source}\n{text}");
                }
                assert_eq!(query, before);
            }
        }
    }
}
