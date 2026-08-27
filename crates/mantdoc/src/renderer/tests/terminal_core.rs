use super::*;

#[test]
fn renderer_resolves_visible_character_escapes_without_changing_ast_spelling() {
    let limits = Limits::default();
    assert_eq!(
        render_visible_text(r"x\N'65'x \[u2014]\&", RenderFormat::Utf8, &limits),
        "xAx —"
    );
    assert_eq!(
        render_visible_text(r"x\N'65'x \[u2014]", RenderFormat::Ascii, &limits),
        "xAx --"
    );
    assert_eq!(
        render_visible_text(r"\[u005C]\N'92'\e\(rs", RenderFormat::Utf8, &limits),
        r"\\\\"
    );
    assert_eq!(
        render_visible_text(
            r"\[u00A2]\[u00C1]\[u03B1]\[u02D8]",
            RenderFormat::Ascii,
            &limits,
        ),
        "/\x08c\x27\x08A<alpha>\x27\x08\x60"
    );
    assert_eq!(
        render_terminal_visible_text(r"\(lqmylib\(rq", RenderFormat::Ascii, &limits),
        "\"mylib\""
    );
    assert_eq!(
        render_terminal_visible_text(r"\(lqmylib\(rq", RenderFormat::Utf8, &limits),
        "“mylib”"
    );
    assert_eq!(
        render_visible_text(
            r"x\N'259'x x\N'XX'x x\N'65XX'x x\N''x x\N665x x\NX65Yx",
            RenderFormat::Utf8,
            &limits,
        ),
        "xx xX'x xAX'x xx x65x xAx"
    );
    assert_eq!(
        render_visible_text(r"\[u2191] \[u21D1]", RenderFormat::Ascii, &limits),
        "|\u{8}^ =\u{8}^"
    );
    assert_eq!(
        render_visible_text(r"e\'e\[']e e\`e\[`]e", RenderFormat::Ascii, &limits),
        "e'ee e`ee"
    );
    assert_eq!(
        render_visible_text(r"e\'e\[']e e\`e\[`]e", RenderFormat::Utf8, &limits),
        "e´ee e`ee"
    );
    assert_eq!(
        render_visible_text(r"e\U'0301' e\C'u0301'", RenderFormat::Utf8, &limits),
        "eU'0301' e\u{301}"
    );
    assert_eq!(
        render_terminal_visible_text(r"\fBname\fR plain", RenderFormat::Utf8, &limits),
        "n\u{8}na\u{8}am\u{8}me\u{8}e plain"
    );
    assert_eq!(
        render_terminal_visible_text(r"\fIitalic\fRroman\fPitalic", RenderFormat::Utf8, &limits,),
        "_\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}croman_\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}c"
    );
    assert_eq!(
        render_terminal_visible_text_with_font(
            r"bold\fRplain\fPbold",
            RenderFormat::Ascii,
            &limits,
            TerminalFont::Bold,
        ),
        "b\u{8}bo\u{8}ol\u{8}ld\u{8}dplainb\u{8}bo\u{8}ol\u{8}ld\u{8}d"
    );
    assert_eq!(
        render_terminal_visible_text(
            r"\f4x\f3x\f2x\f1x\f(BIx\f(CBx\f(CIx\f[]x",
            RenderFormat::Ascii,
            &limits,
        ),
        "_\u{8}x\u{8}xx\u{8}x_\u{8}xx_\u{8}x\u{8}xx\u{8}x_\u{8}xx"
    );
    assert_eq!(
        render_visible_text(r"\*(.T \*[.T] \\*(.T", RenderFormat::Ascii, &limits),
        r"ascii ascii \*(.T"
    );
    assert_eq!(
        render_visible_text(r"\*(.T \*[.T]", RenderFormat::Utf8, &limits),
        "utf8 utf8"
    );
    assert_eq!(
        render_visible_text(
            r"a\[hy]b\[ hy]c a\~b\[~]c a\0b\[0]c",
            RenderFormat::Ascii,
            &limits
        ),
        "a-bhy]c a bc a bc"
    );
}

