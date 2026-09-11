#![cfg(feature = "render")]

use libmandoc_rs::{RenderFormat, Renderer};

#[test]
fn utf8_tabs_measure_character_cells_in_native_basic_units() {
    let source = ".TH WIDTH 7\n.SH BODY\n.nf\n.ta 4n 8n\n界\tTAIL\nab\tTAIL\n.fi\n";
    let report = Renderer::new(RenderFormat::Utf8)
        .with_width(40)
        .render_bytes("width.7", source.as_bytes())
        .expect("render tabs after a double-width character or two ASCII characters");

    // Independently reproduced with unpatched CVS mandoc -T utf8 -O width=40.
    // Both prefixes occupy two cells; the four-en tab adds two spaces.
    assert!(report.output.lines().any(|line| line == "     界  TAIL"));
    assert!(report.output.lines().any(|line| line == "     ab  TAIL"));
}

#[test]
fn html_rfc_references_require_an_all_digit_suffix() {
    let source = concat!(
        ".Dd September 11, 2026\n.Dt REFERENCES 7\n.Os ManT\n",
        ".Sh NAME\n.Nm references\n.Nd bibliography rendering\n",
        ".Sh SEE ALSO\n",
        ".Rs\n.%R RFC 9110\n.Re\n",
        ".Rs\n.%R RFC 9110suffix\n.Re\n",
        ".Rs\n.%R RFC 9110é\n.Re\n",
        ".Rs\n.%U https://example.org/spec\n.Re\n",
    );
    let report = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("references.7", source.as_bytes())
        .expect("render numeric and nonnumeric RFC references");

    assert!(
        report
            .output
            .contains("href=\"https://www.rfc-editor.org/rfc/rfc9110.html\"")
    );
    assert_eq!(
        report
            .output
            .matches("https://www.rfc-editor.org/rfc/")
            .count(),
        1,
        "only the fully numeric RFC reference becomes an automatic link"
    );
    let html = report
        .output
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(html.contains("<span class=\"RsR\">RFC 9110suffix</span>"));
    assert!(html.contains("<span class=\"RsR\">RFC 9110&#x00E9;</span>"));
    assert!(report.output.contains("href=\"https://example.org/spec\""));
}

#[test]
fn html_constant_width_fonts_retain_their_distinct_styles() {
    let source = b".TH FONTS 7\n.SH DESCRIPTION\n\
\\f[CR]plain\\f[R] \\f[CB]bold\\f[R] \\f[CI]italic\\f[R]\n";
    let report = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("fonts.7", source)
        .expect("render constant-width font variants");

    assert!(report.output.contains("<span class=\"Li\">plain</span>"));
    assert!(
        report
            .output
            .contains("<span class=\"Li\"><b>bold</b></span>")
    );
    assert!(
        report
            .output
            .contains("<span class=\"Li\"><i>italic</i></span>")
    );
}

#[test]
fn terminal_margin_escapes_do_not_emit_internal_sentinels() {
    for format in [RenderFormat::Ascii, RenderFormat::Utf8] {
        for escape in [r"\&", r"\[not-a-character]", r"\:", r"\~", r"\-"] {
            let source = format!(".TH MARGIN 7\n.SH BODY\n.mc {escape}\nvisible marker\n.mc\n");
            let report = Renderer::new(format)
                .render_bytes("margin.7", source.as_bytes())
                .expect("render a zero-width or unknown margin character");

            assert!(report.output.contains("visible marker"));
            if escape == r"\-" {
                assert!(report.output.lines().any(|line| line.ends_with('-')));
            }
            assert!(
                !report
                    .output
                    .bytes()
                    .any(|byte| matches!(byte, 0x1a | 0x1c..=0x1f)),
                "internal terminal sentinel escaped through the margin path: {format:?}, {escape:?}"
            );
        }
    }
}
