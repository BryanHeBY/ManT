use super::*;

#[test]
fn mdoc_reference_blocks_apply_bibliography_punctuation_and_fonts() {
    let name = SourceName::new("terminal-reference.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd January 4, 2019\n.Dt REFERENCE 1\n.Os\n.Sh AUTHORS\n.Rs\n.%A first\n.%A second\n.%A third\n.%T title\n.%J journal\n.Re\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "first, second, and third, \"title\", _\u{8}j_\u{8}o_\u{8}u_\u{8}r_\u{8}n_\u{8}a_\u{8}l."
            ),
            "{}",
            report.output
        );
}

#[test]
fn html_reference_blocks_keep_citations_inline_except_in_see_also() {
    let name = SourceName::new("html-reference.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd January 7, 2019\n.Dt REFERENCES 1\n.Os\n.Sh DESCRIPTION\ninitial reference:\n.Rs\n.%A author name\n.%B book title\n.Re\n.Pp\nin a paragraph:\n.Rs\n.%A another author\n.%B another book\n.Re\n.Sh SEE ALSO\ninitial reference:\n.Rs\n.%A author name\n.%B book title\n.Re\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "initial reference: <cite class=\"Rs\"><span class=\"RsA\">author\n    name</span>, <i class=\"RsB\">book title</i>.</cite></p>\n<p class=\"Pp\">in a paragraph: <cite class=\"Rs\"><span class=\"RsA\">another\n    author</span>, <i class=\"RsB\">another book</i>.</cite></p>"
            ),
            "{}",
            report.output
        );
    assert!(
            report.output.contains(
                "<a class=\"permalink\" href=\"#SEE_ALSO\">SEE\n  ALSO</a></h1>\n<p class=\"Pp\">initial reference:</p>\n<p class=\"Pp\"><cite class=\"Rs\"><span class=\"RsA\">author name</span>,\n    <i class=\"RsB\">book title</i>.</cite></p>"
            ),
            "{}",
            report.output
        );
}

#[test]
fn html_ft_requests_keep_font_state_in_one_paragraph() {
    let name = SourceName::new("html-ft.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH FT 1\n.SH DESCRIPTION\ndefault\n.ft I\nitalic\n.ft CR\nliteral\n.ft B\nbold\n.ft I bogus\nitalic again\n.ft P\nstill italic\n.ft\nstill italic\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "default <i>italic</i> <span class=\"Li\">literal</span>\n    <b>bold</b> <i>italic again</i> <i>still italic</i> <i>still italic</i>"
            ),
            "{}",
            report.output
        );
    assert!(!report.output.contains("<p class=\"Pp\">I"));
}

#[test]
fn html_tbl_layout_metadata_merges_rows_and_keeps_fonts() {
    let name = SourceName::new("html-tbl-layout.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH TBL 1\n.SH DESCRIPTION\n.TS\nbox tab(:);\nlb r\nl ri.\nbold:roman\n_\nroman:italic\n.TE\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "<table class=\"tbl\" style=\"border-style: solid;\">\n  <tr style=\"border-bottom-style: solid;\">\n    <td><b>bold</b></td>\n    <td style=\"text-align: right;\">roman</td>\n  </tr>\n  <tr>\n    <td>roman</td>\n    <td style=\"text-align: right;\"><i>italic</i></td>\n  </tr>\n</table>"
            ),
            "{}",
            report.output
        );
    assert_eq!(report.output.matches("<table").count(), 1);
}

