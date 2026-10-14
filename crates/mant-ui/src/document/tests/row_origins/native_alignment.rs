//! Pinned row alignment reaches actual terminal cells after wire round trips.
use super::super::{DocumentView, ResolvedContent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
use serde_json::Value;

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn native_alignment_matches_exact_pristine_rows_in_two_viewports() {
    // All 24 exact sources ran the registered pristine reference first,
    // before this package-local fixture or assertions were written.
    // roff_term.c::roff_term_pre_ce applies CENTER/RIGHT to each accepted
    // term_flushln pass; term.c measures visible cells before padding.
    let fixture: Value = serde_json::from_str(include_str!("fixtures/alignment.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 24);
    assert_eq!(fixture["header"]["expectations_from_product"], false);
    for case in fixture["cases"].as_array().unwrap() {
        let content =
            mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
        let restored: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
            .unwrap()
            .into();
        let view = DocumentView::new(&restored);
        for (width, profile) in [(38_u16, "utf8-40"), (78, "utf8-80")] {
            let rendered = view.render(width);
            let start = rendered
                .text
                .lines
                .iter()
                .position(|line| line.to_string().trim() == "PROBE_START")
                .unwrap()
                + 1;
            let end = rendered.search("AFTER")[0].row;
            let actual = rendered.text.lines[start..=end]
                .iter()
                .map(|line| line.to_string().trim_end_matches(' ').to_owned())
                .collect::<Vec<_>>();
            // A section's UI origin is three columns versus the reference's
            // ordinary five. Use the same available content area and adjust
            // that declared common margin by two columns only;
            // retain all internal empty rows and every alignment column.
            let expected = strings(&case["profiles"][profile]["body_rows"])
                .into_iter()
                .map(|row| row.strip_prefix("  ").unwrap_or(&row).to_owned())
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{} {profile}", case["id"]);
            let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
            let mut buffer = Buffer::empty(area);
            Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
            for (offset, expected) in expected.iter().enumerate() {
                let mut column = 0_u16;
                for glyph in mant_render::cells::graphemes(expected) {
                    assert_eq!(
                        buffer[(column, u16::try_from(start + offset).unwrap())].symbol(),
                        glyph.text(),
                        "{} {profile}",
                        case["id"]
                    );
                    column += u16::try_from(glyph.columns()).unwrap();
                }
            }
        }
    }
}
