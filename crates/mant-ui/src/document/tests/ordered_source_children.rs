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
