use super::*;

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
            b".TH TERMINAL-SPACING 1 28-Aug-2026\n.SH DESCRIPTION\nfirst paragraph\n.sp\nsecond paragraph\n",
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
            b".TH TERMINAL-ADJACENT-SP 1 28-Aug-2026\n.SH DESCRIPTION\nbefore\n.sp\n.sp\nafter\n",
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
                b".TH TERMINAL-NEGATIVE-SPACING 1 28-Aug-2026\n.SH DESCRIPTION\nfirst line\n.sp -1v\n.PP\nsecond line\n",
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
                b".TH TERMINAL-FONT-REQUESTS 1 28-Aug-2026\n.SH DESCRIPTION\nplain\n.ft I\nitalic\n.ft B\nbold\n.ft P\nitalic-again\n.ft\nbold-again\n.ft R\nroman\n",
            ))
            .unwrap();
    let expected = format!(
        "       plain {} {} {} {} roman",
        crate::renderer::render_terminal_font("italic", crate::renderer::TerminalFont::Italic),
        crate::renderer::render_terminal_font("bold", crate::renderer::TerminalFont::Bold),
        crate::renderer::render_terminal_font(
            "italic-again",
            crate::renderer::TerminalFont::Italic
        ),
        crate::renderer::render_terminal_font("bold-again", crate::renderer::TerminalFont::Bold),
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
            crate::renderer::terminal_vertical_span(source),
            Some(expected),
            "{source}"
        );
    }
    assert_eq!(crate::renderer::terminal_vertical_span("1cx"), Some(2));
    assert_eq!(crate::renderer::terminal_vertical_span("xxx"), None);
}

#[test]
fn terminal_temporary_indentation_tracks_relative_and_wide_fields() {
    assert_eq!(
        crate::renderer::terminal_temporary_indent_target("10n", 7),
        Some(10)
    );
    assert_eq!(
        crate::renderer::terminal_temporary_indent_target("+10n", 7),
        Some(17)
    );
    assert_eq!(
        crate::renderer::terminal_temporary_indent_target("-10n", 7),
        Some(0)
    );
    assert_eq!(
        crate::renderer::terminal_temporary_indent_target("80n", 7),
        Some(72)
    );
    assert_eq!(
        crate::renderer::terminal_temporary_indent_target("+4n", 73),
        Some(73)
    );
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
            b".TH TERMINAL-NO-FILL 1 28-Aug-2026\n.SH DESCRIPTION\n.nf\none two three four five six   \n.fi\n",
        ))
        .unwrap();
    assert!(report.output.contains("       one two three four five six"));
    assert!(!report.output.contains("six   \n"));

    let example = Renderer::new(RenderFormat::Utf8)
            .render(Source::new(
                &name,
                b".TH TERMINAL-NO-FILL 1 28-Aug-2026\n.SH DESCRIPTION\nregular\n.EX ignored\nliteral\n.EE ignored\nagain\n",
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
            b".TH TERMINAL-TABS 1 28-Aug-2026\n.SH DESCRIPTION\nsingle\ttab\n.br\ndouble\t\ttab\n",
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
            b".TH CONDITION 1 28-Aug-2026\n.SH DESCRIPTION\n.nr name 0\nlabel:\n.ie rname\tvalue\n",
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
            b".TH SENTENCE-SPACING 1 28-Aug-2026\n.SH DESCRIPTION\nFirst sentence.\nSecond sentence.\n",
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
                b".TH HYPHEN 1 28-Aug-2026\n.SH DESCRIPTION\nA line whose final break-here word crosses the margin.\n",
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
