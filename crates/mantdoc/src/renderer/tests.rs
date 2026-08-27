use crate::ast::{EquationTerminal, EquationTerminalToken, NodeKind};
use crate::{Limits, Parser, Source, SourceName};

use super::{
    DEFAULT_RENDER_OUTPUT_BYTES, RenderErrorKind, RenderFormat, Renderer,
    TERMINAL_HANGING_INDENT_MARKER, TERMINAL_NONBREAKING_SPACE_MARKER, TerminalFont, display_width,
    escape_html, expand_filled_terminal_tabs, expand_literal_terminal_tabs, render_html_equation,
    render_terminal_bold, render_terminal_equation, render_terminal_equation_text,
    render_terminal_visible_text, render_terminal_visible_text_with_font, render_visible_text,
    terminal_character_width, terminal_default_volume, terminal_mdoc_plain_text_sentence,
    terminal_table_text_block_lines, wrap_html_plain_paragraph, wrap_terminal_output,
};

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

#[test]
fn mdoc_compact_displays_keep_a_line_boundary_without_a_blank_slot() {
    let name = SourceName::new("mdoc-compact-display.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt COMPACT-DISPLAY 1\n.Os\n.Sh DESCRIPTION\npreceding text\n.Bd -ragged -offset indent\nordinary display\n.Ed\ntext between displays\n.Bd -ragged -offset indent -compact\ncompact display\n.Ed\nfollowing text\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "           ordinary display\n     text between displays\n           compact display\n     following text"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_literal_display_keeps_all_words_from_one_source_line_together() {
    let name = SourceName::new("mdoc-literal-phrase.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd August 27, 2026\n.Dt LITERAL-PHRASE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\nfirst second\nthird\n.Ed\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     first second\n     third"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_name_blocks_keep_bold_name_and_description_separator() {
    let name = SourceName::new("mdoc-name.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MDOC-NAME 1\n.Os\n.Sh NAME\n.Nm mdoc-name\n.Nd example description\n",
            ))
            .unwrap();
    assert!(report.output.contains(
        "     m\u{8}md\u{8}do\u{8}oc\u{8}c-\u{8}-n\u{8}na\u{8}am\u{8}me\u{8}e - example description"
    ));
}

#[test]
fn mdoc_description_blocks_resume_at_an_owned_paragraph() {
    let name = SourceName::new("mdoc-nd-paragraph.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ND-PAR 1\n.Os\n.Sh NAME\n.Nm nd-par\n.Nd paragraph macro\nafter one-line description\n.Pp\nUsually, there should not be additional text in the NAME section.\n.Sh DESCRIPTION\nThe text belongs here.\n.Nd stray\ndescription macro\n.Pp\nBack to normal state.\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "n\u{8}nd\u{8}d-\u{8}-p\u{8}pa\u{8}ar\u{8}r - paragraph macro after one-line description\n\n     Usually"
            ),
            "{}",
            report.output
        );
    assert!(
        report.output.contains(
            "The text belongs here.  - stray description macro\n\n     Back to normal state."
        ),
        "{}",
        report.output
    );
}

#[test]
fn terminal_paragraph_and_spacing_elements_create_one_blank_line() {
    let name = SourceName::new("terminal-spacing.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".TH TERMINAL-SPACING 1\n.SH DESCRIPTION\nfirst paragraph\n.sp\nsecond paragraph\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("       first paragraph\n\n       second paragraph")
    );
}

#[test]
fn terminal_vertical_requests_accumulate_across_transparent_anchors() {
    let man_name = SourceName::new("terminal-adjacent-sp.1").unwrap();
    let man = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &man_name,
            b".TH TERMINAL-ADJACENT-SP 1\n.SH DESCRIPTION\nbefore\n.sp\n.sp\nafter\n",
        ))
        .unwrap();
    assert!(
        man.output.contains("       before\n\n\n       after"),
        "{}",
        man.output
    );

    let mdoc_name = SourceName::new("terminal-transparent-spacing.1").unwrap();
    let mdoc = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &mdoc_name,
                b".Dd July 4, 2017\n.Dt TERMINAL-TRANSPARENT-SPACING 1\n.Os\n.Sh DESCRIPTION\nbefore\n.sp\n.Tg anchor\n.Pp\nafter\n",
            ))
            .unwrap();
    assert!(
        mdoc.output.contains("     before\n\n\n     after"),
        "{}",
        mdoc.output
    );
}

#[test]
fn terminal_negative_spacing_suppresses_the_next_paragraph_gap() {
    let name = SourceName::new("terminal-negative-spacing.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH TERMINAL-NEGATIVE-SPACING 1\n.SH DESCRIPTION\nfirst line\n.sp -1v\n.PP\nsecond line\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("       first line\n       second line")
    );
}

#[test]
fn terminal_roff_font_requests_persist_across_sibling_text() {
    let name = SourceName::new("terminal-font-requests.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TERMINAL-FONT-REQUESTS 1\n.SH DESCRIPTION\nplain\n.ft I\nitalic\n.ft B\nbold\n.ft P\nitalic-again\n.ft\nbold-again\n.ft R\nroman\n",
            ))
            .unwrap();
    let expected = format!(
        "       plain {} {} {} {} roman",
        super::render_terminal_font("italic", super::TerminalFont::Italic),
        super::render_terminal_font("bold", super::TerminalFont::Bold),
        super::render_terminal_font("italic-again", super::TerminalFont::Italic),
        super::render_terminal_font("bold-again", super::TerminalFont::Bold),
    );
    assert!(report.output.contains(&expected), "{}", report.output);
}

#[test]
fn terminal_page_offsets_are_relative_and_restore_after_invalid_requests() {
    let name = SourceName::new("terminal-page-offsets.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt PAGE-OFFSETS 1\n.Os\n.Sh DESCRIPTION\ninitial\n.Pp\n.po -2n\nleft\n.Pp\n.po +5n\nright\n.Pp\n.po invalid\nleft again\n.Pp\n.po 0\nfinal\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     initial\n\n   left\n\n        right\n\n   left again\n\n     final"),
        "{}",
        report.output
    );
}

#[test]
fn terminal_spacing_uses_mandoc_scaled_vertical_units() {
    for (source, expected) in [
        ("20u", 0),
        ("21u", 1),
        ("1c", 2),
        ("0.25i", 1),
        ("0.5P", 0),
        ("1P", 1),
        ("6p", 0),
        ("7p", 1),
        ("1n", 1),
        ("3n", 2),
        ("2m", 1),
    ] {
        assert_eq!(
            super::terminal_vertical_span(source),
            Some(expected),
            "{source}"
        );
    }
    assert_eq!(super::terminal_vertical_span("1cx"), Some(2));
    assert_eq!(super::terminal_vertical_span("xxx"), None);
}

#[test]
fn terminal_temporary_indentation_tracks_relative_and_wide_fields() {
    assert_eq!(super::terminal_temporary_indent_target("10n", 7), Some(10));
    assert_eq!(super::terminal_temporary_indent_target("+10n", 7), Some(17));
    assert_eq!(super::terminal_temporary_indent_target("-10n", 7), Some(0));
    assert_eq!(super::terminal_temporary_indent_target("80n", 7), Some(72));
    assert_eq!(super::terminal_temporary_indent_target("+4n", 73), Some(73));
}

#[test]
fn terminal_empty_mdoc_sections_do_not_add_vertical_gaps() {
    let name = SourceName::new("terminal-empty-sections.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EMPTY 1\n.Os\n.Sh SYNOPSIS\n.Sh DESCRIPTION Xo\n.Sh BUGS\nvisible\n",
            ))
            .unwrap();
    let synopsis = report.output.find("S\u{8}S").unwrap();
    let description = report.output[synopsis..]
        .find("D\u{8}D")
        .map(|offset| synopsis + offset)
        .unwrap();
    assert_eq!(
        report.output[synopsis..description].matches('\n').count(),
        1
    );
}

#[test]
fn terminal_empty_mdoc_name_description_retains_its_dash() {
    let name = SourceName::new("terminal-empty-nd.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt EMPTY-ND 1\n.Os\n.Sh NAME\n.Nm empty-nd\n.Nd\n",
        ))
        .unwrap();
    assert!(report.output.contains("d\u{8}d -\n"), "{}", report.output);
}

#[test]
fn terminal_mdoc_variable_types_use_synopsis_lines_but_prose_spacing() {
    let name = SourceName::new("terminal-vt-layout.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt VT 1\n.Os\n.Sh SYNOPSIS\n.Vt extern int first\n.Vt extern int second\n.Sh DESCRIPTION\n.Vt signed int.\nfollowing prose\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("_\u{8}e_\u{8}x_\u{8}t"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("_\u{8}t_\u{8}. following prose"),
        "{}",
        report.output
    );
}

#[test]
fn terminal_mdoc_function_macros_render_semantic_prototypes() {
    let name = SourceName::new("terminal-function-prototype.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FUNCTION 1\n.Os\n.Sh SYNOPSIS\n.Ft int\n.Fn abs \"int value\"\n.Sh DESCRIPTION\n.Ft int\n.Fo abs\n.Fa \"int value\"\n.Fc\n",
            ))
            .unwrap();
    let layout = report.output.replace('\u{8}', "");
    assert!(
        layout.contains("_i_n_t\n     aabbss(_i_n_t _v_a_l_u_e);"),
        "{}",
        report.output
    );
    assert!(
        layout.contains("_i_n_t aabbss(_i_n_t _v_a_l_u_e)"),
        "{}",
        report.output
    );
}

#[test]
fn synopsis_function_arguments_wrap_as_whole_argument_phrases() {
    let name = SourceName::new("terminal-function-wrap.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(30)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FUNCTION 1\n.Os\n.Sh SYNOPSIS\n.Fn function \"verylong argument\" \"other argument\"\n",
            ))
            .unwrap();
    let layout = report.output.replace('\u{8}', "");
    assert!(
            layout.contains(
                "ffuunnccttiioonn(_v_e_r_y_l_o_n_g _a_r_g_u_m_e_n_t,\n         _o_t_h_e_r _a_r_g_u_m_e_n_t);"
            ),
            "{report:?}"
        );
}