#[test]
fn renderer_projects_named_and_numeric_control_scalars_without_layout_bytes() {
    let limits = Limits::default();
    assert_eq!(
        render_visible_text(
            r"\[u0000]\N'1'\[u007F]\N'128'",
            RenderFormat::Ascii,
            &limits,
        ),
        "<NUL><SOH><DEL><80>"
    );
    assert_eq!(
        render_visible_text(r"\[u0000]\N'1'\[u007F]\N'128'", RenderFormat::Utf8, &limits,),
        "����"
    );
    assert_eq!(
        render_visible_text(r"\[uD7FB]", RenderFormat::Ascii, &limits),
        "<?>"
    );
    assert_eq!(
        render_visible_text(r"\[u226A]", RenderFormat::Ascii, &limits),
        "<<"
    );
}

#[test]
fn terminal_roff_presentation_controls_do_not_leak_as_source_text() {
    let limits = Limits::default();
    assert_eq!(
        render_terminal_visible_text(r"a\O1b a\O(52b a\O[5dummy]b", RenderFormat::Ascii, &limits),
        "ab ab ab"
    );
    assert_eq!(
        render_terminal_visible_text(r"x\o'|O'x", RenderFormat::Ascii, &limits),
        "x|\u{8}Ox"
    );
    assert_eq!(
        render_terminal_visible_text(r">\l'3n'<", RenderFormat::Ascii, &limits),
        ">___<"
    );
    assert_eq!(
        render_terminal_visible_text(r">\h'0.16i'<", RenderFormat::Ascii, &limits),
        ">  <"
    );
    assert_eq!(
        render_terminal_visible_text(r">\z\fBxbold\fP<", RenderFormat::Ascii, &limits),
        ">x\u{8}x\u{8}b\u{8}bo\u{8}ol\u{8}ld\u{8}d<"
    );
    assert_eq!(
        render_terminal_visible_text(r"a\+b\!c\?d", RenderFormat::Ascii, &limits),
        "a+bcd"
    );
    assert_eq!(
        render_terminal_visible_text(
            r"a\kxb\k(xyc\k[xyz]d a\R'reg 0'b\R'reg \A'y'0'c a\s0b\s(12c\s[123]d\s'123'e\s'1\w'xy'2'f a\s-0b\s-(12c\s-[123]d\s-'123'e\s-'1\w'xy'2'f\s-",
            RenderFormat::Ascii,
            &limits
        ),
        "abcd abc abcdef abcdef"
    );
}

#[test]
fn terminal_roff_p_breaks_at_its_device_word_boundary() {
    let name = SourceName::new("terminal-roff-p.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ESC-P 1\n.Os\n.Sh DESCRIPTION\nno blank: line one\\pline two\n.Pp\nblank after esc: line one\\p line two\n.Pp\nblank before esc: line one \\pline two\n.Pp\nat eol: line one\\p\nline two\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "     no blank: line oneline\n     two\n\n     blank after esc: line one\n     line two\n\n     blank before esc: line one line\n     two\n\n     at eol: line one\n     line two"
            ),
            "{}",
            report.output
        );
}

#[test]
fn terminal_literal_opening_punctuation_does_not_suppress_next_word_spacing() {
    let name = SourceName::new("terminal-literal-opening-punctuation.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH PUNCT 1\n.SH DESCRIPTION\n.tr x\n>>x<<\ntwo words\n",
        ))
        .unwrap();
    assert!(
        report.output.contains(">> << two words"),
        "{}",
        report.output
    );
}

#[test]
fn terminal_equations_keep_ascii_greek_fallback_names() {
    let limits = Limits::default();
    assert_eq!(
        render_terminal_equation_text(r"\[*a] \[*b] \[*g]", RenderFormat::Ascii, &limits),
        "<alpha> <beta> <gamma>"
    );
    assert_eq!(
        render_terminal_equation_text(r"\[*a] \[*b] \[*g]", RenderFormat::Utf8, &limits),
        "α β γ"
    );
}

