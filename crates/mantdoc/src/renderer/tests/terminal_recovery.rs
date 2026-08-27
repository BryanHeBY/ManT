use super::*;

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
                b".TH SH-NOARG 1 28-Aug-2026\n.SH DESCRIPTION\nfirst\n.SH\n.nf\nsecond\n.SH\n.fi\nthird\n.SH\n.TP 6n\ntag\ntagged text\n",
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
            b".TH LEADING-SP 1 28-Aug-2026\n.SH DESCRIPTION\n.sp\n.PP\ntext\n",
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
                b".TH PD 1 28-Aug-2026\n.SH DESCRIPTION\nfirst\n.PD 2\n.SH DOUBLE\nsecond\n.PD 0\n.SS NONE\nthird\n",
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
                b".TH SY 1 28-Aug-2026\n.SH DESCRIPTION\nbefore\n.SY command\n.I argument\n.YS\n.nf\n.SY literal\n.I argument\n.YS\n.fi\n",
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
                b".TH RS 1 28-Aug-2026\n.SH DESCRIPTION\n.TP 2n\n\\(bu\nbullet list\n.RS\nindented text\n.RE\nregular text\n.RS\ntop-level indented list\n.RE\n",
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
                b".TH RS 1 28-Aug-2026\n.SH DESCRIPTION\n.RS 0.0\nzero\n.RS 3.5\nthree\n.RE\nzero again\n.RE\nplain\n",
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
            b".TH LONELY-RE 1 28-Aug-2026\n.SH DESCRIPTION\n.IP tag 6n\nbody\n.RE\nout of body\n",
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
                b".TH SUBSECTION 1 28-Aug-2026\n.SH DESCRIPTION\n.SS nested heading\nfirst paragraph\n.PP\nsecond paragraph\n",
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
                b".TH SENTENCE 1 28-Aug-2026\n.SH DESCRIPTION\nShe said: \"A sentence.\"\nAnd continued.\nA parenthesized dot (.) is not terminal punctuation.\n",
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
            b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n.IP single\ttab 3n\ntext\n.PP\n.B single\\ttab\n",
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
            b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n\n.IP tag\nbody\n",
        ))
        .unwrap();
    let field = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("IP"))
        .unwrap();
    assert!(crate::renderer::terminal_follows_empty_section_paragraph(
        field
    ));
}

#[test]
fn man_ip_uses_the_default_tag_field_and_ignores_trailing_tag_blanks() {
    let name = SourceName::new("terminal-ip-field.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n.IP tag\nbody\n.IP \"tag    \"\nbody\n.IP seseven\nbody\n",
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
            b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n.IP tag1 10n\n.IP tag2\nbody\n",
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
            b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n.IP\n.RS\n.IP tag\ninside\n.RE\nafter\n",
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
    assert_eq!(
        crate::renderer::terminal_signed_roff_en_prefix("-10n"),
        Some(-10)
    );
    assert_eq!(
        crate::renderer::terminal_signed_roff_en_prefix("-0.36i"),
        Some(-4)
    );
    assert_eq!(
        crate::renderer::terminal_signed_roff_en_prefix("1cx"),
        Some(4)
    );
    assert_eq!(crate::renderer::terminal_signed_roff_en_prefix("xxx"), None);

    let name = SourceName::new("terminal-ip-arguments.1").unwrap();
    let report = Renderer::new(RenderFormat::Ascii)
        .render(Source::new(
            &name,
            b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n.nf\n.IP tag 4n ignored\nliteral\n.fi\n",
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
            b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n.PD 2v\n.IP tag\nfirst\n.IP tag\nsecond\n",
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
            b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n.PD 0\n.IP tag\nfirst\n.TP\nnext\ntext\n",
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
                b".TH IP 1 28-Aug-2026\n.SH DESCRIPTION\n.IP \"This indented paragraph has ridiculously long text in its head, such that it doesn't even fit on the line\" 6n\nbody\n",
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
            b".TH ADJUST 1 28-Aug-2026\n.SH DESCRIPTION\nbefore\n.ce 2\ncenter\nsecond\n.rj 1\nright\nafter\n",
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
            b".TH INDENT 1 28-Aug-2026\n.SH DESCRIPTION\nbefore\n.in 4n\nafter\n.PP\nreset\n",
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