#[test]
fn description_fo_arguments_wrap_as_whole_argument_phrases() {
    let name = SourceName::new("terminal-fo-wrap.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(35)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FO-WRAP 1\n.Os\n.Sh DESCRIPTION\n.Fo function\n.Fa \"verylong argument\"\n.Fa \"other argument\"\n.Fc\n",
            ))
            .unwrap();
    let layout = report.output.replace('\u{8}', "");
    assert!(
        layout.contains(
            "ffuunnccttiioonn(_v_e_r_y_l_o_n_g _a_r_g_u_m_e_n_t,\n     _o_t_h_e_r _a_r_g_u_m_e_n_t)"
        ),
        "{report:?}"
    );
}

#[test]
fn long_synopsis_names_keep_default_argument_phrases_in_the_name_field() {
    let name = SourceName::new("terminal-long-name.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LONG-NAME 1\n.Os\n.Sh SYNOPSIS\n.Nm \"This is a terribly long name, it is so long that it does not fit \\\none one single line -\"\n.Fl o\n.Ar\n",
        ))
            .unwrap();
    let layout = report.output.replace('\u{8}', "");
    let argument_line = layout
        .lines()
        .find(|line| line.trim_start().starts_with("_f_i_l_e"))
        .expect("default Ar argument line");
    assert!(argument_line.starts_with(&" ".repeat(70)), "{layout}");
    assert_eq!(argument_line.trim(), "_f_i_l_e _._._.");
}

#[test]
fn recovered_synopsis_names_keep_function_and_enclosure_fields() {
    let name = SourceName::new("terminal-recovered-synopsis-name.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FUNCTION 1\n.Os\n.Sh SYNOPSIS\n.Ft int\n.Fo function\n.Nm name Fc tail\n.Oo oo\n.Nm nm\n.Bk -words\noc\n.Oc\n.Ek\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "f\u{8}fu\u{8}un\u{8}nc\u{8}ct\u{8}ti\u{8}io\u{8}on\u{8}n(n\u{8}na\u{8}am\u{8}me\u{8}e);\n      tail [oo\n     n\u{8}nm\u{8}m oc]"
            ),
            "{}",
            report.output
        );
}

#[test]
fn terminal_mdoc_include_declarations_complete_device_lines() {
    let name = SourceName::new("terminal-fd-layout.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt FD 1\n.Os\n.Sh SYNOPSIS\n.Fd #include <first.h>\n.Fd #include <second.h>\n.Ft int\n.Fn first void\n.Sh DESCRIPTION\n.Fd #include <first.h>\n.Ft int\n.Fn first void\n.Fd #include <second.h>\n",
            ))
            .unwrap();
    let layout = report.output.replace('\u{8}', "");
    assert!(
            layout.contains("##iinncclluuddee <<ffiirrsstt..hh>>\n     ##iinncclluuddee <<sseeccoonndd..hh>>\n\n     _i_n_t"),
            "{}",
            report.output
        );
    assert!(
            layout.contains("##iinncclluuddee <<ffiirrsstt..hh>>\n     _i_n_t ffiirrsstt(_v_o_i_d) ##iinncclluuddee <<sseeccoonndd..hh>>\n"),
            "{}",
            report.output
        );
}

#[test]
fn terminal_mdoc_include_files_switch_between_synopsis_and_prose_forms() {
    let name = SourceName::new("terminal-in-layout.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt IN 1\n.Os\n.Sh SYNOPSIS\n.In first.h\n.In second.h\n.Ft int\n.Fn first void\n.Sh DESCRIPTION\n.In first.h\n",
            ))
            .unwrap();
    let layout = report.output.replace('\u{8}', "");
    assert!(
            layout.contains("##iinncclluuddee <<ffiirrsstt..hh>>\n     ##iinncclluuddee <<sseeccoonndd..hh>>\n\n     _i_n_t"),
            "{}",
            report.output
        );
    assert!(layout.contains("<_f_i_r_s_t_._h>\n"), "{}", report.output);
}

#[test]
fn terminal_hanging_indentation_uses_its_target_after_the_first_wrap() {
    let input = format!(
        "{TERMINAL_HANGING_INDENT_MARKER}0{TERMINAL_HANGING_INDENT_MARKER}       alpha beta gamma delta"
    );
    assert_eq!(
        wrap_terminal_output(&input, 20, DEFAULT_RENDER_OUTPUT_BYTES, 0, 0).unwrap(),
        "       alpha beta\ngamma delta"
    );
}

#[test]
fn terminal_explicit_enclosures_preserve_empty_and_opening_boundaries() {
    let name = SourceName::new("terminal-eo.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Eo\n.Ec\nafter opening\n.Eo <<\n.Ec\nnext\n.No prefix Ns Eo\n.Ec\nclosing\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     before  after opening << next prefix closing"),
        "{}",
        report.output
    );
}

#[test]
fn collected_explicit_enclosures_attach_only_their_matching_tail() {
    let name = SourceName::new("terminal-eo-collected.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\neo\n.Bo\nbo\nec\n.Ec >>\nbc\n.Bc\nno closing\n.Eo <<\n.Bo\n.Ec >>\nbc\n.Bc\nopening only\n.Bo\nbo\n.Eo\n.Bc\n.Ec >>\nclosing only\n",
            ))
            .unwrap();
    assert!(
        report.output.contains(
            "     <<eo [bo ec>> bc] no closing <<[>> bc] opening only [bo ]>> closing only"
        ),
        "{}",
        report.output
    );
}

#[test]
fn terminal_nested_enclosures_only_defer_their_own_recovered_closer() {
    let name = SourceName::new("terminal-nested-closers.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt CLOSERS 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Ao ao\n.Bo bo\n.Nd nd\n.Pq pq bc Bc ac\n.Ac Op op\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("<ao [bo - nd (pq bc] ac)> [op]"),
        "{}",
        report.output
    );
}

#[test]
fn explicit_enclosure_attaches_a_line_start_no_body() {
    let name = SourceName::new("terminal-eo-no.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.Eo <<\n.No prefix Ns Ec\nstray closing\n.Ec >>\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     <<prefix stray closing"),
        "{}",
        report.output
    );
}

#[test]
fn terminal_explicit_enclosures_attach_custom_special_character_delimiters() {
    let name = SourceName::new("terminal-eo-special.1").unwrap();
    let source = b".Dd July 4, 2017\n.Dt EO 1\n.Os\n.Sh DESCRIPTION\n.ds o \\(Fo\n.ds c \\(Fc\n.Eo \\*o\nvalue\n.Ec \\*c\n";
    let ascii = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(&name, source))
        .unwrap();
    let utf8 = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(&name, source))
        .unwrap();
    assert!(ascii.output.contains("     <<value>>"));
    assert!(utf8.output.contains("     «value»"));
}

#[test]
fn terminal_font_blocks_skip_their_retained_validation_head() {
    let name = SourceName::new("terminal-bf-head.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf Sy ignored\nbody\n.Ef\n",
        ))
        .unwrap();
    assert!(report.output.contains("     b\u{8}bo\u{8}od\u{8}dy\u{8}y"));
    assert!(!report.output.contains("ignored"));
}

#[test]
fn terminal_font_blocks_reset_missing_and_unknown_font_arguments() {
    let name = SourceName::new("terminal-bf-missing-font.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf -emphasis\nemphasis\n.Bf\nno argument\n.Ef\nback to emphasis\n.Bf badarg\nbad argument\n.Ef\n.Ef\n",
            ))
            .unwrap();
    assert!(report
            .output
            .contains("_\u{8}e_\u{8}m_\u{8}p_\u{8}h_\u{8}a_\u{8}s_\u{8}i_\u{8}s no argument _\u{8}b_\u{8}a_\u{8}c_\u{8}k"));
    assert!(report.output.contains("_\u{8}s bad argument\n"));
}

#[test]
fn terminal_font_block_closure_inside_an_enclosure_resets_later_text() {
    let name = SourceName::new("terminal-bf-enclosure.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf Em\n.Bo\ninside\n.Ef\nafter\n.Bc\n.Ef\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("_\u{8}[_\u{8}i_\u{8}n_\u{8}s_\u{8}i_\u{8}d_\u{8}e after]")
    );
}

#[test]
fn no_fill_lines_bypass_terminal_width_wrapping() {
    let name = SourceName::new("terminal-no-fill.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .with_width(20)
        .render(Source::new(
            &name,
            b".TH TERMINAL-NO-FILL 1\n.SH DESCRIPTION\n.nf\none two three four five six   \n.fi\n",
        ))
        .unwrap();
    assert!(report.output.contains("       one two three four five six"));
    assert!(!report.output.contains("six   \n"));

    let example = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH TERMINAL-NO-FILL 1\n.SH DESCRIPTION\nregular\n.EX ignored\nliteral\n.EE ignored\nagain\n",
            ))
            .unwrap();
    assert!(
        example
            .output
            .contains("       regular\n       literal\n       again"),
        "{}",
        example.output
    );
    assert!(!example.output.contains("ignored"), "{}", example.output);
}

#[test]
fn filled_terminal_tabs_use_the_native_five_column_stops() {
    let name = SourceName::new("terminal-tabs.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".TH TERMINAL-TABS 1\n.SH DESCRIPTION\nsingle\ttab\n.br\ndouble\t\ttab\n",
        ))
        .unwrap();
    assert!(
        report.output.contains("       single    tab"),
        "{}",
        report.output
    );
    assert!(report.output.contains("       double         tab"));
}

#[test]
fn terminal_tab_stops_are_relative_to_the_text_or_display_field() {
    assert_eq!(expand_filled_terminal_tabs("     1\tx"), "     1    x");
    assert_eq!(expand_filled_terminal_tabs("     \ttab"), "          tab");
    assert_eq!(
        expand_literal_terminal_tabs("       1\tx"),
        "       1       x"
    );
}

#[test]
fn terminal_roff_ta_requests_clear_and_repeat_device_tab_stops() {
    let name = SourceName::new("terminal-roff-ta.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt TA 1\n.Os\n.Sh DESCRIPTION\n.Bd -unfilled\n.ta 3n +6n T 4n +2n\n1\t2\t3\t4\t5\t6\t7\n.ta\n1\t2\t3\n.Ed\n.Bd -literal\n1\t2\t3\n.Ed\n1\t2\t3\n",
            ))
            .unwrap();
    assert!(
            report
                .output
                .contains("     1  2     3   4 5   6 7\n     123\n\n     1       2       3\n     1       2       3"),
            "{}",
            report.output
        );
}

