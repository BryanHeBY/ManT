use super::*;

#[test]
fn terminal_two_character_math_escapes_use_catalog_ascii_fallbacks() {
    let limits = Limits::default();
    assert_eq!(
        render_visible_text(r"\(<<", RenderFormat::Ascii, &limits),
        "<<"
    );
    assert_eq!(
        render_visible_text(r"\(>>", RenderFormat::Ascii, &limits),
        ">>"
    );
    assert_eq!(
        render_visible_text(r"\(~=", RenderFormat::Ascii, &limits),
        "~="
    );
}

#[test]
fn mdoc_prefix_attaches_only_to_the_next_same_line_token() {
    let name = SourceName::new("terminal-prefix.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(200)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt PREFIX 1\n.Os\n.Sh DESCRIPTION\nClosing\n.Pf . right .\nOpening\n.Pf ( left .\nNormal\n.Pf pre fixed .\nIncomplete\n.Pf prefixed\nto next line.\n.Po enclosure Pf . Pc\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("Closing .right."),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("Opening (left."),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("Normal prefixed."),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("Incomplete prefixed to next line."),
        "{}",
        report.output
    );
    assert!(report.output.contains("enclosure .)"), "{}", report.output);
}

#[test]
fn man_layout_requests_are_not_visible_tagged_field_bodies() {
    let name = SourceName::new("terminal-layout-only-field.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH FIELD 1\n.SH DESCRIPTION\n.IP tag 6n\n.sp 2v\nfollowing IP text\n.TP 6n\ntag\n.sp 2v\nfollowing TP text\n",
            ))
            .unwrap();
    assert!(!report.output.contains("tag  \n"), "{}", report.output);
    assert!(
        report.output.contains("       tag\n\n\n"),
        "{}",
        report.output
    );
}

#[test]
fn output_limit_never_returns_a_partial_report() {
    let name = SourceName::new("render-limit.1").unwrap();
    let error = Renderer::new(RenderFormat::Utf8)
        .with_max_output_bytes(1)
        .render(Source::new(&name, b"plain text\n"))
        .unwrap_err();
    assert_eq!(error.kind, RenderErrorKind::OutputLimit);
}

#[test]
fn parser_configuration_is_reused() {
    let renderer = Renderer::new(RenderFormat::Ascii).with_parser(Parser::default());
    assert_eq!(renderer.width(), 78);
    assert_eq!(renderer.format(), RenderFormat::Ascii);
}