#[test]
fn terminal_equation_boxes_retain_positions_and_font_beyond_public_text() {
    let equation = EquationTerminal {
        tokens: [
            ("sum", false),
            ("from", false),
            ("{", false),
            ("i", false),
            ("=", false),
            ("1", false),
            ("}", false),
            ("to", false),
            ("inf", false),
            ("1", false),
            ("over", false),
            ("{", false),
            ("i", false),
            ("sup", false),
            ("2", false),
            ("}", false),
        ]
        .into_iter()
        .map(|(text, quoted)| EquationTerminalToken {
            text: text.into(),
            quoted,
        })
        .collect(),
    };
    assert_eq!(
        render_terminal_equation(&equation, RenderFormat::Ascii, &Limits::default()),
        "<sum>_(_\x08i = 1)^<infinity> 1/(_\x08i^2)"
    );
    assert_eq!(
        render_html_equation(&equation, &Limits::default()),
        "<mrow><munderover><mo>&#x2211;</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mo>&#x221E;</mo></munderover><mfrac><mn>1</mn><mrow><msup><mi>i</mi><mn>2</mn></msup></mrow></mfrac></mrow>"
    );

    let bold = EquationTerminal {
        tokens: [
            ("bold", false),
            ("{", false),
            ("sin", false),
            ("sin", true),
            ("}", false),
            ("text", true),
            ("bold", false),
            ("x", false),
            ("hat", false),
        ]
        .into_iter()
        .map(|(text, quoted)| EquationTerminalToken {
            text: text.into(),
            quoted,
        })
        .collect(),
    };
    assert_eq!(
        render_terminal_equation(&bold, RenderFormat::Ascii, &Limits::default()),
        "(\x08(sin s\x08si\x08in\x08n)\x08) _\x08t_\x08e_\x08x_\x08t x\x08x^\x08^"
    );
}

#[test]
fn terminal_hangul_jamo_extended_b_uses_the_pinned_device_width() {
    assert_eq!(terminal_character_width('\u{d7fb}'), 2);
    assert_eq!(display_width("\u{d7fb}"), 2);
    assert_eq!(
        expand_filled_terminal_tabs("\u{d7fb}\tvalue"),
        "\u{d7fb}   value"
    );
    assert_eq!(terminal_character_width('\u{fffe}'), 0);
    assert_eq!(terminal_character_width('\u{10ffff}'), 0);
    assert_eq!(terminal_character_width('\u{0fff}'), 0);
    assert_eq!(terminal_character_width('\u{d7ff}'), 0);
    assert_eq!(terminal_character_width('\u{40000}'), 0);
    assert_eq!(terminal_character_width('\u{c0000}'), 0);
}

#[test]
fn terminal_volume_defaults_match_the_pinned_manual_sections() {
    assert_eq!(terminal_default_volume("2"), "System Calls Manual");
    assert_eq!(terminal_default_volume("3p"), "Perl Library Manual");
    assert_eq!(terminal_default_volume("8"), "System Manager's Manual");
}

#[test]
fn terminal_renderer_keeps_section_headings_out_of_body_flow() {
    let name = SourceName::new("sections.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".TH SECTIONS 1\n.SH NAME\nsections \\- body\n.SH DESCRIPTION\nvisible text\n",
        ))
        .unwrap();
    assert_eq!(
        report.output,
        "SECTIONS(1)                 General Commands Manual                SECTIONS(1)\n\nN\u{8}NA\u{8}AM\u{8}ME\u{8}E\n       sections - body\n\nD\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n       visible text\n\nOpenBSD                                                            SECTIONS(1)\n"
    );
}

#[test]
fn terminal_footer_accumulates_a_final_roff_vertical_space() {
    let name = SourceName::new("footer-final-sp.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FOOTER-FINAL-SP 1\n.Os\n.Sh DESCRIPTION\nlast table field\n.sp\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "     last table field\n\n\nOpenBSD                          July 4, 2017                          OpenBSD\n"
            ),
            "{}",
            report.output
        );
}

#[test]
fn terminal_renderer_wraps_by_display_width() {
    let name = SourceName::new("wrap.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .with_width(20)
            .render(Source::new(
                &name,
                b".TH WRAP 1\n.SH DESCRIPTION\nwide \\[u4E2D]\\[u6587] text stays together on terminal lines\n",
            ))
            .unwrap();
    assert_eq!(
        report.output,
        "WRAP(1)\nGeneral Commands Manual\n\nD\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n       wide 中文\n       text stays\n       together on\n       terminal\n       lines\n\nOpenBSD      WRAP(1)\n"
    );
}

#[test]
fn ascii_terminal_headings_use_deterministic_overstrike_emphasis() {
    let name = SourceName::new("ascii-heading.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH ASCII-HEADING 1\n.SH NAME\nascii-heading \\- test\n",
        ))
        .unwrap();
    assert!(report.output.contains("N\u{8}NA\u{8}AM\u{8}ME\u{8}E"));
    assert_eq!(display_width("N\u{8}N"), 1);
    assert_eq!(display_width("+\u{8}+\u{8}o\u{8}o"), 1);
    assert_eq!(
        render_terminal_bold("name", RenderFormat::Utf8),
        "n\u{8}na\u{8}am\u{8}me\u{8}e"
    );
}

