use super::*;

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
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ndoublebox;\nL .\none\n.TE\n.sp 2v\nfollowing\n",
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
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:) allbox;\n||l||l||.\na:b\n_\nc:d\n_\n.TE\n",
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
                b".TH TP 1 28-Aug-2026\n.SH DESCRIPTION\n.TP 10n ignored\ntag\nbody\n.TP 8n\n.in 3n\nshifted\nbody\n",
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
            b".TH TP 1 28-Aug-2026\n.SH DESCRIPTION\n.TP not-a-width\ntag\nbody\n",
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
            b".TH TP 1 28-Aug-2026\n.SH DESCRIPTION\n.TP\n.B\n.I\nitalic term\nbody\n",
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
                b".TH TP 1 28-Aug-2026\n.SH DESCRIPTION\n.TP 6n\nshort\nbody\n.TP\n20n\nbody\n.PP\nreset\n.TP\n20n\nbody\n",
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
                b".TH TP 1 28-Aug-2026\n.SH DESCRIPTION\n.TP 4n\n*\nitem\n.RS 8n\nindented text\n.RE\nmiddle text\n.TP 4n\n*\n.RS 8n\nindented text\n.RE\ntrailing text\n",
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
            b".TH HP 1 28-Aug-2026\n.SH DESCRIPTION\n.RS\nouter text\n.HP 2n\n.RS 4n\ninner text\n.RE\n.RE\n",
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
                b".TH TP 1 28-Aug-2026\n.SH DESCRIPTION\n.PD 2v\n.TP\nfirst-tag\ntext\n.TP\nsecond-tag\ntext\n.TP 6n\nThis tagged paragraph has ridiculously long text in its head\nbody\n",
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
                b".TH TP 1 28-Aug-2026\n.SH DESCRIPTION\n.TP\ntag\\ \\&\nfirst body\n.TP\ntag\\ \\ \\ \\ \\ \\&\nsecond body\n",
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
                b".TH TP 1 28-Aug-2026\n.SH DESCRIPTION\n.TP 12n\ntag\nfirst second third fourth fifth sixth\n.TP 100n\nwide\nbody\n",
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
                b".TH FONT 1 28-Aug-2026\n.SH DESCRIPTION\nEarlier sentence.\nIt works with\n.B several words\nand with\n.B\nnext line\nscope.\n",
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
            b".TH FONT 1 28-Aug-2026\n.SH DESCRIPTION\n.BI bold italic bold again\n.IR italic roman\n",
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
            b".TH FONT 1 28-Aug-2026\n.SH DESCRIPTION\nempty\n.OP\nvalue\n.OP -f arg excess\n",
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
