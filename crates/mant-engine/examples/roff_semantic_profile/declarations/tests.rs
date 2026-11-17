use super::{
    profile,
    source::{literal_numeric_label, native_man_search_template},
    violations,
};
use libmandoc_rs::{Node, NodeKind};
use mant_ir::Block;
use serde_json::json;

#[test]
fn numeric_boundary_requires_independent_complete_native_evidence() {
    for (input, expected) in [
        ("422", Some("422")),
        (r"\fB422\fR", Some("422")),
        (r"\fB4\fI22\fP", Some("422")),
        ("foo", None),
        ("4 22", None),
        (r"\&422", None),
        (r"\n[digits]", None),
        (r"\f[unknown]422", None),
        (r"422\f", None),
        (r"\fB\fR", None),
    ] {
        assert_eq!(literal_numeric_label(input).as_deref(), expected, "{input}");
    }

    let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fBfoo\\fR 4\n.PD 0\n.IP \\fBhigh\\fR 4\n.IP \\fBss\\fR 4\n.PD\nSpatially Scalable\n";
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("probe.1", source)
        .unwrap();
    let mut document =
        mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    assert!(violations(&profile(&native.document.root, &document)).is_empty());
    let Block::DefinitionList {
        items,
        declaration_groups,
        ..
    } = &mut document.sections[0].blocks[0]
    else {
        panic!("definition list")
    };
    assert_eq!(declaration_groups[0].start_item, 0);
    assert_eq!(declaration_groups[0].end_item, 3);
    // Coupled corruption must not turn a real source declaration into a
    // numeric exception and then bless the shortened observed group.
    items[0].terms = vec![
        vec![mant_ir::Inline::Text {
            value: "422".into(),
        }]
        .into(),
    ];
    items[0].entry.as_mut().unwrap().names.clear();
    declaration_groups[0].start_item = 1;
    let changed = profile(&native.document.root, &document);
    assert!(!violations(&changed).is_empty(), "{changed}");
    assert_eq!(changed["unexpectedGroups"].as_array().unwrap().len(), 1);
}

#[test]
fn unnamed_source_head_does_not_invalidate_or_hide_a_named_suffix_group() {
    // Reduced from ffmpeg-codecs(1)'s 422/high/ss profile list. A numeric
    // label has no exact discovered name; high/ss still form a contiguous
    // named source run ending at the independently authored description.
    let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fB422\\fR 4\n.PD 0\n.IP \\fBhigh\\fR 4\n.IP \\fBss\\fR 4\n.PD\nSpatially Scalable\n";
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("probe.1", source)
        .unwrap();
    let document =
        mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    let valid = profile(&native.document.root, &document);
    assert_eq!(valid["observedGroups"].as_array().unwrap().len(), 1);
    assert!(violations(&valid).is_empty(), "{valid}");
    assert!(valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
        row["reason"] == "unsigned-numeric-head" && row["physicalSources"] == json!([[3, 2, 0]])
    }));
    // Pinned roff.c::roff_name and the pristine tree identify these owners
    // as IP. Profiler JSON stores names without native serde being enabled.
    for row in valid["sourceRuns"].as_array().unwrap() {
        for owner in row["sourceOwners"].as_array().unwrap() {
            assert_eq!(owner["macro"], "IP");
        }
    }

    for mutation in ["delete-group", "cross-unnamed-owner", "move-source-owner"] {
        let mut changed = document.clone();
        let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = &mut changed.sections[0].blocks[0]
        else {
            panic!("definition list")
        };
        match mutation {
            "delete-group" => declaration_groups.clear(),
            "cross-unnamed-owner" => declaration_groups[0].start_item = 0,
            "move-source-owner" => {
                let source = items[1].source;
                items[1].source = items[0].source;
                items[0].source = source;
            }
            _ => unreachable!(),
        }
        let observed = profile(&native.document.root, &changed);
        if mutation == "delete-group" {
            assert!(violations(&observed).is_empty(), "{mutation}: {observed}");
            assert!(
                observed["sourceRuns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| { row["reason"] == "named-source-run-without-group" })
            );
        } else {
            assert!(!violations(&observed).is_empty(), "{mutation}: {observed}");
        }
    }
}