#[test]
fn mdoc_sections_begin_body_at_the_native_five_column_indent() {
    let name = SourceName::new("mdoc-indent.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt MDOC-INDENT 1\n.Os\n.Sh DESCRIPTION\nvisible text\n",
        ))
        .unwrap();
    assert!(report
            .output
            .contains("D\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n     visible text"));
}

#[test]
fn mdoc_section_headings_preserve_inline_semantic_fonts() {
    let name = SourceName::new("mdoc-section-inline-font.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SECTION 1\n.Os OpenBSD\n.Sh SEE Em ALSO\n.Tg reference\n.Rs\n.%A author\n.%J journal\n.%N 42\n.Re\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "S\u{8}SE\u{8}EE\u{8}E _\u{8}A_\u{8}L_\u{8}S_\u{8}O\n     author, _\u{8}j_\u{8}o_\u{8}u_\u{8}r_\u{8}n_\u{8}a_\u{8}l, 42."
            ),
            "{}",
            report.output
        );
}

#[test]
fn empty_mdoc_section_headings_keep_a_blank_device_field() {
    let name = SourceName::new("mdoc-empty-section-heading.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SECTION 1\n.Os OpenBSD\n.Sh DESCRIPTION\nbefore\n.Sh \\ \\&\nafter\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     before\n\n\n     after"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_column_lists_keep_declared_terminal_fields() {
    let name = SourceName::new("mdoc-column-fields.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt COLUMN-FIELDS 1\n.Os\n.Sh DESCRIPTION\n.Bl -column wide column\n.It a Ta b\n.El\n.Bl -column a b c d e\n.It a Ta b Ta c Ta d Ta e\n.El\n",
            ))
            .unwrap();
    // The list labels are intentionally absent from the public AST, but
    // select device fields of `width + 4` (or `width + 3` for five
    // columns).  This remains no-fill terminal geometry, not prose.
    assert!(
        report.output.contains("     a       b\n"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("     a   b   c   d   e\n"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_column_lists_render_tbl_items_structurally() {
    let name = SourceName::new("mdoc-column-tbl.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt COLUMN-TBL 1\n.Os\n.Sh DESCRIPTION\n.Bl -column a b\n.Sy a Ta b\n.TS\nlll.\n1\t2\t3\n4\t5\t6\n.TE\n.Em c Ta d\n.El\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     1   2   3\n     4   5   6\n\n     _\u{8}c    d\n"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_column_list_cells_keep_recovered_displays_structural() {
    let name = SourceName::new("mdoc-column-display.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt COLUMN-DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Bl -column column\n.It column\n.Bd -ragged -offset indent\ninside display\n.El\nafter list\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     column\n\n           inside display after list"),
        "{}",
        report.output
    );
}

#[test]
fn tbl_text_blocks_wrap_at_the_selected_device_field() {
    assert_eq!(
        terminal_table_text_block_lines("This is a very long sentence.", 20),
        ["This is a very long", "sentence."]
    );
    assert_eq!(
        terminal_table_text_block_lines("This is a very long sentence.", 10),
        ["This is a", "very long", "sentence."]
    );
}

#[test]
fn tbl_rows_share_calculated_terminal_columns() {
    let name = SourceName::new("tbl-terminal-columns.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-COLUMNS 1\n.SH DESCRIPTION\nnormal text\n.TS\ntab(:);\nr c l.\n*:*:*\n**:**:**\n.TE\n",
            ))
            .unwrap();
    // The first field is right-aligned, the middle one centered, and
    // the final field left-aligned in the shared five-cell columns.
    assert!(
        report
            .output
            .contains("\n\n        *   *    *\n       **   **   **\n"),
        "{}",
        report.output
    );
}

#[test]
fn tbl_ranges_keep_private_boundaries_and_centering() {
    let name = SourceName::new("tbl-terminal-ranges.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt TBL-RANGES 1\n.Os\n.Sh DESCRIPTION\n.TS\ncenter box; l.\none\n.TE\n.TS\ncenter box; l.\ntwo\n.TE\n",
            ))
            .unwrap();
    // Adjacent source tables are intentionally flat generated Table
    // siblings in the compatible AST. Their private range markers still
    // have to preserve two independent boxed and centered device fields.
    let first = report.output.find("|one |").unwrap();
    let second = report.output.find("|two |").unwrap();
    assert!(
        report.output[first..second].contains("+----+\n\n"),
        "{}",
        report.output
    );
    for line in report.output.lines().filter(|line| line.contains("+----+")) {
        assert_eq!(
            line.bytes().take_while(|byte| *byte == b' ').count(),
            39,
            "{}",
            report.output
        );
    }
}

#[test]
fn tbl_center_uses_one_calculated_offset_for_rules_and_data() {
    let name = SourceName::new("tbl-terminal-centering-grid.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TBL-CENTRE 1\n.SH DESCRIPTION\n.TS\ncenter tab(:); |l||l|.\n_\ntxt:text\n.TE\n",
        ))
        .unwrap();
    // The visual rule has one more intersection glyph than tblcalc's
    // centering width.  Both the rule and content still start at the
    // one precomputed grid offset rather than being centred separately.
    for line in report
        .output
        .lines()
        .filter(|line| line.contains("+----++-----+") || line.contains("|txt ||text |"))
    {
        assert_eq!(
            line.bytes().take_while(|byte| *byte == b' ').count(),
            36,
            "{}",
            report.output
        );
    }
}

#[test]
fn tbl_interior_empty_data_rows_are_terminal_blank_lines() {
    let name = SourceName::new("tbl-empty-data-row.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TBL-EMPTY-ROW 1\n.SH DESCRIPTION\n.TS\nlb\nli\nlb.\nfirst\n\nlast\n.TE\n",
        ))
        .unwrap();
    assert!(
        report.output.contains(
            "\n       f\u{8}fi\u{8}ir\u{8}rs\u{8}st\u{8}t\n\n       l\u{8}la\u{8}as\u{8}st\u{8}t\n"
        ),
        "{}",
        report.output
    );
}

#[test]
fn terminal_c_continuation_attaches_filled_and_literal_source_lines() {
    let name = SourceName::new("roff-c-continuation.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ROFF-C 1\n.Os\n.Sh DESCRIPTION\none\\c\nword\n.Bd -literal\none\\c\nword\n.Ed\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     oneword\n\n     oneword\n"),
        "{}",
        report.output
    );
    let man_name = SourceName::new("roff-c-man-font.1").unwrap();
    let man_report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &man_name,
            b".TH ROFF-C 1\n.SH DESCRIPTION\n.B\none\\c\nword\n",
        ))
        .unwrap();
    assert!(
        man_report
            .output
            .contains("o\u{8}on\u{8}ne\u{8}ew\u{8}wo\u{8}or\u{8}rd\u{8}d"),
        "{}",
        man_report.output
    );
}

