//! Row presentation belongs to the accepted graph, even within one styled word.

use mant_ir::{Block, ResolvedContent, TextAlignment, inline_plain_text};

use super::load;

const FIXTURE: &str = include_str!("alignment-passes-rj.1");

fn check_publication(query: &ResolvedContent) {
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(query)).unwrap();
    assert!(!wire.contains("\\u0000mant:"), "{wire}");
    let markdown = mant_codec::encode::render_markdown_with_options(
        query,
        mant_codec::encode::MarkdownOptions::ADDRESSABLE,
    );
    assert!(!markdown.contains("projected-blank:"), "{markdown}");
}

#[test]
fn alignment_receipts_cross_restored_blanks_and_inner_style_returns() {
    // All exact sources ran pristine ASCII/UTF-8 at 40 and 80 columns,
    // tree and lint. term.c:162-220 aligns every accepted pass. In the
    // second captured TEXT, both Y and BETA print on separate aligned rows,
    // though restoration of their accepted blanks adds no source slot.
    for (request, alignment) in [("ce", TextAlignment::Center), ("rj", TextAlignment::Right)] {
        for hanging in [false, true] {
            for (opening, change, closing) in [
                ("", "", ""),
                ("\\fB", "", "\\fP"),
                ("\\fI", "\\fR", ""),
                ("\\X'tty: link https://example.test'", "", "\\X'tty: link'"),
            ] {
                for tail in ["\\&", "\\z"] {
                    let source = FIXTURE
                        .replace(".rj 2", &format!(".{request} 2"))
                        .replace(".HP 275u\n", if hanging { ".HP 275u\n" } else { "" })
                        .replace(
                            "\\fB\\zY\\p\\: \\p BETA\\fP\n\\&\n",
                            &format!("{opening}\\zY\\p\\: \\p {change}BETA{closing}\n{tail}\n"),
                        );
                    let query = load(&source);
                    check_publication(&query);
                    let document = query.document.as_ref().unwrap();
                    let Block::Paragraph {
                        children,
                        inline_layout,
                        ..
                    } = &document.sections[1].blocks[0]
                    else {
                        panic!("missing accepted paragraph: {source}");
                    };
                    let text = inline_plain_text(children);
                    let rows: Vec<_> = text.split('\n').take(5).map(str::trim_start).collect();
                    assert_eq!(rows, ["BEFOREMARK", "ALPHAX", "", "Y", "BETA"], "{source}");
                    let hints = inline_layout
                        .row_hints
                        .iter()
                        .filter(|hint| hint.alignment != TextAlignment::Left)
                        .map(|hint| (hint.row, hint.alignment, hint.indent_columns))
                        .collect::<Vec<_>>();
                    assert_eq!(
                        hints,
                        [(1, alignment, 0), (3, alignment, 0), (4, alignment, 0)],
                        "{source}"
                    );
                    for width in [40, 80] {
                        let rendered = mant_render::render_query_man_at_width(&query, width - 5);
                        let rows = super::description(&rendered)
                            .split('\n')
                            .collect::<Vec<_>>();
                        let native_columns = match (alignment, hanging) {
                            (TextAlignment::Center, false) => {
                                [19, 22, 21].map(|col| col + (width - 40) / 2)
                            }
                            (TextAlignment::Center, true) => {
                                [25, 28, 27].map(|col| col + (width - 40) / 2)
                            }
                            _ => [34, 39, 36].map(|col| col + width - 40),
                        };
                        for (index, (row, native)) in [rows[1], rows[3], rows[4]]
                            .into_iter()
                            .zip(native_columns)
                            .enumerate()
                        {
                            let graph = row.len() - row.trim_start_matches(' ').len();
                            // The native 275u offset has a fractional column.
                            // Its centered BETA can round one column above
                            // the portable 11-column hanging indentation.
                            let rounding = usize::from(
                                hanging && alignment == TextAlignment::Center && index == 2,
                            );
                            assert_eq!(graph + 5 + rounding, native, "width {width}: {source}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn restored_blank_positions_do_not_escape_without_a_hint_or_after_rejection() {
    // Both exact variants ran pristine ASCII/UTF-8/HTML/tree/lint.
    for (source, rejected) in [
        (FIXTURE.replace(".rj 2\n", ""), false),
        (
            FIXTURE.replace("BETA\\fP\n", "BETA\\fP\\p \\p REJECTED\n"),
            true,
        ),
    ] {
        let query = load(&source);
        check_publication(&query);
        let plain = mant_render::render_query_man(&query);
        assert!(plain.contains("BETA"), "{plain:?}");
        if rejected {
            assert!(!plain.contains("REJECTED"), "{plain:?}");
        }
    }
}