#[test]
fn html_escapes_visible_text_and_preserves_parse_diagnostics() {
    let name = SourceName::new("render.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render(Source::new(&name, b".TH RENDER 1\n.SH NAME\n<&>\n"))
        .unwrap();
    assert!(report.output.contains("&lt;&amp;&gt;"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn html_source_lines_are_not_synthetic_break_elements() {
    let name = SourceName::new("html-source-lines.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render(Source::new(
            &name,
            b"first source line\nsecond source line\n",
        ))
        .unwrap();
    assert_eq!(report.output, "first source line\nsecond source line");
}

#[test]
fn html_font_blocks_wrap_only_their_body_and_keep_nested_paragraphs() {
    let name = SourceName::new("html-font-block.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FONT-BLOCK 1\n.Os\n.Sh DESCRIPTION\n.Pp\nnormal text\n.Bf -literal\nliteral text\n.Pp\nliteral paragraph\n.Ef\n",
            ))
            .unwrap();
    assert!(
        report.output.contains(
            "<div class=\"Bf Li\">literal text\n<p class=\"Pp\">literal paragraph</p>\n</div>"
        ),
        "{}",
        report.output
    );
    assert!(!report.output.contains("-literal"), "{}", report.output);
}

#[test]
fn html_one_line_displays_keep_their_phrase_break_and_literal_wrapper() {
    let name = SourceName::new("html-one-line-display.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Tg display\n.D1 spacing  in  and around one-line displays\nempty display:\n.D1\n.Tg literal\n.Dl literal  display\n.Dl\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "<div class=\"Bd\n  Bd-indent\" id=\"display\"><a class=\"permalink\" href=\"#display\">spacing</a> in\n  and around one-line displays</div>"
            ),
            "{}",
            report.output
        );
    assert!(
        report.output.contains("<div class=\"Bd Bd-indent\"></div>"),
        "{}",
        report.output
    );
    assert!(
            report.output.contains(
                "<div class=\"Bd\n  Bd-indent\" id=\"literal\"><code class=\"Li\"><a class=\"permalink\" href=\"#literal\">literal</a>\n  display</code></div>"
            ),
            "{}",
            report.output
        );
}

#[test]
fn html_man_blocks_keep_field_indent_synopsis_and_literal_boundaries() {
    let name = SourceName::new("html-man-blocks.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH BLOCKS 1\n.SH DESCRIPTION\n.PD 2v\n.TP 10n\ntag\nbody\n.HP 10n\nhanging body\n.RS\nindented body\n.PP\nnested body\n.RE\n.SY command\n.I arguments\n.YS\n.PP\nregular paragraph\n.nf\nliteral\ntext\n.fi\nregular tail\n.br\n",
            ))
            .unwrap();
    assert!(report.output.contains(
            "<dl class=\"Bl-tag\">\n  <dt id=\"tag\"><a class=\"permalink\" href=\"#tag\">tag</a></dt>\n  <dd>body</dd>\n</dl>"
        ));
    assert!(
        report
            .output
            .contains("<p class=\"Pp HP\">hanging body</p>")
    );
    assert!(report.output.contains(
        "<div class=\"Bd-indent\">indented body\n<p class=\"Pp\">nested body</p>\n</div>"
    ));
    assert!(report.output.contains(
            "<table class=\"Nm\">\n  <tr>\n    <td><code class=\"Nm\">command</code></td>\n    <td><i>arguments</i></td>\n  </tr>\n</table>"
        ));
    assert!(report.output.contains(
        "<p class=\"Pp\">regular paragraph</p>\n<pre>literal\ntext</pre>\nregular tail\n<br/>"
    ));
    assert!(!report.output.contains("2v"), "{}", report.output);
}

#[test]
fn html_mdoc_displays_keep_nested_blocks_literal_flow_and_targets() {
    let name = SourceName::new("html-mdoc-displays.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Tg outer\n.Bd -ragged -offset indent\nouter text\n.Pq default indent\n.Tg inner\n.Bd -ragged -offset indent\ninner text\n.Ed\nouter text\n.Ed\n.Bl -tag\n.It term\nouter text\n.Bd -ragged -offset 2n\ninner text\n.Ed\nouter text\n.El\n.Tg literal\n.Bd -literal\nliteral display\n.Tg paragraph\n.Pp\nliteral paragraph\n.Ed\n",
            ))
            .unwrap();
    assert!(report.output.contains(
            "<div class=\"Bd Pp\n  Bd-indent\" id=\"outer\"><a class=\"permalink\" href=\"#outer\">outer</a> text\n  (default indent)\n<div class=\"Bd Pp\n  Bd-indent\" id=\"inner\"><a class=\"permalink\" href=\"#inner\">inner</a> text</div>\nouter text</div>"
        ), "{}", report.output);
    assert!(report.output.contains(
            "<dd>outer text\n    <div class=\"Bd Pp Bd-indent\">inner text</div>\n    outer text</dd>"
        ), "{}", report.output);
    assert!(report.output.contains(
            "<div class=\"Bd Pp Li\" id=\"literal\">\n<pre><a class=\"permalink\" href=\"#literal\">literal</a> display\n<mark id=\"paragraph\"></mark>\n<a class=\"permalink\" href=\"#paragraph\">literal</a> paragraph</pre>\n</div>"
        ), "{}", report.output);
}