#[test]
fn tbl_closing_line_consumes_the_first_following_positive_sp_slot() {
    let name = SourceName::new("tbl-sp-after-table.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TBL-SP 1\n.SH DESCRIPTION\n.TS\nbox;\nl.\nvalue\n.TE\n.sp\nfollowing text\n",
        ))
        .unwrap();
    assert!(
        report.output.contains("+------+\n       following text\n"),
        "{}",
        report.output
    );
}

#[test]
fn tbl_layout_horizontal_cells_override_input_and_preserve_following_sp() {
    let name = SourceName::new("tbl-layout-horizontal-input.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-HORIZONTAL 1\n.SH DESCRIPTION\n.TS\ntab(:);\n_ _\nl l\n- -\nl r\n_ ^\nr.\ncolum one:column two\nleft:right\nnot:printed\nright:left\n.TE\n.sp\nfollowing text\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "-----------------------\n       colum one   column two\n       -----------------------\n       left             right\n       -----------\n           right   left\n\n       following text"
            ),
            "{}",
            report.output
        );
}

#[test]
fn tbl_next_row_vertical_rules_extend_into_the_current_device_row() {
    let name = SourceName::new("tbl-next-row-rules.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-SPACING 1\n.SH DESCRIPTION\n.TS\nbox tab(:);\nl0 l1 |  l2 |  l3 |  l4 |  l5 |  l6 |  l7 |  l8\nl0 l1    l2    l3    l4    l5    l6    l7    l8\nl0 l1 |  l2 || l3 || l4    l5 || l6 |  l7 || l8.\na:b:c:d:e:f:g:h:i\na:b:c:d:e:f:g:h:i\na:b:c:d:e:f:g:h:i\n.TE\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "|ab|c |d ||e    f  || g   |  h   ||  i |\n       |ab|c |d ||e    f  || g   |  h   ||  i |\n       +--+--+--++--------++-----+------++----+"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_bk_keeps_its_body_phrase_without_printing_recovered_head_words() {
    let name = SourceName::new("mdoc-bk-body-keep.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BK-BODY 1\n.Os\n.Sh SYNOPSIS\n.Nm body-keep\n.Ar x x x x x x x x\n.Ar x x x x x x x x\n.Ar x x x x x x x x\n.Ar x x x x x x\n.Bk -invalid ignored\n.Op o Ar a\n.Ek\n.Pp\n.Nm next\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("               [o _\u{8}a]\n\n     n\u{8}ne\u{8}ex\u{8}xt\u{8}t"),
        "{}",
        report.output
    );
    assert!(!report.output.contains("ignored"), "{}", report.output);
}

#[test]
fn mdoc_bk_releases_a_nested_optional_after_its_input_line_break() {
    let name = SourceName::new("mdoc-bk-input-lines.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BK-INPUTLINES 1\n.Os\n.Sh NAME\n.Nm Bk-inputlines\n.Nd input-line word keeps\n.Sh SYNOPSIS\n.Nm\n.Ar x x x x x x x x x x x x x x x x x x x x x x x x x x x\n.Bk -words\n.Oo Oo No a Oc\n.Oo No b Oc Oc Pq input-line boundary\n.Ek\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("[[a]\n                   [b]]"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_synopsis_options_keep_a_complete_later_form_in_its_field() {
    let name = SourceName::new("mdoc-synopsis-options.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SYNOPSIS-OPTIONS 1\n.Os\n.Sh SYNOPSIS\n.Nm ksh\n.Op Fl +abCefhiklmnpruvXx\n.Op Fl +o Ar option\n.Op Fl c Ar string \\*(Ba Fl s \\*(Ba Ar file Op Ar argument ...\n",
            ))
            .unwrap();
    // The final optional form moves as one field to the conventional
    // nine-column continuation rather than leaving `[` on the prior
    // line or breaking the `-s` option at its hyphen.
    assert!(
        report.output.contains("\n         [-\u{8}-c"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains(" | -\u{8}-s\u{8}s | "),
        "{}",
        report.output
    );
    assert!(
        !report.output.contains("\n     [-\u{8}-c"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_bk_keeps_function_argument_boundaries_after_commas() {
    let word = "x".repeat(20);
    let source = format!(
        ".Dd July 4, 2017\n.Dt BK-FUNCTIONS 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.Fn {word} \"{word} {word}\" {word}\n.Pp\n.Fo {word}\n.Fa \"{word} {word}\" {word}\n.Fc\n.Ek\n"
    );
    let name = SourceName::new("mdoc-bk-functions.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(&name, source.as_bytes()))
        .unwrap();
    let bold = "x\u{8}x".repeat(20);
    let italic = "_\u{8}x".repeat(20);
    let one_request_signature = format!("{bold}({italic}\n     {italic}, {italic})");
    let block_signature = format!("{bold}({italic} {italic}, {italic})");
    assert!(
        report.output.contains(&one_request_signature),
        "{}",
        report.output
    );
    assert!(
        report.output.contains(&block_signature),
        "{}",
        report.output
    );
}

#[test]
fn tbl_device_layout_keeps_box_rules_and_decimal_columns_private_to_rendering() {
    let name = SourceName::new("tbl-device-layout.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-DEVICE 1\n.SH DESCRIPTION\n.TS\nbox tab(:);\nr || n | n .\n1:1.00:+42.0\n_\n10:-10.0:3.14\n.TE\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "+---++-------+--------+\n       | 1 ||  1.00 | +42.0  |\n       +---++-------+--------+\n       |10 ||-10.0  |   3.14 |\n       +---++-------+--------+"
            ),
            "{}",
            report.output
        );
}

#[test]
fn tbl_layout_vertical_edges_frame_contents_and_horizontal_rules() {
    let name = SourceName::new("tbl-layout-vertical-edges.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TBL-EDGES 1\n.SH DESCRIPTION\n.TS\n|l|l|.\n_\nA\ttest\n_\n.TE\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("+--+------+\n       |A | test |\n       +--+------+"),
        "{}",
        report.output
    );
}

#[test]
fn tbl_leading_layout_metadata_applies_only_to_the_outer_field() {
    let name = SourceName::new("tbl-leading-metadata.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TBL 1\n.SH DESCRIPTION\n.TS\ntab(:);\n  l l\n  l l\n| l l\n  l l.\n11:12\n21:22\n31:32\n41:42\n.TE\n",
            ))
            .unwrap();
    let mut stack = vec![report.document.node(report.document.root()).unwrap()];
    let mut found = false;
    while let Some(node) = stack.pop() {
        if node.kind() == NodeKind::Table {
            let Some(terminal) = node.table_terminal() else {
                continue;
            };
            if terminal
                .cells
                .first()
                .is_some_and(|cell| cell.before_vertical_rules == 1)
            {
                assert_eq!(terminal.cells[1].before_vertical_rules, 0);
                found = true;
            }
        }
        stack.extend(node.children());
    }
    assert!(found);
}

#[test]
fn tbl_badspan_terminal_columns_follow_the_occupied_span() {
    let name = SourceName::new("tbl-badspan-metadata.1").unwrap();
    let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TBL 1\n.SH DESCRIPTION\n.TS\nallbox tab(:);\nS L S S\nL L L L L L.\nspan:end\n1:2:3:4:5:6\n.TE\n",
            ))
            .unwrap();
    let mut stack = vec![report.document.node(report.document.root()).unwrap()];
    let mut found = false;
    while let Some(node) = stack.pop() {
        if node.kind() == NodeKind::Table && node.table_cells().len() == 2 {
            assert_eq!(node.table_cells()[0].column_span, 3);
            assert_eq!(node.table_terminal().unwrap().data_columns, [1, 4]);
            found = true;
        }
        stack.extend(node.children());
    }
    assert!(found);
}

#[test]
fn tbl_full_rules_keep_the_preceding_layout_grid() {
    let name = SourceName::new("tbl-complex-metadata.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL 1\n.SH DESCRIPTION\n.TS\ntab(:);\n||l||l||\n|l|l|\nll.\n_\na:b\n_\nc:d\n_\ne:f\n_\n.TE\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "       +--++--+\n       |a ||b |\n       +--++--+\n       |c | d |\n       +--+---+\n        e   f\n       --------\n"
            ),
            "{}",
            report.output
        );
}

#[test]
fn tbl_standalone_leading_vertical_layout_line_joins_the_next_row() {
    let name = SourceName::new("tbl-standalone-vertical.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-STANDALONE 1\n.SH DESCRIPTION\n.TS\nl\n|\nr.\ntable text\n_\nbar\nright\n.TE\n.PP\nfollowing text\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "        table text\n       +-----------\n       |       bar\n       |     right\n\n       following text"
            ),
            "{}",
            report.output
        );
}

#[test]
fn tbl_allbox_rules_resume_after_a_spanned_row() {
    let name = SourceName::new("tbl-spanned-allbox.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TBL-SPAN 1\n.SH DESCRIPTION\n.TS\nallbox tab(:);\nL L L\nC S C.\na:b:c\nwide:c\n.TE\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "+--+---+---+\n       |a | b | c |\n       +--+---+---+\n       |wide  | c |\n       +------+---+"
            ),
            "{}",
            report.output
        );
}

#[test]
fn tbl_empty_layout_retains_an_authored_leading_vertical_rule() {
    let name = SourceName::new("tbl-empty-leading-rule.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TBL-EMPTY 1\n.SH DESCRIPTION\n.TS\n|.\ntable text\n.TE\n",
        ))
        .unwrap();
    // The compatible AST recovers the empty format as one normal left
    // column.  tbl nevertheless prints the authored leading `|` rule.
    assert!(
        report.output.contains("\n       |table text\n"),
        "{}",
        report.output
    );
}