#[test]
fn signed_option_shaped_head_remains_in_the_source_run() {
    let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fBseq_disp_ext\\fR 4\nEncoder configuration.\n.RS 4\n.IP \\fB\\-1\\fR 4\n.PD 0\n.IP \\fBauto\\fR 4\n.PD\nDecide automatically.\n.RE\n";
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("probe.1", source)
        .unwrap();
    let document =
        mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    let valid = profile(&native.document.root, &document);
    assert_eq!(valid["observedGroups"].as_array().unwrap().len(), 1);
    assert!(violations(&valid).is_empty(), "{valid}");
}

#[test]
fn semantic_subrun_can_follow_unaddressable_mdoc_templates() {
    // Pinned mdoc_term.c::termp_pf_post/termp_ns_pre suppress spacing
    // around the search prefix. mdoc_html.c::mdoc_ar_pre still renders
    // a distinct Ar operand. The plain Cm control paints the same ?RE.
    let prefix = ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Xo\n.Pf / Ns Ar RE\n.Xc\n.It Xo\n.Pf / Ns Ar RE Ns /\n.Xc\n.It Xo\n";
    let probe_path = std::path::Path::new("probe.1");
    for (reverse_search, label, literal, append_repeat, group_start) in [
        (".Pf ? Ns Ar RE", "?RE", None, false, None),
        (".Pf ? Ns Ar RE", "?RE", None, true, Some(3)),
        (".Pf ? Ar RE", "?RE", None, true, Some(3)),
        (".No / Ns Ar RE", "/RE", None, true, Some(3)),
        (".No ? Ns Ar RE", "?RE", None, true, Some(3)),
        (".No ? Ns Ar RE Ns ?", "?RE?", None, true, Some(3)),
        (r".No \fB/\fR Ns Ar RE", "/RE", None, true, Some(3)),
        (r".Pf \&/ Ns Ar RE", "/RE", None, true, Some(3)),
        (r".Pf \^/ Ns Ar RE", "/RE", None, true, Some(3)),
        (r".Pf \%/ Ns Ar RE", "/RE", None, true, Some(3)),
        (
            ".Pf / Ns Ar RE Ns / Ar offset",
            "/RE/ offset",
            None,
            true,
            Some(3),
        ),
        (r".No \f[B]?\fP Ns Ar RE", "?RE", None, true, Some(3)),
        (".Pf ? Ns Ar RE Ar flags", "?RE flags", None, true, Some(3)),
        (".No ? Ar RE", "? RE", None, true, None),
        (
            r#".Ar "\fBfind\fR operand""#,
            "find operand",
            None,
            true,
            None,
        ),
        (r#".Ar "find operand""#, "find operand", None, true, None),
        (".Cm ?RE", "?RE", Some("?RE"), true, Some(2)),
        (".Cm ?RE Ar KEY", "?RE KEY", Some("?RE"), true, Some(2)),
        (
            ".Cm /usr/bin/search Ar RE",
            "/usr/bin/search RE",
            None,
            true,
            None,
        ),
    ] {
        let repeat = if append_repeat { ".It Cm N\n" } else { "" };
        let source = format!("{prefix}{reverse_search}\n.Xc\n.It Cm n\n{repeat}Search.\n.El\n");
        let native = libmandoc_rs::Parser::default()
            .parse_bytes(probe_path, source.as_bytes())
            .unwrap();
        let mut document = mant_loader::parse_manual_bytes(probe_path, source.as_bytes()).unwrap();
        let analyzed = profile(&native.document.root, &document);
        let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = &mut document.sections[0].blocks[0]
        else {
            panic!("search declaration list")
        };
        let mut expected_labels = vec!["/RE", "/RE/", label, "n"];
        if append_repeat {
            expected_labels.push("N");
        }
        let mut physical_sources = vec![[6, 2, 0], [9, 2, 0], [12, 2, 0], [15, 2, 0]];
        if append_repeat {
            physical_sources.push([16, 2, 0]);
        }
        assert_eq!(items.len(), expected_labels.len(), "{source}");
        for (index, (item, label)) in items.iter().zip(expected_labels).enumerate() {
            assert_eq!(super::super::inline_text(&item.terms[0]), label, "{source}");
            let name = match index {
                2 => literal,
                3.. => Some(label),
                _ => None,
            };
            assert_eq!(
                &*item.entry.as_ref().unwrap().names,
                name.as_slice(),
                "{source}"
            );
        }
        assert!(violations(&analyzed).is_empty(), "{analyzed}");
        let runs = analyzed["sourceRuns"].as_array().unwrap();
        let run = runs
            .iter()
            .find(|row| row["physicalSources"] == json!(physical_sources))
            .unwrap_or_else(|| panic!("{analyzed}"));
        if let Some(start_item) = group_start {
            assert_eq!(run["reason"], "source-run-semantic-subset");
            assert_eq!(run["observedGroup"], 0);
            assert_eq!(
                declaration_groups.as_slice(),
                [mant_ir::DeclarationGroup {
                    start_item,
                    end_item: items.len(),
                }]
            );
            if literal.is_some() {
                declaration_groups[0].start_item += 1;
            } else {
                // The source template can be omitted from the group
                // only while its original IR owner remains readable.
                items.remove(0);
                declaration_groups[0].start_item -= 1;
                declaration_groups[0].end_item -= 1;
            }
            let changed = profile(&native.document.root, &document);
            assert!(!violations(&changed).is_empty(), "{changed}");
        } else {
            assert_eq!(run["reason"], "named-source-run-without-group");
            assert!(run["observedGroup"].is_null());
            assert!(declaration_groups.is_empty());
        }
    }
}

#[test]
fn direct_mdoc_search_templates_preserve_the_following_command_group() {
    // CVS places the literal sign, Ns and Ar directly under It HEAD
    // in both directions, with no Xo wrapper.
    let prefix = ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n";
    let probe_path = std::path::Path::new("probe.1");
    for sign in ['/', '?'] {
        let source = format!("{prefix}.It {sign} Ns Ar RE\n.It Cm n\n.It Cm N\nSearch.\n.El\n");
        let native = libmandoc_rs::Parser::default()
            .parse_bytes(probe_path, source.as_bytes())
            .unwrap();
        let document = mant_loader::parse_manual_bytes(probe_path, source.as_bytes()).unwrap();
        let audited = profile(&native.document.root, &document);
        assert!(violations(&audited).is_empty(), "{audited}");
        assert_eq!(audited["observedGroups"], json!([[[7, 2, 0], [8, 2, 0]]]));
        assert!(audited["sourceRuns"].as_array().unwrap().iter().any(|row| {
            row["reason"] == "source-run-semantic-subset"
                && row["physicalSources"] == json!([[6, 2, 0], [7, 2, 0], [8, 2, 0]])
                && row["observedGroup"] == 0
        }));
    }
}

#[test]
fn executed_search_template_runs_need_the_original_operator_and_argument() {
    let prefix = ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n.Sm off\n.Sh COMMANDS\n.Bl -tag -width Ds\n";
    for (head, grouped) in [
        (".It No ? Ar RE", true),
        (".It Xo\n.Sm off\n.Xo\n.Pf ? Ar RE\n.Xc\n.Xc", true),
        (r#".It No "\fR[argument]""#, true),
        (r#".It No "\&[argument]""#, true),
        (r#".It No "\(lBargument\(rB""#, true),
        (r#".It No "\[lB]argument\[rB]""#, true),
        (r#".It No "\[u005B]argument\[u005D]""#, true),
        (r#".It No "\N'91'argument\N'93'""#, true),
        (".It Xo\n.Pf \\z? Bq Ar RE\n.Xc", false),
        (r".It No ? Ar \& Ar RE", false),
        (r".It No ? Ar \zR Ar E", false),
        (r".It No ? Ar x?y Ns \z? Ar OFFSET", false),
    ] {
        // Check the source roles behind the pinned spellings. In
        // particular, a generated bracket or retained parameter can
        // remain after the search sign was overstruck (term.c::encode1()).
        let source = format!("{prefix}{head}\n.It Cm n\n.It Cm N\nGROUP_BODY\n.El\n");
        let path = std::path::Path::new("probe.3");
        let native = libmandoc_rs::Parser::default()
            .parse_bytes(path, source.as_bytes())
            .unwrap();
        let mut document = mant_loader::parse_manual_bytes(path, source.as_bytes()).unwrap();
        let audited = profile(&native.document.root, &document);
        assert!(violations(&audited).is_empty(), "{audited}");
        let Block::DefinitionList {
            declaration_groups, ..
        } = &mut document.sections[1].blocks[0]
        else {
            panic!("expected command list");
        };
        assert_eq!(!declaration_groups.is_empty(), grouped, "{head}");
        if !grouped {
            declaration_groups.push(mant_ir::DeclarationGroup {
                start_item: 1,
                end_item: 3,
            });
            let mutated = profile(&native.document.root, &document);
            assert!(!violations(&mutated).is_empty(), "{head}: {mutated}");
        }
    }
}

#[test]
fn styled_search_group_audit_is_independent_of_injected_names() {
    let prefix = ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n.Sh COMMANDS\n.Sm off\n.Bl -tag -width Ds\n";
    for (head, grouped) in [
        (r".It Sy / Ar RE", true),
        (r".It Em \[u003F] Ar RE", true),
        (r".It Pf / Ar \(eu", true),
        (r".It Pf / Ar RE Pf \(sl Ar OFFSET", true),
        (r".It Pf \z\c\(sl Ar /RE", true),
        (r".It Pf \Ez/ Ar //RE", false),
        (r".It Pf \z/\c Ar /RE", false),
    ] {
        let source = format!("{prefix}{head}\n.It Cm n\n.It Cm N\nGROUP_BODY\n.El\n");
        let path = std::path::Path::new("probe.3");
        let native = libmandoc_rs::Parser::default()
            .parse_bytes(path, source.as_bytes())
            .unwrap();
        let mut document = mant_loader::parse_manual_bytes(path, source.as_bytes()).unwrap();
        let first = profile(&native.document.root, &document);
        assert!(violations(&first).is_empty(), "{head}: {first}");
        let Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = &mut document.sections[1].blocks[0]
        else {
            panic!("expected command list");
        };
        if grouped {
            assert_eq!(declaration_groups[0].start_item, 1, "{head}");
            declaration_groups[0].start_item = 0;
            items[0].entry.as_mut().unwrap().names.push("RE".into());
        } else {
            assert!(declaration_groups.is_empty(), "{head}");
            declaration_groups.push(mant_ir::DeclarationGroup {
                start_item: 1,
                end_item: 3,
            });
        }
        let mutated = profile(&native.document.root, &document);
        assert!(!violations(&mutated).is_empty(), "{head}: {mutated}");
    }
}

#[test]
fn semantic_subrun_can_follow_source_proven_man_option_templates() {
    // Reduced from strings(1). The legacy `-min-len` heading is a
    // formatter-visible synopsis template, not an addressable option;
    // `-n` and `--bytes` are the independent names sharing its body.
    let source = b".TH PROBE 1\n.SH OPTIONS\n.IP \\fB\\-\\fR\\fImin\\-len\\fR 4\n.PD 0\n.IP \"\\fB\\-n\\fR \\fImin\\-len\\fR\" 4\n.IP \\fB\\-\\-bytes=\\fR\\fImin\\-len\\fR 4\n.PD\nDisplay strings.\n";
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("probe.1", source)
        .unwrap();
    let document =
        mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    let valid = profile(&native.document.root, &document);
    assert!(violations(&valid).is_empty(), "{valid}");
    assert!(valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
        row["reason"] == "source-run-semantic-subset"
            && row["physicalSources"] == json!([[3, 2, 0], [5, 2, 0], [6, 2, 0]])
    }));

    let mut corrupted = document;
    let Block::DefinitionList {
        declaration_groups, ..
    } = &mut corrupted.sections[0].blocks[0]
    else {
        panic!("definition list")
    };
    declaration_groups.clear();
    let ungrouped = profile(&native.document.root, &corrupted);
    assert!(violations(&ungrouped).is_empty(), "{ungrouped}");
    assert!(
        ungrouped["sourceRuns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| { row["reason"] == "named-source-run-without-group" })
    );
}

#[test]
fn semantic_subrun_can_follow_source_proven_man_search_templates() {
    // Reduced from the historic NetBSD ex(1) source. The RE form is a
    // visible edit-language template; N and n are the selectable commands
    // carrying its shared description.
    let source = b".TH PROBE 1\n.SH COMMANDS\n.TP\n.B \"?RE? [offset]<carriage-return>\"\n.TP\n.B N\n.TP\n.B n\nSearch forward or backward.\n";
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("probe.1", source)
        .unwrap();
    fn find_line(node: &Node, line: u32) -> Option<&Node> {
        (node.kind == NodeKind::Block && node.line == line)
            .then_some(node)
            .or_else(|| {
                node.children
                    .iter()
                    .find_map(|child| find_line(child, line))
            })
    }
    assert!(native_man_search_template(
        find_line(&native.document.root, 3).expect("search template")
    ));
    let document =
        mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    let valid = profile(&native.document.root, &document);
    assert!(violations(&valid).is_empty(), "{valid}");
    assert!(
        valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
            row["reason"] == "source-run-retained"
                && row["physicalSources"] == json!([[3, 2, 0], [5, 2, 0], [7, 2, 0]])
        }),
        "{valid}"
    );
}

#[test]
fn plain_title_taxonomies_remain_ungrouped_census_evidence() {
    // Reduced from perltoc(1). Plain title-cased labels under a generic
    // heading are an index/taxonomy, not declarations sharing the final
    // item's prose. They remain visible individual terms, but must never
    // manufacture explanation context by adjacency alone.
    let source = b".TH PROBE 1\n.SH CONTENTS\n.IP Solution 4\n.PD 0\n.IP \"The Rest\" 4\n.IP Summary 4\n.IP Credits 4\n.PD\nContents.\n";
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("probe.1", source)
        .unwrap();
    let document =
        mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    let valid = profile(&native.document.root, &document);
    assert!(violations(&valid).is_empty(), "{valid}");
    assert!(valid["observedGroups"].as_array().unwrap().is_empty());
    assert!(valid["sourceRuns"].as_array().unwrap().iter().any(|row| {
        row["reason"] == "named-source-run-without-group"
            && row["physicalSources"] == json!([[3, 2, 0], [5, 2, 0], [6, 2, 0], [7, 2, 0]])
    }));
}

#[test]
fn headless_ip_layout_arguments_and_non_definition_predecessors_preserve_obligations() {
    for prefix in [
        ".TP\n.B -c\n.TP\n.B -d\nFirst body.\n.IP \"\" 4\nTail.\n",
        ".IP \\(bu\nBullet body.\n.IP\nTail.\n",
    ] {
        let source = format!(
            ".TH PROBE 1\n.SH OPTIONS\n.de PAIR\n{prefix}.TP\n.B -a\n.TP\n.B -b\nSecond body.\n..\n.PAIR\n"
        );
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source.as_bytes())
            .unwrap();
        let mut document =
            mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source.as_bytes())
                .unwrap();
        let original = profile(&native.document.root, &document);
        assert!(violations(&original).is_empty(), "{source}\n{original}");
        for block in &mut document.sections[0].blocks {
            if let Block::DefinitionList {
                declaration_groups, ..
            } = block
            {
                declaration_groups.clear();
            }
        }
        let removed = profile(&native.document.root, &document);
        assert!(
            violations(&removed).is_empty(),
            "ungrouped source run is a census fact, not a false failure: {source}\n{removed}"
        );
        assert!(
            removed["sourceRuns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| { row["reason"] == "named-source-run-without-group" })
        );
    }
}
#[test]
fn same_coordinate_stream_accounts_for_explicit_heads_and_list_conversion() {
    for prefix in ["", ".IP 1.\nA numbered step.\n", ".IP \\(bu\nA bullet.\n"] {
        for continuation in ["", ".TQ\n.B --all\n"] {
            let source = format!(
                ".TH PROBE 1\n.SH OPTIONS\n.de PAIR\n{prefix}.TP\n.B -a\n{continuation}.TP\n.B -b\nBody.\n.IP\nTail.\n..\n.PAIR\n.PAIR\n"
            );
            let native = libmandoc_rs::Parser::default()
                .parse_bytes("probe.1", source.as_bytes())
                .unwrap();
            let document =
                mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source.as_bytes())
                    .unwrap();
            let result = profile(&native.document.root, &document);
            assert_eq!(
                result["observedGroups"].as_array().unwrap().len(),
                2,
                "{source}\n{result}"
            );
            assert!(violations(&result).is_empty(), "{source}\n{result}");
        }
    }
}
#[test]
fn macro_expansion_retains_every_same_coordinate_owner_occurrence() {
    let source =
        b".TH PROBE 1\n.SH OPTIONS\n.de PAIR\n.TP\n.B -a\n.TP\n.B -b\nBody.\n..\n.PAIR\n.PAIR\n";
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("probe.1", source)
        .unwrap();
    let mut document =
        mant_loader::parse_manual_bytes(std::path::Path::new("probe.1"), source).unwrap();
    let original = profile(&native.document.root, &document);
    assert_eq!(original["observedGroups"].as_array().unwrap().len(), 2);
    assert!(violations(&original).is_empty(), "{original}");
    let Block::DefinitionList {
        declaration_groups, ..
    } = &mut document.sections[0].blocks[0]
    else {
        panic!("definitions")
    };
    declaration_groups.remove(0);
    let ungrouped = profile(&native.document.root, &document);
    assert!(violations(&ungrouped).is_empty(), "{ungrouped}");
    assert!(
        ungrouped["sourceRuns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| { row["reason"] == "named-source-run-without-group" })
    );
}
#[test]
fn parameter_continuation_separates_runs_without_hiding_crossing_groups() {
    let source = b".TH PROBE 1\n.SH COMMANDS\n.TP\n.B first\n.TP\n\\fB      \\fP[ \\fIargument\\fP ]\n.TP\n.B second\n.TP\n.B third\nBody.\n";
    let parsed = libmandoc_rs::Parser::new(Default::default())
        .parse_bytes("probe.1", source)
        .unwrap();
    let mut document = mant_loader::load_roff_bytes(source)
        .unwrap()
        .document
        .unwrap();
    let valid = profile(&parsed.document.root, &document);
    assert_eq!(valid["unexpectedGroups"], json!([]));
    assert!(
        valid["sourceRuns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["reason"] == "parameter-only-head")
    );
    let Block::DefinitionList {
        declaration_groups, ..
    } = &mut document.sections[0].blocks[0]
    else {
        panic!()
    };
    declaration_groups[0].start_item = 0;
    assert_eq!(
        profile(&parsed.document.root, &document)["unexpectedGroups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn accounting_binds_actual_groups_and_records_ungrouped_runs() {
    let source = b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.TP\n.B --second\nBody.\n";
    let parsed = libmandoc_rs::Parser::new(Default::default())
        .parse_bytes("probe.1", source)
        .unwrap();
    let mut document = mant_loader::load_roff_bytes(source)
        .unwrap()
        .document
        .unwrap();
    let valid = profile(&parsed.document.root, &document);
    assert_eq!(valid["sourceRuns"][0]["status"], "retained");
    assert_eq!(valid["unexpectedGroups"], json!([]));
    let Block::DefinitionList {
        declaration_groups, ..
    } = &mut document.sections[0].blocks[0]
    else {
        panic!()
    };
    declaration_groups.clear();
    let ungrouped = profile(&parsed.document.root, &document);
    assert!(violations(&ungrouped).is_empty(), "{ungrouped}");
    assert!(
        ungrouped["sourceRuns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| { row["reason"] == "named-source-run-without-group" })
    );
    let Block::DefinitionList {
        declaration_groups, ..
    } = &mut document.sections[0].blocks[0]
    else {
        panic!()
    };
    declaration_groups.push(mant_ir::DeclarationGroup {
        start_item: 0,
        end_item: 2,
    });
    let mut wrong = parsed.document.root.clone();
    fn relocate(node: &mut Node) {
        node.line += 100;
        for child in &mut node.children {
            relocate(child);
        }
    }
    relocate(&mut wrong);
    assert_eq!(
        profile(&wrong, &document)["unexpectedGroups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
