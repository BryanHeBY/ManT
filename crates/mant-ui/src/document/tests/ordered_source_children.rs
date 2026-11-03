//! Ordered native siblings reach final paint, selection and target ranges.
use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

const HEADER: &str = concat!(
    ".Dd November 3, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
    ".Nm probe\n.Nd ordered child audit\n"
);

const CASES: &[(&str, &str)] = &[
    (
        ".Sh DESCRIPTION\nGiven twice\n.Xo ( Fl CC ) Xc\ndisables echo.\n",
        "Given twice (-CC) disables echo.",
    ),
    (
        ".Sh DESCRIPTION\nmulticast DNS\n.Dq ( zeroconf )\nlookups.\n",
        "multicast DNS (“zeroconf”) lookups.",
    ),
    (
        ".Sh DESCRIPTION\nnamely\n.Dq Li \"!\" ,\n.No AFTER\n",
        "namely “!” , AFTER",
    ),
    (
        ".Sh SYNOPSIS\n.In zip.h\n.Vt zip_t *archive ;\n",
        "zip_t *archive;",
    ),
    (
        ".Sh DESCRIPTION\n.D1 [ Ns Ar number ] Ar flag status command\n",
        "[number] flag status command",
    ),
    (
        ".Sh DESCRIPTION\n.Bl -tag -width Ds\n.It ( Xr printf 3 )\nBODY.\n.El\n",
        "(printf(3))",
    ),
];

fn check_buffer(rendered: &RenderedDocument, width: u16, expected: &str, original: bool) {
    let hits = rendered.search(expected);
    assert_eq!(hits.len(), 1, "{width}: {expected:?}\n{}", rendered.text);
    let hit = &hits[0];
    let fragments = std::iter::once(RenderedSearchFragment {
        row: hit.row,
        start_column: hit.start_column,
        end_column: hit.end_column,
    })
    .chain(hit.additional_fragments.iter().copied());
    let area = Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    let mut painted = String::new();
    for fragment in fragments {
        for column in fragment.start_column..fragment.end_column {
            painted.push_str(
                buffer[(
                    u16::try_from(column).unwrap(),
                    u16::try_from(fragment.row).unwrap(),
                )]
                    .symbol(),
            );
        }
    }
    if width == 78 {
        assert_eq!(painted, expected);
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column
                },
                focus: TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1
                },
            }),
            expected
        );
    } else {
        // Visual wrapping can consume ordinary word separators. Logical
        // search above still requires the exact source words and spacing.
        assert_eq!(painted.replace(' ', ""), expected.replace(' ', ""));
    }
    if original && expected == "(printf(3))" && width >= 20 {
        let label = &rendered.search("printf(3)")[0];
        let target = LinkTarget::Manual {
            name: "printf".into(),
            manual_section: Some("3".into()),
        };
        assert!(
            rendered
                .link_target_at(label.row, label.start_column - 1)
                .is_none()
        );
        for column in label.start_column..label.end_column {
            assert_eq!(rendered.link_target_at(label.row, column), Some(&target));
        }
        assert!(
            rendered
                .link_target_at(label.row, label.end_column)
                .is_none()
        );
    }
}

#[test]
fn complete_native_children_survive_json_buffer_resize_and_markdown_readback() {
    // These six exact complete inputs ran pristine ASCII/UTF-8/tree/lint
    // first. mdoc_term.c::print_mdoc_node visits every sibling between the
    // part-specific pre/post hooks; delimiter flags only control spacing.
    for (body, expected) in CASES {
        let source = format!("{HEADER}{body}");
        let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let decoded: ResolvedContent = decoded.into();
        let markdown = mant_codec::encode::render_markdown(&decoded);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for (query, original_targets) in [(&decoded, true), (&readback, false)] {
            let view = DocumentView::new(query);
            let original = view.render(78).text;
            for width in [78, 40, 20, 8, 78] {
                check_buffer(&view.render(width), width, expected, original_targets);
            }
            assert_eq!(view.render(78).text, original);
        }
        assert_eq!(
            serde_json::to_string(&mant_protocol::QueryBundle::from(&decoded)).unwrap(),
            wire,
            "viewport layout mutated original ownership"
        );
    }
}

#[test]
fn man_synopsis_posts_keep_physical_rows_and_word_seams_in_final_cells() {
    // These exact inputs ran pristine ASCII/UTF-8/tree/lint before the
    // assertions. man_term.c::post_SY flushes HEAD unconditionally, retains
    // TABREF, and executes BODY's term_newln before restoring its offset.
    // Buffer/selection must consume those results without executing them again.
    let cases = [
        (
            ".TH PROBE 1 \"November 4, 2026\"\n.SH TEST\n.nf\n.SY probe\nALPHA\n.B \\&\nBETA\n.YS\n.fi\n.SH NEXT\nAFTER\n",
            "ALPHA",
            "BETA",
            2,
        ),
        (
            ".TH PROBE 1 \"November 4, 2026\"\n.SH TEST\n.SY \"A\tB\"\nC\tD\n.YS\n.SH NEXT\nAFTER\n",
            "A    B",
            "C  D",
            1,
        ),
        (
            ".TH PROBE 1 \"November 4, 2026\"\n.SH TEST\n.SY \"\\zX\"\nALPHA\n.YS\n.SH NEXT\nAFTER\n",
            "XALPHA",
            "ALPHA",
            0,
        ),
        (
            ".TH PROBE 1 \"November 4, 2026\"\n.SH TEST\n.SY probe\nALPHA BRAVO CHARLIE DELTA\n.YS\n.SH NEXT\nAFTER\n",
            "probe ALPHA BRAVO CHARLIE DELTA",
            "ALPHA BRAVO CHARLIE DELTA",
            0,
        ),
        (
            ".TH PROBE 1 \"November 4, 2026\"\n.SH TEST\n.RS 4n\n.nf\n.SY probe\n.fi\nC\tD\n.YS\nAFTER\n.RE\nOUT\n.SH NEXT\nSENTINEL\n",
            "C    D",
            "AFTER",
            1,
        ),
    ];
    for (source, first, second, row_distance) in cases {
        let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored: ResolvedContent = restored.into();
        for content in [&content, &restored] {
            let view = DocumentView::new(content);
            for width in [78, 40, 78] {
                let rendered = view.render(width);
                check_buffer(&rendered, width, first, false);
                check_buffer(&rendered, width, second, false);
                let first_hit = &rendered.search(first)[0];
                let second_hit = &rendered.search(second)[0];
                assert_eq!(second_hit.row - first_hit.row, row_distance);
                if row_distance == 2 {
                    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
                    let mut buffer = Buffer::empty(area);
                    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
                    for column in 0..width {
                        assert_eq!(
                            buffer[(column, (first_hit.row + 1).try_into().unwrap())].symbol(),
                            " "
                        );
                    }
                }
            }
        }
        assert_eq!(
            serde_json::to_string(&mant_protocol::QueryBundle::from(&restored)).unwrap(),
            wire
        );
    }
}
