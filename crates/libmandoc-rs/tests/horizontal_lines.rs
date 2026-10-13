//! Native drawing arguments distinguish delimiters from custom glyphs.
#![cfg(feature = "render")]

use libmandoc_rs::{RenderFormat, Renderer};
use serde_json::Value;

#[path = "support/native_render_region.rs"]
mod native_render_region;

#[test]
fn native_horizontal_lines_use_consumed_argument_boundaries_and_glyph_cells() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/horizontal_lines.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 11);
    assert_eq!(fixture["header"]["profilesPerSource"], 5);
    assert_eq!(fixture["header"]["expectationsFromProduct"], false);
    assert_eq!(
        fixture["header"]["oracleSha256"],
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    // All exact complete sources ran pristine ASCII/UTF-8/HTML/tree/lint
    // before these assertions. term.c::term_word, ESCAPE_HLINE, selects its
    // default glyph when a2roffsu() consumes seq+sz, rather than interpreting
    // the closing escaped delimiter as an authored drawing glyph. Custom
    // Unicode drawing glyphs still consume their measured character cells.
    for case in fixture["cases"].as_array().unwrap() {
        assert_eq!(case["profiles"]["lint"]["code"], 0, "{}", case["id"]);
        for (profile, format) in [("ascii", RenderFormat::Ascii), ("utf8", RenderFormat::Utf8)] {
            let report = Renderer::new(format)
                .with_width(78)
                .render_bytes("draw.1", case["source"].as_str().unwrap().as_bytes())
                .unwrap();
            assert_eq!(
                native_render_region::description_body(&report.output),
                case["expectedNativeBodies"][profile].as_str().unwrap(),
                "{}: exact {profile} glyphs and rows",
                case["id"]
            );
        }
    }
}