#[test]
fn terminal_same_line_conditional_body_keeps_its_tab_in_the_current_field() {
    let name = SourceName::new("terminal-inline-condition.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH CONDITION 1\n.SH DESCRIPTION\n.nr name 0\nlabel:\n.ie rname\tvalue\n",
        ))
        .unwrap();
    assert!(
        report.output.contains("       label:    value"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_displays_keep_their_offset_and_distinct_tab_stops() {
    let name = SourceName::new("terminal-display-tabs.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY-TABS 1\n.Os\n.Sh DESCRIPTION\n.Bd -unfilled -offset 3n\nsingle\ttab\ndouble\t\ttab\n.Ed\n.Bd -literal -offset 3n\nsingle\ttab\ndouble\t\ttab\n.Ed\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("\n\n        single    tab\n        double         tab\n\n"),
        "{}",
        report.output
    );
    assert!(
        report
            .output
            .contains("\n\n        single  tab\n        double          tab\n\n")
    );
}

#[test]
fn first_literal_mdoc_display_starts_in_the_section_field() {
    let name = SourceName::new("terminal-first-literal-display.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\nfirst\n.Ed\n",
        ))
        .unwrap();
    let lines = report.output.lines().collect::<Vec<_>>();
    let first = lines
        .iter()
        .position(|line| *line == "     first")
        .expect("literal display line");
    assert!(
        first > 0 && !lines[first - 1].is_empty(),
        "{}",
        report.output
    );
}

#[test]
fn first_unoffset_unfilled_mdoc_display_starts_in_the_section_field() {
    let name = SourceName::new("first-unfilled-display.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\n.Bd -unfilled\nfirst\n.Ed\n",
        ))
        .unwrap();
    let lines = report.output.lines().collect::<Vec<_>>();
    let first = lines
        .iter()
        .position(|line| *line == "     first")
        .expect("unfilled display line");
    assert!(
        first > 0 && !lines[first - 1].is_empty(),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_display_closes_on_one_physical_line_without_an_extra_paragraph_gap() {
    let name = SourceName::new("terminal-display-close.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY-CLOSE 1\n.Os\n.Sh DESCRIPTION\n.Bd -ragged\ndisplay text\n.Ed\nfollowing text\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     display text\n     following text"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_centered_displays_center_each_wrapped_terminal_line() {
    let name = SourceName::new("terminal-centered-display.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd January 4, 2019\n.Dt CENTERED 1\n.Os\n.Sh DESCRIPTION\n.Bd -centered -offset indent\nThe text in this centered block is wide enough to not fit on one line.\n.Ed\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "            The text in this centered block is wide enough to not fit on one\n                                          line."
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_authors_render_one_an_macro_per_terminal_line() {
    let name = SourceName::new("terminal-authors.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt AUTHORS 1\n.Os\n.Sh AUTHORS\n.An First Author\n.An Second Author\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     First Author\n     Second Author"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_an_layout_directives_are_terminal_state_not_visible_text() {
    let name = SourceName::new("terminal-an-layout.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt AUTHORS 1\n.Os\n.Sh DESCRIPTION\nsplit follows:\n.An -split ignored\n.An First Author\n.An Second Author\n.Sh AUTHORS\ninline: \n.An First Author\n.An -nosplit ignored\n.An Second Author\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     split follows:\n     First Author\n     Second Author"),
        "{}",
        report.output
    );
    assert!(
        report
            .output
            .contains("     inline: First Author Second Author"),
        "{}",
        report.output
    );
    assert!(!report.output.contains("ignored"), "{}", report.output);
}

#[test]
fn mdoc_op_body_keeps_parsed_punctuation_adjacent() {
    let name = SourceName::new("terminal-op-punctuation.1").unwrap();
    let source = b".Dd July 4, 2017\n.Dt OP 1\n.Os\n.Sh DESCRIPTION\n.Op a \"(\" z\n.Op a . z\n.Op ( (\n.Op . .\n";
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(&name, source))
        .unwrap();
    assert!(report.output.contains("[a (z] [a. z]"), "{}", report.output);
    assert!(report.output.contains("(([] [].."), "{}", report.output);
}

#[test]
fn filled_mdoc_source_lines_use_terminal_sentence_spacing() {
    let name = SourceName::new("terminal-sentence-spacing.1").unwrap();
    let source = b".Dd July 4, 2017\n.Dt SENTENCE-SPACING 1\n.Os\n.Sh DESCRIPTION\nFirst sentence.\nSecond sentence.\n";
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(&name, source))
        .unwrap();
    assert!(
        report
            .output
            .contains("     First sentence.  Second sentence."),
        "{}",
        report.output
    );
}

#[test]
fn filled_man_source_lines_use_terminal_sentence_spacing() {
    let name = SourceName::new("terminal-man-sentence-spacing.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".TH SENTENCE-SPACING 1\n.SH DESCRIPTION\nFirst sentence.\nSecond sentence.\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("       First sentence.  Second sentence.")
    );
}

#[test]
fn terminal_wraps_at_a_fitting_hyphen_before_moving_a_whole_word() {
    let name = SourceName::new("terminal-hyphen.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .with_width(32)
            .render(Source::new(
                &name,
                b".TH HYPHEN 1\n.SH DESCRIPTION\nA line whose final break-here word crosses the margin.\n",
            ))
            .unwrap();
    assert!(report.output.contains("final break-\n       here"));
}

#[test]
fn mdoc_semantic_font_macros_override_inline_font_controls() {
    let name = SourceName::new("terminal-mdoc-fonts.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MDOC-FONTS 1\n.Os\n.Sh DESCRIPTION\n.Fl option \\fR|\\fP tail\n.br\n.Fl \\-long\n.br\n.Sy symbol\n.br\n.Ar argument\n.br\n.Fa parameter\n.br\n.Em emphasis\n.br\n.Ft return\\fBbold\\fPtail\n.br\n.Cd constant\n.br\n.Fd function\n.br\n.Vt plain Sy child Li literal\n",
            ))
            .unwrap();
    assert!(
            report
                .output
                .contains("-\u{8}-o\u{8}op\u{8}pt\u{8}ti\u{8}io\u{8}on\u{8}n | -\u{8}-t\u{8}ta\u{8}ai\u{8}il\u{8}l"),
            "{}",
            report.output
        );
    assert!(
        report
            .output
            .contains("-\u{8}--\u{8}-l\u{8}lo\u{8}on\u{8}ng\u{8}g"),
        "{}",
        report.output
    );
    assert!(
        report
            .output
            .contains("s\u{8}sy\u{8}ym\u{8}mb\u{8}bo\u{8}ol\u{8}l")
    );
    assert!(
        report
            .output
            .contains("_\u{8}a_\u{8}r_\u{8}g_\u{8}u_\u{8}m_\u{8}e_\u{8}n_\u{8}t")
    );
    assert!(
        report
            .output
            .contains("_\u{8}p_\u{8}a_\u{8}r_\u{8}a_\u{8}m_\u{8}e_\u{8}t_\u{8}e_\u{8}r")
    );
    assert!(
        report
            .output
            .contains("_\u{8}e_\u{8}m_\u{8}p_\u{8}h_\u{8}a_\u{8}s_\u{8}i_\u{8}s")
    );
    assert!(
            report.output.contains(
                "_\u{8}r_\u{8}e_\u{8}t_\u{8}u_\u{8}r_\u{8}nb\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}t_\u{8}a_\u{8}i_\u{8}l"
            ),
            "{}",
            report.output
        );
    assert!(
        report
            .output
            .contains("c\u{8}co\u{8}on\u{8}ns\u{8}st\u{8}ta\u{8}an\u{8}nt\u{8}t")
    );
    assert!(
        report
            .output
            .contains("f\u{8}fu\u{8}un\u{8}nc\u{8}ct\u{8}ti\u{8}io\u{8}on\u{8}n")
    );
    assert!(
        report.output.contains(
            "_\u{8}p_\u{8}l_\u{8}a_\u{8}i_\u{8}n c\u{8}ch\u{8}hi\u{8}il\u{8}ld\u{8}d literal"
        ),
        "{}",
        report.output
    );
}

#[test]
fn empty_mdoc_flag_attaches_to_a_same_line_macro() {
    let name = SourceName::new("terminal-empty-flag.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt FLAG 1\n.Os\n.Sh DESCRIPTION\n.Op Fl Ux\n",
        ))
        .unwrap();
    assert!(report.output.contains("[-\u{8}-UNIX]"), "{}", report.output);
}

#[test]
fn mdoc_navigation_and_escape_nodes_do_not_emit_terminal_text() {
    let name = SourceName::new("terminal-transparent.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt TRANSPARENT 1\n.Os\n.Sh DESCRIPTION\n.Tg destination\n.Es < >\nvisible text\n",
            ))
            .unwrap();
    assert!(report.output.contains("visible text"), "{}", report.output);
    assert!(!report.output.contains("destination"), "{}", report.output);
    assert!(!report.output.contains("<>"), "{}", report.output);
}

#[test]
fn mdoc_name_description_uses_the_section_field_without_a_preceding_name() {
    let name = SourceName::new("terminal-mdoc-nd-first.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ND 1\n.Os\n.Sh NAME\n.Nd description without a preceding name\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("\n     - description without a preceding name"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_one_line_displays_complete_their_terminal_lines() {
    let name = SourceName::new("terminal-mdoc-one-line-display.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DISPLAY 1\n.Os\n.Sh DESCRIPTION\nbefore\n.D1 filled display\nafter\n.Dl literal display\nend\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "     before\n           filled display\n     after\n           literal display\n     end"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_cross_references_join_name_and_section() {
    let name = SourceName::new("terminal-mdoc-xr.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt XR 1\n.Os\n.Sh DESCRIPTION\n.Xr echo 1 Ns s\n.br\n.Xr ( echo 1\n.br\n.Xr echo,\n",
            ))
            .unwrap();
    assert!(report.output.contains("echo(1)s"), "{}", report.output);
    assert!(report.output.contains("(echo(1)"), "{}", report.output);
    assert!(report.output.contains("echo,"), "{}", report.output);
}

#[test]
fn mdoc_ns_only_attaches_when_not_at_a_physical_line_start() {
    let name = SourceName::new("terminal-mdoc-ns.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NS 1\n.Os\n.Sh DESCRIPTION\n.Op before\n.Ns Op after\n.br\n.Oo before\n.Oc Ns Op after\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("[before] [after]\n     [before][after]"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_line_start_macros_do_not_inherit_open_delimiter_attachment() {
    let name = SourceName::new("terminal-mdoc-delimiter-boundary.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BOUNDARY 1\n.Os\n.Sh DESCRIPTION\n.Li a (\n.Li b\n.br\nopening (\n.No word\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("a ( b\n     opening ( word"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_links_render_labels_before_bold_targets() {
    let name = SourceName::new("terminal-mdoc-lk.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LK 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.test/ Example site ,\n.br\n.Lk https://only.example/,\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "_\u{8}E_\u{8}x_\u{8}a_\u{8}m_\u{8}p_\u{8}l_\u{8}e _\u{8}s_\u{8}i_\u{8}t_\u{8}e: h\u{8}ht\u{8}tt\u{8}tp\u{8}ps\u{8}s:\u{8}:/\u{8}//\u{8}/e\u{8}ex\u{8}xa\u{8}am\u{8}mp\u{8}pl\u{8}le\u{8}e.\u{8}.t\u{8}te\u{8}es\u{8}st\u{8}t/"
            ),
            "{}",
            report.output
        );
    assert!(
            report.output.contains(
                "h\u{8}ht\u{8}tt\u{8}tp\u{8}ps\u{8}s:\u{8}:/\u{8}//\u{8}/o\u{8}on\u{8}nl\u{8}ly\u{8}y.\u{8}.e\u{8}ex\u{8}xa\u{8}am\u{8}mp\u{8}pl\u{8}le\u{8}e/\u{8}/,\u{8},"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_debug_requests_are_terminal_invisible() {
    let name = SourceName::new("terminal-mdoc-db.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt DB 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Db hidden arguments\nafter\n",
            ))
            .unwrap();
    assert!(report.output.contains("before after"), "{}", report.output);
    assert!(!report.output.contains("hidden"), "{}", report.output);
}

#[test]
fn mdoc_library_macros_complete_line_only_in_library_sections() {
    let name = SourceName::new("terminal-mdoc-lb.3").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LB 3\n.Os\n.Sh LIBRARY\n.Lb mylib\ntext\n.Sh DESCRIPTION\n.Lb mylib\ntext\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("library \"mylib\"\n     text"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("library \"mylib\" text"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_plain_lists_render_item_bodies_with_compact_boundaries() {
    let name = SourceName::new("terminal-mdoc-plain-list.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os\n.Sh DESCRIPTION\n.Bl -item\n.It\nfirst line\n.It ignore\nsecond line\n.It\nthird line\n.El\n.Bl -item -compact\n.It\nfirst compact\n.It ignored\nsecond compact\n.El\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "first line\n\n     second line\n\n     third line\n     first compact\n     second compact"
            ),
            "{}",
            report.output
        );
    assert!(!report.output.contains("ignore"), "{}", report.output);
}

#[test]
fn mdoc_plain_list_completes_its_final_terminal_field() {
    let name = SourceName::new("terminal-plain-list-tail.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -item -offset indent\n.It\nitem body\n.El\nouter text\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("           item body\n     outer text"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_definition_lists_use_tag_width_for_inline_and_continuation_bodies() {
    let name = SourceName::new("terminal-definition-list.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Fl a | b\nlong tag body\n.It Fl c\nshort body\n.El\n.Fl d\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "     -\u{8}-a\u{8}a | -\u{8}-b\u{8}b\n             long tag body\n\n     -\u{8}-c\u{8}c      short body\n     -\u{8}-d\u{8}d"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_definition_lists_handle_overflow_fields_and_quoted_tag_padding() {
    let name = SourceName::new("terminal-definition-overflow.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width 100n\n.It hundred\ntext text\n.El\n.Bl -tag -width 5n\n.It \"a  \"\ntwo\n.El\n",
            ))
            .unwrap();
    let layout = &report.output;
    assert!(
        layout.lines().any(|line| {
            line.trim_start().starts_with("hundred") && line.trim_end().ends_with("text")
        }),
        "{layout}"
    );
    assert!(layout.contains("\n     a      two\n"), "{layout}");
}

#[test]
fn doublebox_uses_two_terminal_rules_and_consumes_two_following_sp_slots() {
    let name = SourceName::new("terminal-doublebox.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ndoublebox;\nL .\none\n.TE\n.sp 2v\nfollowing\n",
        ))
        .unwrap();
    let layout = report.output.replace('\u{8}', "");
    assert_eq!(layout.matches("+----+").count(), 4, "{layout}");
    let final_rule = layout.rfind("+----+").expect("doublebox final rule");
    assert!(
        layout[final_rule..].starts_with("+----+\n       following"),
        "{layout}"
    );
}

#[test]
fn allbox_adds_its_rule_before_an_authored_manual_table_rule() {
    let name = SourceName::new("terminal-allbox-manual.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TABLE 1\n.SH DESCRIPTION\n.TS\ntab(:) allbox;\n||l||l||.\na:b\n_\nc:d\n_\n.TE\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("|a ||b |\n       +--++--+\n       +--++--+\n       |c ||d |"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_definition_list_preserves_a_leading_vertical_body_tail() {
    let name = SourceName::new("terminal-definition-list-vertical-tail.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width 6n\n.It tag\n.sp 2v\nEl sp 2v\n.El\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     tag\n\n\n             El sp 2v"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_definition_list_resumes_structural_display_bodies() {
    let name = SourceName::new("terminal-definition-list-display-tail.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width 6n\n.It tag\nouter text\n.Bd -ragged -offset 2n\ninner text\n.Ed\nouter text\n.El\n",
            ))
            .unwrap();
    assert!(
        report.output.contains(
            "     tag     outer text\n\n               inner text\n             outer text"
        ),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_definition_list_heads_retain_extended_quote_delimiters() {
    let name = SourceName::new("terminal-definition-list-quote.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It prefix Ao\n.No quoted tag\n.Ac\nbody\n.El\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("prefix <quoted tag>"),
        "{}",
        report.output
    );
}

#[test]
fn obsolete_mdoc_es_en_blocks_retain_the_resolved_enclosure() {
    let name = SourceName::new("terminal-obsolete-enclosure.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt ENCLOSURE 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Es << >>\n.En enclosed words\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("<<enclosed words>>"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_marker_lists_retain_private_selector_spelling_and_offset() {
    let name = SourceName::new("terminal-marker-list.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Bl -bullet -offset indent\n.It\nfirst\n.It\n.El\n.Bl -dash\n.It\ndash body\n.El\n.Bl -enum\n.It\nfirst enum\n.It\nsecond enum\n.El\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "     before\n\n           +\u{8}+\u{8}o\u{8}o   first\n\n           +\u{8}+\u{8}o\u{8}o\n\n     -\u{8}-   dash body\n\n     1.   first enum\n\n     2.   second enum"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_nested_lists_at_item_starts_keep_the_outer_device_field() {
    let name = SourceName::new("terminal-nested-list-item-start.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -dash\n.It\n.Bl -dash\n.It\ntext\n.El\n.El\n.Bl -inset\n.It outer\n.Bl -inset\n.It inner\ntext\n.El\n.El\n.Bl -tag -width 4n\n.It outer tag\n.Bl -tag -width 4n\n.It inner tag\ntext\n.El\n.El\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "     -\u{8}-\n\n         -\u{8}-   text\n\n     outer\n\n     inner text\n\n     outer tag\n\n           inner tag\n                 text"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_marker_list_width_outdents_wrapped_body_lines() {
    let name = SourceName::new("terminal-marker-width.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -bullet -width -4n\n.It\nx x x x x x x x x x\n.El\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     +\u{8}+\u{8}o\u{8}o x x x x x x x\n   x x x"),
        "{}",
        report.output
    );
}

#[test]
fn empty_mdoc_lists_complete_the_current_line_without_vertical_spacing() {
    let name = SourceName::new("terminal-empty-list.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\nbefore\n.Bl -bullet\n.El\nafter\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     before\n     after"),
        "{}",
        report.output
    );
    assert!(
        !report.output.contains("     before\n\n"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_tag_list_widths_control_inline_and_outdented_body_fields() {
    let name = SourceName::new("terminal-tag-width.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width -4n\n.It tag\nx x x x x x x x x x\n.El\n.Bl -tag -width 3n\n.It tag\nx x x\n.El\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     tag\n   x x x x x x x x x\n   x"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("     tag  x x x"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_hanging_lists_keep_the_first_body_phrase_on_the_tag_line() {
    let name = SourceName::new("terminal-hanging-list.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -hang -width -4n\n.It tag\nx x x x x x x x x x\n.El\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     tag x x x x x x\n   x x x x"),
        "{}",
        report.output
    );
}

#[test]
fn compact_mdoc_hanging_lists_keep_adjacent_items_on_neighboring_lines() {
    let name = SourceName::new("terminal-compact-hanging-list.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -hang -width 6n -compact\n.It one\nfirst\n.It second\nsecond\n.El\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     one     first\n     second  second"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_overhanging_lists_keep_term_and_body_on_equally_indented_lines() {
    let name = SourceName::new("terminal-overhanging-list.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -ohang\n.It term\nbody\n.El\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     term\n     body\n"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_inset_and_diagnostic_lists_keep_their_private_terminal_fields() {
    let name = SourceName::new("terminal-definition-list-variants.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -inset\n.It \"term  \"\nbody\n.El\n.Bl -diag\n.It label\nbody\n.El\n",
            ))
            .unwrap();
    assert!(report.output.contains("term   body"), "{}", report.output);
    assert!(
        report
            .output
            .contains("l\u{8}la\u{8}ab\u{8}be\u{8}el\u{8}l  body"),
        "{}",
        report.output
    );
}

#[test]
fn empty_mdoc_definition_heads_use_each_list_forms_body_margin() {
    let name = SourceName::new("terminal-empty-definition-head.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag\n.It\ntag body\n.El\n.Bl -ohang\n.It\nohang body\n.El\n.Bl -inset\n.It\ninset body\n.El\n.Bl -diag\n.It\ndiag body\n.El\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("             tag body"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("     ohang body"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("     inset body"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("       diag body"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_tag_list_a2width_handles_roff_scales_and_visible_fallbacks() {
    let name = SourceName::new("terminal-tag-a2width.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(20)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST 1\n.Os OpenBSD\n.Sh DESCRIPTION\n.Bl -tag -width 4m\n.It tag\nx x x x x x\n.El\n.Bl -tag -width xxx\n.It tag\nx x x\n.El\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     tag   x x x x x\n           x"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("     tag  x x x"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_synopsis_nm_declarations_are_bold_and_line_separated() {
    let name = SourceName::new("terminal-synopsis-nm.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt NM 1\n.Os OpenBSD\n.Sh SYNOPSIS\n.Nm first\n.Nm second\n",
        ))
        .unwrap();
    assert!(
        report.output.contains(
            "     f\x08fi\x08ir\x08rs\x08st\x08t\n     s\x08se\x08ec\x08co\x08on\x08nd\x08d"
        ),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_synopsis_nm_keeps_nested_optional_delimiters() {
    let name = SourceName::new("terminal-synopsis-nm-optional.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt NM 1\n.Os\n.Sh SYNOPSIS\n.Nm before Bo within\n",
        ))
        .unwrap();
    assert!(
            report.output.contains(
                "b\u{8}be\u{8}ef\u{8}fo\u{8}or\u{8}re\u{8}e [\u{8}[w\u{8}wi\u{8}it\u{8}th\u{8}hi\u{8}in\u{8}n]\u{8}]"
            ),
            "{}",
            report.output
        );
}

#[test]
fn man_ip_inherits_a_preceding_explicit_tag_field_width() {
    let name = SourceName::new("terminal-ip-field.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.IP first 10n\nfirst body\n.IP second\nsecond body\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("       first     first body\n\n       second    second body"),
        "{}",
        report.output
    );
}

#[test]
fn man_tp_uses_its_tag_field_without_rendering_the_width_argument() {
    let name = SourceName::new("terminal-tp.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1 \"July 4, 2017\"\n.SH DESCRIPTION\nbefore\n.TP 10n\n.I \"plain\"\nfilled text\n.nf\n.TP 10n\ntag\nliteral\ntext\n.fi\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "       before\n\n       _\u{8}p_\u{8}l_\u{8}a_\u{8}i_\u{8}n     filled text\n\n       tag       literal\n                 text"
            ),
            "{}",
            report.output
        );
    assert!(!report.output.contains("10n"), "{}", report.output);
}

#[test]
fn man_tp_skips_extra_header_arguments_and_honours_head_indentation() {
    let name = SourceName::new("terminal-tp-head.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP 10n ignored\ntag\nbody\n.TP 8n\n.in 3n\nshifted\nbody\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("       tag       body"),
        "{}",
        report.output
    );
    assert!(!report.output.contains("ignored"), "{}", report.output);
    assert!(
        report
            .output
            .contains("          shifted\n               body"),
        "{}",
        report.output
    );

    let invalid_width = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TP 1\n.SH DESCRIPTION\n.TP not-a-width\ntag\nbody\n",
        ))
        .unwrap();
    assert!(
        invalid_width.output.contains("       tag    body"),
        "{}",
        invalid_width.output
    );
    assert!(
        !invalid_width.output.contains("not-a-width"),
        "{}",
        invalid_width.output
    );
}

#[test]
fn man_tp_head_font_requests_override_an_open_parent_font() {
    let name = SourceName::new("terminal-tp-nested-font.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH TP 1\n.SH DESCRIPTION\n.TP\n.B\n.I\nitalic term\nbody\n",
        ))
        .unwrap();
    assert!(
            report
                .output
                .contains("       _\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}c _\u{8}t_\u{8}e_\u{8}r_\u{8}m\n              body"),
            "{}",
            report.output
        );
}

#[test]
fn man_tp_field_width_is_shared_until_a_paragraph_reset() {
    let name = SourceName::new("terminal-tp-shared-field.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP 6n\nshort\nbody\n.TP\n20n\nbody\n.PP\nreset\n.TP\n20n\nbody\n",
            ))
            .unwrap();
    assert!(
        report.output.contains(
            "       short body\n\n       20n   body\n\n       reset\n\n       20n    body"
        ),
        "{}",
        report.output
    );
}

#[test]
fn man_empty_tp_before_rs_does_not_leave_field_padding_at_line_end() {
    let name = SourceName::new("terminal-empty-tp-rs.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP 4n\n*\nitem\n.RS 8n\nindented text\n.RE\nmiddle text\n.TP 4n\n*\n.RS 8n\nindented text\n.RE\ntrailing text\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("       *\n               indented text"),
        "{}",
        report.output
    );
    assert!(!report.output.contains("*  \n"), "{}", report.output);
}

#[test]
fn man_empty_hp_before_sibling_rs_keeps_both_vertical_boundaries() {
    let name = SourceName::new("terminal-empty-hp-rs.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH HP 1\n.SH DESCRIPTION\n.RS\nouter text\n.HP 2n\n.RS 4n\ninner text\n.RE\n.RE\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("              outer text\n\n\n                  inner text"),
        "{}",
        report.output
    );
}

#[test]
fn man_tp_uses_pd_density_and_wraps_long_terms() {
    let name = SourceName::new("terminal-tp-spacing.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(40)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.PD 2v\n.TP\nfirst-tag\ntext\n.TP\nsecond-tag\ntext\n.TP 6n\nThis tagged paragraph has ridiculously long text in its head\nbody\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("              text\n\n\n       second-tag"),
        "{}",
        report.output
    );
    assert!(
        report
            .output
            .contains("       This tagged paragraph has\n       ridiculously long text"),
        "{}",
        report.output
    );
}

#[test]
fn man_tp_trailing_nonbreaking_blanks_reserve_but_do_not_print_field_cells() {
    let name = SourceName::new("terminal-tp-trailing-space.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP\ntag\\ \\&\nfirst body\n.TP\ntag\\ \\ \\ \\ \\ \\&\nsecond body\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("       tag    first body\n\n       tag\n              second body"),
        "{}",
        report.output
    );
}

#[test]
fn man_tp_body_wraps_at_its_field_and_keeps_wide_fields_unfilled() {
    let name = SourceName::new("terminal-tp-wrapping.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .with_width(40)
            .render(Source::new(
                &name,
                b".TH TP 1\n.SH DESCRIPTION\n.TP 12n\ntag\nfirst second third fourth fifth sixth\n.TP 100n\nwide\nbody\n",
            ))
            .unwrap();
    assert!(
        report.output.contains(
            "       tag         first second third\n                   fourth fifth sixth"
        ),
        "{}",
        report.output
    );
    assert!(report.output.contains("       wide"), "{}", report.output);
    assert!(
        !report.output.contains("wide\n       body"),
        "{}",
        report.output
    );
}

#[test]
fn man_font_macro_arguments_do_not_add_sentence_spacing() {
    let name = SourceName::new("terminal-man-font-arguments.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH FONT 1\n.SH DESCRIPTION\nEarlier sentence.\nIt works with\n.B several words\nand with\n.B\nnext line\nscope.\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("Earlier sentence.  It works with"),
        "{}",
        report.output
    );
    assert!(
            report
                .output
                .contains("with s\u{8}se\u{8}ev\u{8}ve\u{8}er\u{8}ra\u{8}al\u{8}l w\u{8}wo\u{8}or\u{8}rd\u{8}ds\u{8}s and with"),
            "{}",
            report.output
        );
    assert!(
        report
            .output
            .contains("and with n\u{8}ne\u{8}ex\u{8}xt\u{8}t l\u{8}li\u{8}in\u{8}ne\u{8}e"),
        "{}",
        report.output
    );
    let alternating = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH FONT 1\n.SH DESCRIPTION\n.BI bold italic bold again\n.IR italic roman\n",
        ))
        .unwrap();
    assert!(
            alternating.output.contains(
                "b\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}cb\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}a_\u{8}g_\u{8}a_\u{8}i_\u{8}n"
            ),
            "{}",
            alternating.output
        );
    assert!(
        alternating
            .output
            .contains("_\u{8}i_\u{8}t_\u{8}a_\u{8}l_\u{8}i_\u{8}croman"),
        "{}",
        alternating.output
    );
    let option = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH FONT 1\n.SH DESCRIPTION\nempty\n.OP\nvalue\n.OP -f arg excess\n",
        ))
        .unwrap();
    assert!(option.output.contains("empty []"), "{}", option.output);
    assert!(
        option
            .output
            .contains("value [-\u{8}-f\u{8}f _\u{8}a_\u{8}r_\u{8}g]"),
        "{}",
        option.output
    );
}

#[test]
fn mdoc_no_hyphens_are_not_terminal_break_points() {
    let name = SourceName::new("terminal-mdoc-no-hyphen.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .with_width(32)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NO-HYPHEN 1\n.Os\n.Sh DESCRIPTION\nA line whose final macro argument is\n.No no-break\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("argument is no-break"),
        "{}",
        report.output
    );
    assert!(
        !report.output.contains("no-\n     break"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_apostrophe_macro_attaches_to_both_neighboring_words() {
    let name = SourceName::new("terminal-apostrophe.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt APOSTROPHE 1\n.Os\n.Sh DESCRIPTION\n.An Ingo Ap s .\n.An Kristaps Ap .\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("Ingo's.  Kristaps'."),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_spacing_controls_are_terminal_invisible() {
    let name = SourceName::new("terminal-sm-control.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt SM-CONTROL 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.No visible\n",
        ))
        .unwrap();
    assert!(
        report.output.contains("     visible\n"),
        "{}",
        report.output
    );
    assert!(!report.output.contains("off"), "{}", report.output);
}

#[test]
fn mdoc_spacing_controls_reach_nested_and_recovered_terminal_phrases() {
    let name = SourceName::new("terminal-sm-phrases.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SM-PHRASES 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.No toggle Pq now off\n.Sm bad two\n.No restored words\n.Sm on\n.No final words\n.Sm bad\n.No joined words\n.Pp\n.No prefix\n.Sm off\n.Op outer Op inner\n.Sm on\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("toggle(nowoff)"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("bad two restored words"),
        "{}",
        report.output
    );
    assert!(report.output.contains("final words"), "{}", report.output);
    assert!(
        report.output.contains("badjoined words"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("prefix [outer[inner]]"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_sm_off_preserves_sentence_spacing_at_a_new_source_phrase() {
    let name = SourceName::new("terminal-sm-sentence.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SM-SENTENCE 1\n.Os\n.Sh DESCRIPTION\nfirst sentence.\n.Sm off\n.Em following words\n.Sm on\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("first sentence.  _\u{8}f"),
        "{}",
        report.output
    );
}

#[test]
fn plain_mdoc_text_keeps_its_sentence_boundary_inside_a_list() {
    let name = SourceName::new("terminal-mdoc-list-sentence.1").unwrap();
    let source = b".Dd July 4, 2017\n.Dt LIST-SENTENCE 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It tag\nFirst sentence.\nFollowing text.\n.El\n";
    let parsed = Parser::default().parse(Source::new(&name, source)).unwrap();
    let first_sentence = parsed
        .document
        .preorder()
        .find(|node| node.text() == Some("First sentence."))
        .unwrap();
    assert!(terminal_mdoc_plain_text_sentence(first_sentence));
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(&name, source))
        .unwrap();
    assert!(
        report.output.contains("First sentence.  Following text."),
        "{}",
        report.output
    );
}

#[test]
fn list_sentence_spacing_survives_a_following_explicit_enclosure() {
    let name = SourceName::new("terminal-mdoc-list-enclosure-sentence.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LIST-SENTENCE 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It tag\nFirst sentence.\n.Ao\nquoted text\n.Ac\n.El\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("First sentence.  <quoted text>"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_inline_macro_periods_do_not_create_terminal_sentence_spacing() {
    let name = SourceName::new("terminal-mdoc-inline-period.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MDOC-PERIOD 1\n.Os\n.Sh DESCRIPTION\n.Ad example.\nfollowing prose\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("_\u{8}e_\u{8}x_\u{8}a_\u{8}m_\u{8}p_\u{8}l_\u{8}e_\u{8}. following prose"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_exit_and_return_expansions_start_below_their_labels() {
    let name = SourceName::new("terminal-mdoc-ex-rv.1").unwrap();
    for (source, expected) in [
        (
            b".Dd July 4, 2017\n.Dt EX 1\n.Os\n.Sh EXIT STATUS\nlabel:\n.Ex -std\n".as_slice(),
            "     label:\n     The utility exits 0 on success",
        ),
        (
            b".Dd July 4, 2017\n.Dt RV 3\n.Os\n.Sh RETURN VALUES\nlabel:\n.Rv -std\n".as_slice(),
            "     label:\n     Upon successful completion, the value 0",
        ),
    ] {
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, source))
            .unwrap();
        assert!(report.output.contains(expected), "{}", report.output);
    }
}

#[test]
fn mdoc_nm_keeps_inline_font_changes_inside_its_bold_base() {
    let name = SourceName::new("terminal-nm-font.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NM 1\n.Os\n.Sh DESCRIPTION\nnormal text\n.Nm bold\\fIemphasis\\fPback\ntrailing text\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "normal text b\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}e_\u{8}m_\u{8}p_\u{8}h_\u{8}a_\u{8}s_\u{8}i_\u{8}sb\u{8}ba\u{8}ac\u{8}ck\u{8}k trailing text"
            ),
            "{}",
            report.output
        );
}

#[test]
fn quoted_mdoc_arguments_keep_their_significant_trailing_blanks() {
    let name = SourceName::new("terminal-quoted-trailing.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt QUOTED 1\n.Os\n.Sh DESCRIPTION\n.Fl \"one \" \"two \"\ntext\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("-\u{8}-o\u{8}on\u{8}ne\u{8}e  -\u{8}-t\u{8}tw\u{8}wo\u{8}o  text"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_cd_sentence_ending_punctuation_keeps_normal_spacing() {
    let name = SourceName::new("terminal-cd-punctuation.1").unwrap();
    let input = b".Dd July 4, 2017\n.Dt CD 1\n.Os\n.Sh DESCRIPTION\n.Cd literal .\n.Cd next\n";
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(&name, input))
        .unwrap();
    assert!(
        report.output.contains(
            "l\u{8}li\u{8}it\u{8}te\u{8}er\u{8}ra\u{8}al\u{8}l.  n\u{8}ne\u{8}ex\u{8}xt\u{8}t"
        ),
        "{}",
        report.output
    );
}

#[test]
fn argumentless_man_th_retains_its_terminal_footer() {
    let name = SourceName::new("terminal-th-noarg.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(&name, b".TH\n.SH DESCRIPTION\ntext\n"))
        .unwrap();
    assert!(
        report.output.ends_with(
            "\n\nOpenBSD                                                                     ()\n"
        ),
        "{}",
        report.output
    );
}

#[test]
fn empty_man_section_recovery_keeps_orphaned_blocks_in_the_body_column() {
    let name = SourceName::new("terminal-sh-noarg.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH SH-NOARG 1\n.SH DESCRIPTION\nfirst\n.SH\n.nf\nsecond\n.SH\n.fi\nthird\n.SH\n.TP 6n\ntag\ntagged text\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("       first\n\n       second\n\n       third\n\n       tag   tagged text"),
        "{}",
        report.output
    );
}

#[test]
fn leading_man_section_spacing_does_not_create_a_body_blank_line() {
    let name = SourceName::new("terminal-leading-sp.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH LEADING-SP 1\n.SH DESCRIPTION\n.sp\n.PP\ntext\n",
        ))
        .unwrap();
    let text_start = report.output.find("\n       text\n").unwrap();
    assert_ne!(
        report.output.as_bytes()[text_start - 1],
        b'\n',
        "{}",
        report.output
    );
}

#[test]
fn synopsis_pretty_mdoc_paragraphs_continue_below_the_name_field() {
    let name = SourceName::new("terminal-nm-par.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt NM-PAR 1\n.Os\n.Sh SYNOPSIS\n.Nm\n.Fl a\n.Pp\n.Fl b\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("-\u{8}-a\u{8}a\n\n            -\u{8}-b\u{8}b"),
        "{}",
        report.output
    );
}

#[test]
fn synopsis_paragraphs_inside_optional_name_blocks_keep_their_field() {
    let name = SourceName::new("terminal-nm-parns.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt NM-PARNS 1\n.Os\n.Sh DESCRIPTION\n.nr nS 1\n.Nm\n.Oo Fl a\n.nr nS 0\n.Pp\n.Fl b Oc\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("-\u{8}-a\u{8}a\n\n            -\u{8}-b\u{8}b]"),
        "{}",
        report.output
    );
}

#[test]
fn recovered_bf_closer_ends_the_enclosure_and_font_scope_in_place() {
    let name = SourceName::new("terminal-bf-broken.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF-BROKEN 1\n.Os\n.Sh DESCRIPTION\nbefore both\n.Bo before font block\n.Bf Em\ninside both\n.Bc\nafter bracket\n.Ef\nafter both\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "before both [before font block _\u{8}i_\u{8}n_\u{8}s_\u{8}i_\u{8}d_\u{8}e _\u{8}b_\u{8}o_\u{8}t_\u{8}h] after bracket after both"
            ),
            "{}",
            report.output
        );
}

#[test]
fn recovered_display_closer_resumes_an_open_quote_at_its_outer_margin() {
    let name = SourceName::new("terminal-bd-break.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BD-BREAK 1\n.Os\n.Sh DESCRIPTION\nbefore both\n.Bd -ragged -offset indent\nbefore bracket\n.Bo inside both\n.Ed\nafter display\n.Bc\nafter both\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("           before bracket [inside both\n     after display] after both"),
        "{}",
        report.output
    );
}

#[test]
fn display_opened_inside_a_quote_retains_its_vertical_offset_when_closed() {
    let name = SourceName::new("terminal-bd-broken.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BD-BROKEN 1\n.Os\n.Sh DESCRIPTION\nbefore both\n.Bo before display\n.Bd -ragged -offset indent\ninside both\n.Bc\nafter bracket\n.Ed\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("before both [before display\n\n           inside both] after bracket"),
        "{}",
        report.output
    );
}

#[test]
fn incomplete_man_titles_retain_blank_date_terminal_footers() {
    for (source, left, right) in [
        (
            b".TH ONEARG\n.SH DESCRIPTION\ntext\n".as_slice(),
            "OpenBSD",
            "ONEARG()",
        ),
        (
            b".TH EMPTYDATE 1 \"\" source\n.SH DESCRIPTION\ntext\n".as_slice(),
            "source",
            "EMPTYDATE(1)",
        ),
    ] {
        let name = SourceName::new("terminal-th-incomplete.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, source))
            .unwrap();
        let footer = report.output.trim_end().rsplit('\n').next().unwrap();
        assert!(footer.starts_with(left), "{}", report.output);
        assert!(footer.ends_with(right), "{}", report.output);
    }
}

#[test]
fn overwide_man_header_and_footer_fields_keep_terminal_columns() {
    let cases = [
            (
                b".TH TH-LONGTIT-23456789012345678901234567890123456789012345678901234567890123456789 1 \"November 20, 2014\" source\n.SH DESCRIPTION\nSome text.\n".as_slice(),
                "TH-LONGTIT-23456789012345678901234567890123456789012345678901234567890123456789(1)\n                                                       General Commands Manual",
                "source                         November 20, 2014\nTH-LONGTIT-23456789012345678901234567890123456789012345678901234567890123456789(1)",
            ),
            (
                b".TH TH-LONGDATE 1 \"1234567890123456789012345678901234567890123456789012345678901234567890123456789012\" source\n.SH DESCRIPTION\nSome text.\n".as_slice(),
                "TH-LONGDATE(1)              General Commands Manual             TH-LONGDATE(1)",
                "source\n1234567890123456789012345678901234567890123456789012345678901234567890123456789012\n                                                                TH-LONGDATE(1)",
            ),
        ];
    for (source, expected_header, expected_footer) in cases {
        let name = SourceName::new("terminal-th-overwide.1").unwrap();
        let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(&name, source))
            .unwrap();
        assert!(
            report.output.starts_with(expected_header),
            "{}",
            report.output
        );
        assert!(
            report.output.trim_end().ends_with(expected_footer),
            "{}",
            report.output
        );
    }
}

#[test]
fn overwide_mdoc_system_footer_still_centres_a_fitting_date() {
    let name = SourceName::new("terminal-os-long.1").unwrap();
    let system = "1234567890123456789012345678901234567890123456789012345678901234567890123456789";
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            format!(".Dd July 4, 2017\n.Dt OS-LONG 1\n.Os {system}\n.Sh DESCRIPTION\ntext\n")
                .as_bytes(),
        ))
        .unwrap();
    assert!(
        report.output.ends_with(&format!(
            "{system}\n                                 July 4, 2017\n{system}\n"
        )),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_argumentless_date_retains_the_blank_date_footer() {
    let name = SourceName::new("terminal-dd-noarg.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd\n.Dt DD-NOARG 1\n.Os\n.Sh DESCRIPTION\ntext\n",
        ))
        .unwrap();
    assert!(
        report.output.ends_with(
            "\n\nOpenBSD                                                                OpenBSD\n"
        ),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_title_without_section_uses_the_local_header_volume() {
    let name = SourceName::new("terminal-dt-nosec.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt DT-NOSEC\n.Os\n.Sh DESCRIPTION\ntext\n",
        ))
        .unwrap();
    assert!(
        report.output.starts_with(
            "DT-NOSEC                             LOCAL                            DT-NOSEC\n"
        ),
        "{}",
        report.output
    );
    assert!(report.output.ends_with("OpenBSD\n"), "{}", report.output);
}

#[test]
fn man_pd_controls_later_paragraph_vertical_density_without_visible_text() {
    let name = SourceName::new("terminal-pd.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH PD 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.PD 2v\n.PP\nfirst\n.PP\nsecond\n",
        ))
        .unwrap();
    assert!(
            report.output.contains("D\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n       first\n\n\n       second"),
            "{}",
            report.output
        );
    assert!(!report.output.contains("2v"), "{}", report.output);
}

#[test]
fn man_pd_bare_numeric_argument_adds_terminal_blank_lines() {
    let name = SourceName::new("terminal-pd-bare.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH PD 1 \"July 4, 2017\"\n.SH DESCRIPTION\ninitial\n.PP\ndefault\n.PD 2\n.PP\nnext\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("       default\n\n\n       next"),
        "{}",
        report.output
    );
}

#[test]
fn man_pd_controls_following_section_heading_density() {
    let name = SourceName::new("terminal-pd-section.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH PD 1\n.SH DESCRIPTION\nfirst\n.PD 2\n.SH DOUBLE\nsecond\n.PD 0\n.SS NONE\nthird\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("       first\n\n\nD\u{8}DO\u{8}OU\u{8}UB\u{8}BL\u{8}LE\u{8}E"),
        "{}",
        report.output
    );
    assert!(
        report
            .output
            .contains("       second\n   N\u{8}NO\u{8}ON\u{8}NE\u{8}E"),
        "{}",
        report.output
    );
}

#[test]
fn man_uri_blocks_render_text_before_the_bracketed_resource() {
    let name = SourceName::new("terminal-uri.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH URI 1 \"July 4, 2017\"\n.SH DESCRIPTION\nsee:\n.UR https://example.test/\nexample site\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("see: example site <https://example.test/>"),
        "{}",
        report.output
    );
    let mailto = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH URI 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.MT test@example.test\nmail text\n.ME tail\n.MT\nno-address\n.ME\n",
            ))
            .unwrap();
    assert!(
        mailto
            .output
            .contains("mail text <test@example.test>tail no-address <>"),
        "{}",
        mailto.output
    );
    let empty_uri = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH URI 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.UR\nlink text\n.UE\n",
        ))
        .unwrap();
    assert!(
        empty_uri.output.contains("link text <>"),
        "{}",
        empty_uri.output
    );
}

#[test]
fn man_synopsis_blocks_keep_filled_and_literal_argument_fields() {
    let name = SourceName::new("terminal-sy.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH SY 1\n.SH DESCRIPTION\nbefore\n.SY command\n.I argument\n.YS\n.nf\n.SY literal\n.I argument\n.YS\n.fi\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "       before\n\n       c\u{8}co\u{8}om\u{8}mm\u{8}ma\u{8}an\u{8}nd\u{8}d _\u{8}a_\u{8}r_\u{8}g_\u{8}u_\u{8}m_\u{8}e_\u{8}n_\u{8}t\n\n       l\u{8}li\u{8}it\u{8}te\u{8}er\u{8}ra\u{8}al\u{8}l\n               _\u{8}a_\u{8}r_\u{8}g_\u{8}u_\u{8}m_\u{8}e_\u{8}n_\u{8}t"
            ),
            "{}",
            report.output
        );
}

#[test]
fn man_rs_uses_signed_n_and_i_indentation_units() {
    let name = SourceName::new("terminal-rs.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH RS 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.RS -14n\nleft\n.RE\n.RS -0.36i\nthree\n.RE\n.RS 0.36i\neleven\n.RE\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("\nleft\n   three\n           eleven\n"),
        "{}",
        report.output
    );
}

#[test]
fn widthless_man_rs_restores_the_current_field_margin() {
    let name = SourceName::new("terminal-rs-field-margin.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH RS 1\n.SH DESCRIPTION\n.TP 2n\n\\(bu\nbullet list\n.RS\nindented text\n.RE\nregular text\n.RS\ntop-level indented list\n.RE\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "       +\u{8}o bullet list\n         indented text\n       regular text\n         top-level indented list"
            ),
            "{}",
            report.output
        );
}

#[test]
fn man_rs_truncates_unsuffixed_fractional_widths_to_terminal_cells() {
    let name = SourceName::new("terminal-rs-decimal.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH RS 1\n.SH DESCRIPTION\n.RS 0.0\nzero\n.RS 3.5\nthree\n.RE\nzero again\n.RE\nplain\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("       zero\n          three\n       zero again\n       plain"),
        "{}",
        report.output
    );
}

#[test]
fn man_stray_re_after_ip_consumes_the_field_paragraph_slot() {
    let name = SourceName::new("terminal-lonely-re.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH LONELY-RE 1\n.SH DESCRIPTION\n.IP tag 6n\nbody\n.RE\nout of body\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("       tag   body\n       out of body"),
        "{}",
        report.output
    );
    assert!(
        !report
            .output
            .contains("       tag   body\n\n       out of body"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_closing_delimiters_attach_without_source_spacing() {
    let name = SourceName::new("terminal-mdoc-delimiter.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt DELIMITER 1\n.Os\n.Sh DESCRIPTION\n.Dv value \";\"\n",
        ))
        .unwrap();
    assert!(report.output.contains("value;"), "{}", report.output);
    assert!(!report.output.contains("value ;"), "{}", report.output);
}

#[test]
fn man_subsections_and_paragraph_blocks_have_terminal_geometry() {
    let name = SourceName::new("terminal-subsection.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH SUBSECTION 1\n.SH DESCRIPTION\n.SS nested heading\nfirst paragraph\n.PP\nsecond paragraph\n",
            ))
            .unwrap();
    assert!(report
            .output
            .contains("   n\u{8}ne\u{8}es\u{8}st\u{8}te\u{8}ed\u{8}d h\u{8}he\u{8}ea\u{8}ad\u{8}di\u{8}in\u{8}ng\u{8}g\n       first paragraph\n\n       second paragraph"));
}

#[test]
fn man_pd_controls_before_empty_subsections_do_not_create_blank_lines() {
    let name = SourceName::new("terminal-ss-pd.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH SS 1 \"July 4, 2017\"\n.SH DESCRIPTION\n.PD 2v\n.SS First\n.PD 1v\n.SS Second\ntext\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "D\u{8}DE\u{8}ES\u{8}SC\u{8}CR\u{8}RI\u{8}IP\u{8}PT\u{8}TI\u{8}IO\u{8}ON\u{8}N\n   F\u{8}Fi\u{8}ir\u{8}rs\u{8}st\u{8}t\n   S\u{8}Se\u{8}ec\u{8}co\u{8}on\u{8}nd\u{8}d\n       text"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_system_names_keep_optional_versions_in_one_terminal_word() {
    let name = SourceName::new("terminal-system-version.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .with_width(20)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt SYSTEM 1\n.Os\n.Sh DESCRIPTION\none two three\n.Ox 6.1\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("     one two three\n     OpenBSD 6.1"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_word_keep_holds_a_system_macro_and_its_line_tail_together() {
    let name = SourceName::new("terminal-system-word-keep.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt SYSTEM 1\n.Os\n.Sh DESCRIPTION\nBecause we use a keep,\n.Bk -words\n.Ox 4.9 must be at the beginning of a new line.\n.Ek\n",
            ))
            .unwrap();
    assert!(
        report.output.contains(
            "     Because we use a keep,\n     OpenBSD 4.9 must be at the beginning of a new line."
        ),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_parenthetical_blocks_emit_structural_delimiters() {
    let name = SourceName::new("terminal-parenthetical.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt PARENTHETICAL 1\n.Os\n.Sh DESCRIPTION\nBefore\n.Pq nested words .\nafter\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     Before (nested words).  after"),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_quote_blocks_include_explicit_opening_delimiters() {
    let name = SourceName::new("terminal-quote-blocks.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt QUOTE-BLOCKS 1\n.Os\n.Sh DESCRIPTION\n.Dq \"(\" value)\n.Brq\n.Sq\n",
            ))
            .unwrap();
    assert!(report.output.contains("(\"value)\""), "{}", report.output);
    assert!(report.output.contains("{}"), "{}", report.output);
    assert!(report.output.contains("`'"), "{}", report.output);
}

#[test]
fn mdoc_quote_bodies_keep_nested_lists_structural() {
    let name = SourceName::new("terminal-quote-list.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt QUOTE-LIST 1\n.Os\n.Sh DESCRIPTION\n.Bo before list\n.Bl -enum -offset indent\n.It\ninside both\n.Bc\nafter bracket\n.El\nafter list\n",
            ))
            .unwrap();
    assert!(
        report.output.contains(
            "     [before list\n\n           1.   inside both] after bracket\n     after list"
        ),
        "{}",
        report.output
    );
}

#[test]
fn mdoc_quote_bodies_restore_recovered_list_breaks() {
    let name = SourceName::new("terminal-quote-list-break.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt QUOTE-LIST-BREAK 1\n.Os\n.Sh DESCRIPTION\n.Bl -enum -offset indent\n.It\nbefore bracket\n.Bo inside both\n.El\n.It\nstray item\n.Bc\nafter both\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "           1.   before bracket [inside both\n                stray item]\n     after both"
            ),
            "{}",
            report.output
        );
}

#[test]
fn mdoc_bf_body_uses_its_normalized_font_as_the_terminal_base() {
    let name = SourceName::new("terminal-bf.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt BF 1\n.Os\n.Sh DESCRIPTION\n.Bf -emphasis\nvalue\\fBbold\\fPtail\n.Ef\n",
            ))
            .unwrap();
    assert!(
            report
                .output
                .contains("_\u{8}v_\u{8}a_\u{8}l_\u{8}u_\u{8}eb\u{8}bo\u{8}ol\u{8}ld\u{8}d_\u{8}t_\u{8}a_\u{8}i_\u{8}l"),
            "{}",
            report.output
        );
}

#[test]
fn terminal_renderer_retains_adjacent_authored_and_escaped_spaces() {
    let name = SourceName::new("terminal-multiple-space.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt MULTIPLE 1\n.Os\n.Sh DESCRIPTION\ntwo spaces  here\n.Pp\ntwo escaped spaces\\ \\ here\n",
            ))
            .unwrap();
    assert!(
        report.output.contains("     two spaces  here"),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("     two escaped spaces  here"),
        "{}",
        report.output
    );
}

#[test]
fn terminal_nonbreaking_spaces_move_the_entire_phrase_to_the_next_line() {
    assert!(!TERMINAL_NONBREAKING_SPACE_MARKER.is_whitespace());
    let input = format!("     123456789012 x{TERMINAL_NONBREAKING_SPACE_MARKER}x");
    assert_eq!(
        wrap_terminal_output(&input, 20, DEFAULT_RENDER_OUTPUT_BYTES, 0, 0).unwrap(),
        "     123456789012\n     x x"
    );
}

#[test]
fn terminal_sentence_flags_survive_attached_closing_delimiters() {
    let name = SourceName::new("terminal-sentence-delimiter.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH SENTENCE 1\n.SH DESCRIPTION\nShe said: \"A sentence.\"\nAnd continued.\nA parenthesized dot (.) is not terminal punctuation.\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("She said: \"A sentence.\"  And continued."),
        "{}",
        report.output
    );
    assert!(
        report.output.contains("parenthesized dot (.) is not"),
        "{}",
        report.output
    );
}

#[test]
fn filled_leading_source_space_retains_a_terminal_line_and_column() {
    let name = SourceName::new("terminal-leading-space.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".Dd July 4, 2017\n.Dt LEADING-SPACE 1\n.Os\n.Sh DESCRIPTION\nfirst line\n leading line\nfollowing words\n",
            ))
            .unwrap();
    assert!(
        report
            .output
            .contains("     first line\n      leading line following words")
    );
}

#[test]
fn mdoc_dl_preserves_its_indentation_and_can_wrap_as_terminal_prose() {
    let name = SourceName::new("terminal-dl.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt DL 1\n.Os\n.Sh DESCRIPTION\n.Dl one-line display\n",
        ))
        .unwrap();
    assert!(report.output.contains("\n           one-line display\n"));
}

#[test]
fn mdoc_dl_uses_a_discretionary_break_only_when_the_line_overflows() {
    let name = SourceName::new("terminal-dl-break.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .with_width(20)
        .render(Source::new(
            &name,
            b".Dd July 4, 2017\n.Dt DL-BREAK 1\n.Os\n.Sh DESCRIPTION\n.Dl alpha,\\:beta\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("\n           alpha,\n           beta\n")
    );
}

#[test]
fn man_ip_separates_a_tabbed_tag_from_its_indented_body() {
    let name = SourceName::new("terminal-ip.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1\n.SH DESCRIPTION\n.IP single\ttab 3n\ntext\n.PP\n.B single\\ttab\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("\n\n       single    tab\n          text\n\n"),
        "{}",
        report.output
    );
    assert!(
        report
            .output
            .contains("       s\u{8}si\u{8}in\u{8}ng\u{8}gl\u{8}le\u{8}e    t\u{8}ta\u{8}ab\u{8}b")
    );
}

#[test]
fn man_field_after_a_recovered_section_blank_is_detected() {
    let name = SourceName::new("terminal-ip-section-blank.1").unwrap();
    let report = Parser::default()
        .parse(Source::new(
            &name,
            b".TH IP 1\n.SH DESCRIPTION\n\n.IP tag\nbody\n",
        ))
        .unwrap();
    let field = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("IP"))
        .unwrap();
    assert!(super::terminal_follows_empty_section_paragraph(field));
}

#[test]
fn man_ip_uses_the_default_tag_field_and_ignores_trailing_tag_blanks() {
    let name = SourceName::new("terminal-ip-field.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1\n.SH DESCRIPTION\n.IP tag\nbody\n.IP \"tag    \"\nbody\n.IP seseven\nbody\n",
        ))
        .unwrap();
    assert!(
        report.output.contains(
            "       tag    body\n\n       tag    body\n\n       seseven\n              body"
        ),
        "{}",
        report.output
    );
}

#[test]
fn empty_man_ip_body_does_not_leave_unused_tag_field_padding() {
    let name = SourceName::new("terminal-empty-ip.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1\n.SH DESCRIPTION\n.IP tag1 10n\n.IP tag2\nbody\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("       tag1\n\n       tag2      body")
    );
    assert!(!report.output.contains("tag1      \n"));
}

#[test]
fn man_ip_inside_rs_closes_without_an_extra_vertical_gap() {
    let name = SourceName::new("terminal-ip-in-rs.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1\n.SH DESCRIPTION\n.IP\n.RS\n.IP tag\ninside\n.RE\nafter\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("              tag    inside\n       after")
    );
    assert!(!report.output.contains("tag    inside\n\n       after"));
}

#[test]
fn man_ip_uses_only_its_tag_and_optional_scaled_width() {
    assert_eq!(super::terminal_signed_roff_en_prefix("-10n"), Some(-10));
    assert_eq!(super::terminal_signed_roff_en_prefix("-0.36i"), Some(-4));
    assert_eq!(super::terminal_signed_roff_en_prefix("1cx"), Some(4));
    assert_eq!(super::terminal_signed_roff_en_prefix("xxx"), None);

    let name = SourceName::new("terminal-ip-arguments.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1\n.SH DESCRIPTION\n.nf\n.IP tag 4n ignored\nliteral\n.fi\n",
        ))
        .unwrap();
    assert!(report.output.contains("       tag literal"));
    assert!(!report.output.contains("ignored"));
}

#[test]
fn man_pd_density_applies_to_ip_field_boundaries() {
    let name = SourceName::new("terminal-ip-density.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1\n.SH DESCRIPTION\n.PD 2v\n.IP tag\nfirst\n.IP tag\nsecond\n",
        ))
        .unwrap();
    assert!(report.output.contains("N\x08N\n       tag    first"));
    assert!(
        report
            .output
            .contains("       tag    first\n\n\n       tag    second"),
        "{}",
        report.output
    );

    let zero_density = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1\n.SH DESCRIPTION\n.PD 0\n.IP tag\nfirst\n.TP\nnext\ntext\n",
        ))
        .unwrap();
    assert!(
        zero_density
            .output
            .contains("       tag    first\n       next   text"),
        "{}",
        zero_density.output
    );
}

#[test]
fn long_man_ip_tags_wrap_without_losing_the_body_field() {
    let name = SourceName::new("terminal-long-ip-tag.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
            .render(Source::new(
                &name,
                b".TH IP 1\n.SH DESCRIPTION\n.IP \"This indented paragraph has ridiculously long text in its head, such that it doesn't even fit on the line\" 6n\nbody\n",
            ))
            .unwrap();
    assert!(
            report.output.contains(
                "       This indented paragraph has ridiculously long text in its head, such\n       that it doesn't even fit on the line\n             body"
            ),
            "{}",
            report.output
        );
}

#[test]
fn roff_center_and_right_adjust_requests_own_no_fill_input_lines() {
    let name = SourceName::new("terminal-adjusted-input.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH ADJUST 1\n.SH DESCRIPTION\nbefore\n.ce 2\ncenter\nsecond\n.rj 1\nright\nafter\n",
        ))
        .unwrap();
    assert!(
        report.output.contains(&format!(
            "       before\n{}center\n{}second\n{}right\n       after",
            " ".repeat(39),
            " ".repeat(39),
            " ".repeat(73),
        )),
        "{}",
        report.output
    );
}

#[test]
fn roff_line_length_requests_are_stateful_and_reset_to_renderer_width() {
    let name = SourceName::new("terminal-line-length.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .with_width(20)
        .render(Source::new(
            &name,
            b".ll 8n\none two three four\n.br\n.ll\none two three four\n",
        ))
        .unwrap();
    assert_eq!(report.output, "one two\nthree\nfour\none two three four\n");
}

#[test]
fn roff_indent_requests_start_new_fields_and_reset_at_a_paragraph() {
    let name = SourceName::new("terminal-indent.1").unwrap();
    let report = Renderer::new(RenderFormat::Utf8)
        .render(Source::new(
            &name,
            b".TH INDENT 1\n.SH DESCRIPTION\nbefore\n.in 4n\nafter\n.PP\nreset\n",
        ))
        .unwrap();
    assert!(
        report
            .output
            .contains("       before\n    after\n\n       reset"),
        "{}",
        report.output
    );
}

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