#[test]
fn html_roff_font_escapes_emit_semantic_and_literal_spans() {
    let name = SourceName::new("html-font-escape.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".TH FONT 1\n.SH DESCRIPTION\n.nf\n\\f4bolditalic\\f3bold\\f2italic\\f1roman\n\\f(CWliteral\\f(CBbold\\f(CIitalic\\fRroman\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "<b><i>bolditalic</i></b><b>bold</b><i>italic</i>roman\n<span class=\"Li\">literal</span><span class=\"Li\"><b>bold</b></span><span class=\"Li\"><i>italic</i></span>roman"
            ),
            "{}",
            report.output
        );
}

#[test]
fn html_plain_paragraphs_fold_at_the_device_output_field() {
    assert_eq!(
        wrap_html_plain_paragraph(
            "We are using the html device. It can also be written as the html device.",
            "<p class=\"Pp\">".len(),
        ),
        "We are using the html device. It can also be written as the html\n    device."
    );
    assert_eq!(
        wrap_html_plain_paragraph("<i>semantic markup stays intact</i>", 14),
        "<i>semantic markup stays intact</i>"
    );
}

#[test]
fn html_tg_marks_and_inline_semantic_macros_stay_in_their_paragraph() {
    let name = SourceName::new("html-tg.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt TAG 1\n.Os\n.Sh DESCRIPTION\n.Pp\n.Tg paragraph\ninitial text\n.Tg macro\n.Ic macro\nfollowing text\n.Tg marker\n.Tg subsection\n.Ss next\ntext\n",
            ))
            .unwrap();
    assert!(report.output.contains(
            "<p class=\"Pp\" id=\"paragraph\">initial text\n    <a class=\"permalink\" href=\"#macro\"><code class=\"Ic\" id=\"macro\">macro</code></a>\n    following text <mark id=\"marker\"></mark></p>"
        ), "{}", report.output);
}

#[test]
fn html_function_macros_keep_callable_links_and_fo_arguments_together() {
    let name = SourceName::new("html-functions.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
            .with_html_fragment(true)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Pp\nautomatic:\n.Fn first\nand\n.Fn second\n.Pp\n.Fn second\nand\n.Fn first\n.Pp\nexplicit:\n.Tg e3\n.Fn third\nand\n.Tg e4\n.Fo fourth\n.Fa void\n.Fc\n",
            ))
            .unwrap();
    assert!(report.output.contains(
            "<p class=\"Pp\" id=\"first\">automatic:\n    <a class=\"permalink\" href=\"#first\"><code class=\"Fn\">first</code></a>() and\n    <code class=\"Fn\">second</code>()</p>"
        ), "{}", report.output);
    assert!(report.output.contains(
            "<p class=\"Pp\" id=\"e3\">explicit:\n    <a class=\"permalink\" href=\"#e3\"><code class=\"Fn\">third</code></a>() and\n    <a class=\"permalink\" href=\"#e4\"><code class=\"Fn\" id=\"e4\">fourth</code></a>(<var class=\"Fa\">void</var>);</p>"
        ), "{}", report.output);
}

#[test]
fn html_no_fill_spacing_request_stays_inside_one_preformatted_region() {
    let name = SourceName::new("html-no-fill-space.1").unwrap();
    let report = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render(Source::new(
            &name,
            b".TH SPACE 1\n.SH DESCRIPTION\n.nf\nfirst\n.sp\nsecond\n.fi\n",
        ))
        .unwrap();
    assert!(
        report.output.contains("<pre>first\n\nsecond</pre>"),
        "{}",
        report.output
    );
}

#[test]
fn html_text_escapes_required_characters_and_non_ascii_scalars() {
    assert_eq!(
        escape_html("'\"<&>\u{a1}\u{1f642}"),
        "'&quot;&lt;&amp;&gt;&#x00A1;&#x1F642;"
    );
}
