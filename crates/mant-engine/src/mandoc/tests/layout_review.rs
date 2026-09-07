//! End-to-end name, ownership and layout regression matrices.

#[test]
fn invocation_forms_and_aliases_agree_across_query_consumers() {
    use mant_protocol::{
        EntryProjection, OutlineNode, SearchCase, SearchQuery, SearchScope, SearchSyntax,
    };
    for (section, head, form, aliases, rejected) in [
        (
            "OPTIONS",
            ".BI \"-n, \" -NUM",
            "-n, -NUM",
            vec!["-n"],
            "-NUM",
        ),
        (
            "OPTIONS",
            ".BI \"-n| \" -NUM",
            "-n| -NUM",
            vec!["-n"],
            "-NUM",
        ),
        (
            "OPTIONS",
            ".B \"-n, \\fI-NUM\"",
            "-n, -NUM",
            vec!["-n"],
            "-NUM",
        ),
        (
            "OPTIONS",
            ".B \"-h/--help\"",
            "-h/--help",
            vec!["-h", "--help"],
            "--missing",
        ),
        (
            "OPTIONS",
            ".BI \"-q \" -COUNT",
            "-q -COUNT",
            vec!["-q"],
            "-COUNT",
        ),
        (
            "COMMANDS",
            ".B \"create, duplicate\"",
            "create, duplicate",
            vec!["create", "duplicate"],
            "create, duplicate",
        ),
        (
            "ENVIRONMENT",
            ".B MODE=red,blue",
            "MODE=red,blue",
            vec!["MODE"],
            "blue",
        ),
    ] {
        let query = crate::query_roff_bytes(
            format!(".TH NAMES 1\n.SH {section}\n.TP\n{head}\nOWNEDPAYLOAD.\n").as_bytes(),
        )
        .unwrap();
        let index = mant_ir::SemanticIndex::build(query.document.as_ref().unwrap());
        let indexed = &index.section(&query.document.as_ref().unwrap().sections[0].id)[0];
        assert_eq!(indexed.aliases, aliases);
        assert_eq!(indexed.forms, [form]);
        let outline = crate::build_outline_projection(&query, EntryProjection::All, None).unwrap();
        let OutlineNode::DocumentSection { children, .. } = &outline.nodes[0] else {
            panic!("{outline:?}")
        };
        let OutlineNode::DocumentEntry {
            id,
            path,
            aliases: projected,
            forms,
            ..
        } = &children[0]
        else {
            panic!("{children:?}")
        };
        assert_eq!(projected, &aliases);
        assert_eq!(forms, &[form]);
        let direct = crate::select_excerpt(&query, &[path.as_str()]).unwrap();
        for alias in aliases {
            let explained = crate::select_explanation(&query, alias).unwrap();
            assert_eq!(explained.selections, direct.selections);
            assert!(crate::render_excerpt_text(&explained).contains("OWNEDPAYLOAD"));
        }
        assert!(crate::select_explanation(&query, rejected).is_err());
        for scope in [SearchScope::Visible, SearchScope::Markdown] {
            let found = crate::search_query(
                &query,
                &SearchQuery {
                    pattern: "OWNEDPAYLOAD".into(),
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Sensitive,
                    scope,
                    word: false,
                    context_lines: 0,
                    offset: 0,
                    limit: 10,
                },
            )
            .unwrap();
            assert_eq!(found.matches.len(), 1);
            let hit = &found.matches[0];
            assert!(
                matches!(&hit.outline.node, mant_protocol::OutlineNodeReference::DocumentEntry { id: found_id, path: found_path, .. } if found_id == id && found_path == path)
            );
            assert_eq!(hit.node_source.unwrap().line, 3);
            assert_eq!(hit.occurrences[0].matched_text, "OWNEDPAYLOAD");
        }
    }
}

#[test]
fn incomplete_tag_paragraphs_preserve_visible_terms_at_eof() {
    for tail in [
        ".B --unfinished",
        ".BI \"--unfinished \" VALUE",
        ".B --first\n.TP\n.B --unfinished",
    ] {
        let query =
            crate::query_roff_bytes(format!(".TH TAGS 1\n.SH OPTIONS\n.TP\n{tail}\n").as_bytes())
                .unwrap();
        for text in [
            crate::render_query_text(&query),
            crate::render_markdown(&query),
        ] {
            assert!(text.contains("--unfinished"), "{tail}: {text}");
            if tail.contains("--first") {
                assert!(text.contains("--first"), "{text}");
            }
        }
    }
}

#[test]
fn variable_subscripts_must_be_complete_authored_forms() {
    for name in ["FOO", "FOO[bar]", "FOO[0]", "$FOO[_index]"] {
        assert_names(
            &format!(".TH PROBE 1\n.SH VARIABLES\n.TP\n.B {name}\nValue.\n"),
            &[name],
            "missing",
        );
    }
    for name in [
        "FOO[bar",
        "FOO[bar]TRAILING",
        "FOO[]",
        "FOO[[bar]]",
        "FOO[a][b]",
        "FOO]",
        "FOO[bar]]",
    ] {
        let source = format!(".TH PROBE 1\n.SH VARIABLES\n.TP\n.B {name}\nValue.\n");
        let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        let index = mant_ir::SemanticIndex::build(document);
        assert!(
            index.section(&document.sections[0].id)[0]
                .aliases
                .is_empty(),
            "{name}"
        );
        assert!(document.diagnostics.iter().any(|d| d.code.as_deref() == Some("manual.semantic-entry.unclassified-definition")), "{name}: {:?}", document.diagnostics);
        assert!(crate::render_query_text(&query).contains(name), "{name}");
    }
}

#[test]
fn ordinary_man_paragraphs_reset_prevailing_definition_width() {
    use mant_ir::visit::{Visit, walk_definition_item};
    struct Widths(Vec<bool>);
    impl<'a> Visit<'a> for Widths {
        fn visit_definition_item(&mut self, item: &'a mant_ir::DefinitionItem) {
            self.0.push(item.inline_term);
            walk_definition_item(self, item);
        }
    }
    for (boundary, inline_second) in [("PP", false), ("P", false), ("LP", false), ("HP", true)] {
        let source = format!(
            ".TH PROBE 1\n.SH DESCRIPTION\n.TP 15\nFIRSTLONGTAG\nFIRST\n.{boundary}\nBETWEEN\n.TP\nSECONDLONGTAG\nSECOND\n"
        );
        let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
        let mut widths = Widths(Vec::new());
        widths.visit_document(query.document.as_ref().unwrap());
        assert_eq!(widths.0, [true, inline_second], "{source}");
        let text = crate::render_query_text(&query);
        assert!(
            text.lines()
                .any(|line| line.contains("FIRSTLONGTAG") && line.contains("FIRSTLONGTAG FIRST")),
            "{text}"
        );
        assert_eq!(
            text.lines()
                .any(|line| line.contains("SECONDLONGTAG") && line.ends_with("SECOND")),
            inline_second,
            "{text}"
        );
    }
}

#[test]
fn literal_display_controls_preserve_physical_rows_and_continuation() {
    for display in ["literal", "unfilled"] {
        for (body, expected) in [
            ("BEFORE\n.sp 2\nAFTER", "BEFORE\n\n\nAFTER"),
            ("BEFORE\n.sp 1\nAFTER", "BEFORE\n\nAFTER"),
            ("BEFORE\n.sp 0\nAFTER", "BEFORE\nAFTER"),
            ("BEFORE\n.br\nAFTER", "BEFORE\nAFTER"),
            ("BEFORE\n.Sm off\nAFTER", "BEFORE\nAFTER"),
            ("BEFORE\\c\nAFTER", "BEFOREAFTER"),
            ("BEFORE\\c\n.Sm off\nAFTER", "BEFOREAFTER"),
            ("BEFORE\n\nAFTER", "BEFORE\n\nAFTER"),
            ("BEFORE\n.sp 1\n.sp 1\nAFTER", "BEFORE\n\n\nAFTER"),
            ("BEFORE\\c\n.br\nAFTER", "BEFORE\nAFTER"),
            ("\\fBBEFORE\nAFTER\\fR", "BEFORE\nAFTER"),
            (
                ".Bf -emphasis\nBEFORE\n.sp 2\nAFTER\n.Ef",
                "BEFORE\n\n\nAFTER",
            ),
        ] {
            let source = format!(
                ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -{display} -offset left\n{body}\n.Ed\n"
            );
            let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
            let document = query.document.as_ref().unwrap();
            let mant_ir::Block::Preformatted { children, .. } = &document.sections[0].blocks[0]
            else {
                panic!("{document:?}")
            };
            assert_eq!(super::inline_text(children), expected, "{source}");
            assert!(
                crate::render_query_text(&query).contains(expected),
                "{source}: {}",
                crate::render_query_text(&query)
            );
        }
    }
}

/// A styling container is not a logical-line boundary. These inputs are
/// authored here from that contract, not copied from a formatter's tests.
#[test]
fn literal_continuations_cross_styling_containers_without_phantom_rows() {
    for body in [
        "FIRST\\c\n.Bf -emphasis\nSECOND\\c\n.Ef\nTHIRD",
        "FIRST\\c\n.Bf -emphasis\n.Sm off\n.Ef\nSECOND\\c\nTHIRD",
        ".Bf -emphasis\nFIRST\\c\n.Ef\nSECOND\\c\nTHIRD",
        "FIRST\\c\n.Bf -symbolic\n.Bf -emphasis\nSECOND\\c\n.Ef\n.Ef\nTHIRD",
    ] {
        let source = format!(
            ".Dd September 7, 2026\n.Dt FLOW 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal -offset left\n{body}\n.Ed\n"
        );
        let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
        let mant_ir::Block::Preformatted { children, .. } =
            &query.document.as_ref().unwrap().sections[0].blocks[0]
        else {
            panic!("{query:?}")
        };
        assert_eq!(super::inline_text(children), "FIRSTSECONDTHIRD", "{body}");
        assert!(crate::render_query_text(&query).contains("FIRSTSECONDTHIRD"));
        assert!(crate::render_markdown(&query).contains("FIRSTSECONDTHIRD"));
    }
}

#[test]
fn styled_literal_breaks_and_eof_keep_exact_content_boundaries() {
    for (body, expected) in [
        (
            "FIRST\\c\n.Bf -emphasis\n.br\nSECOND\n.Ef\nTHIRD",
            "FIRST\nSECOND\nTHIRD",
        ),
        (
            "FIRST\n.Bf -emphasis\nSECOND\n.sp 2\n.Ef\nTHIRD",
            "FIRST\nSECOND\n\n\nTHIRD",
        ),
        (
            "FIRST\n.Bf -emphasis\n.Sm off\n.Ef\nSECOND",
            "FIRST\nSECOND",
        ),
        ("FIRST\n.Bf -emphasis\nSECOND\\c\n.Ef", "FIRST\nSECOND"),
    ] {
        let source = format!(
            ".Dd September 7, 2026\n.Dt FLOW 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal -offset left\n{body}\n.Ed\n"
        );
        let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
        let mant_ir::Block::Preformatted { children, .. } =
            &query.document.as_ref().unwrap().sections[0].blocks[0]
        else {
            panic!("{query:?}")
        };
        assert_eq!(super::inline_text(children), expected, "{body}");
        assert!(crate::render_query_text(&query).contains(expected));
        assert!(crate::render_markdown(&query).contains(expected));
    }
}

#[test]
fn explicit_literal_breaks_are_not_repeated_at_styling_boundaries() {
    fn line_breaks(nodes: &[mant_ir::Inline]) -> usize {
        nodes
            .iter()
            .map(|node| match node {
                mant_ir::Inline::LineBreak => 1,
                mant_ir::Inline::Strong { children }
                | mant_ir::Inline::Emphasis { children }
                | mant_ir::Inline::Link { children, .. } => line_breaks(children),
                _ => 0,
            })
            .sum()
    }
    for (control, gap) in [
        (".br", "\n"),
        (".sp 0", "\n"),
        (".sp 1", "\n\n"),
        (".sp 2", "\n\n\n"),
    ] {
        for font in ["emphasis", "literal", "symbolic"] {
            for first in ["FIRST", "FIRST\\c"] {
                for (body, expected) in [
                    (
                        format!("{first}\n{control}\n.Bf -{font}\nSECOND\n.Ef\nTHIRD"),
                        format!("FIRST{gap}SECOND\nTHIRD"),
                    ),
                    (
                        format!("{first}\n.Bf -{font}\n{control}\nSECOND\n.Ef\nTHIRD"),
                        format!("FIRST{gap}SECOND\nTHIRD"),
                    ),
                    (
                        format!(
                            "{first}\n{control}\n.Bf -symbolic\n.Bf -{font}\nSECOND\n.Ef\n.Ef\nTHIRD"
                        ),
                        format!("FIRST{gap}SECOND\nTHIRD"),
                    ),
                    (
                        format!("{first}\n{control}\n.Bf -{font}\n.Ef\nSECOND\nTHIRD"),
                        format!("FIRST{gap}SECOND\nTHIRD"),
                    ),
                    (
                        format!("{first}\n{control}\n.Bf -{font}\nSECOND\\c\n.Ef"),
                        format!("FIRST{gap}SECOND"),
                    ),
                    (
                        format!("FIRST\n.Bf -{font}\nSECOND\n{control}\n.Ef\nTHIRD"),
                        format!("FIRST\nSECOND{gap}THIRD"),
                    ),
                    (
                        format!("FIRST\n.Bf -{font}\nSECOND\n.Ef\n{control}\nTHIRD"),
                        format!("FIRST\nSECOND{gap}THIRD"),
                    ),
                ] {
                    let source = format!(
                        ".Dd September 7, 2026\n.Dt FLOW 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal -offset left\n{body}\n.Ed\n"
                    );
                    let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
                    let mant_ir::Block::Preformatted { children, .. } =
                        &query.document.as_ref().unwrap().sections[0].blocks[0]
                    else {
                        panic!("{query:?}")
                    };
                    assert_eq!(super::inline_text(children), expected, "{body}");
                    assert_eq!(
                        line_breaks(children),
                        expected.matches('\n').count(),
                        "{body}"
                    );
                    assert!(
                        crate::render_query_text(&query).contains(&expected),
                        "{body}"
                    );
                    assert!(crate::render_markdown(&query).contains(&expected), "{body}");
                }
            }
        }
    }
}

#[test]
fn option_arguments_never_become_aliases() {
    for (head, aliases, missed) in [
        (".It Fl n Ar -NUM", vec!["-n"], "-NUM"),
        (".It Fl x Ar -1 | 1", vec!["-x"], "-1"),
        (
            ".It Fl n , Fl -number Ar NUM",
            vec!["-n", "--number"],
            "NUM",
        ),
        (".It Fl -arg Ns = Ns Ar VALUE", vec!["--arg"], "VALUE"),
    ] {
        let source = format!(
            ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n{head}\nDescription.\n.El\n"
        );
        assert_names(&source, &aliases, missed);
    }
    for head in [".BI \"-n \" -NUM", ".B -n\n.I -NUM", ".B \"-n -NUM\""] {
        assert_names(
            &format!(".TH PROBE 1\n.SH OPTIONS\n.TP\n{head}\nDescription.\n"),
            &["-n"],
            "-NUM",
        );
    }
}

fn assert_names(source: &str, names: &[&str], missed: &str) {
    let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
    let index = mant_ir::SemanticIndex::build(query.document.as_ref().unwrap());
    let entries = index.section(&query.document.as_ref().unwrap().sections[0].id);
    assert_eq!(entries[0].aliases, names, "{source}");
    for name in names {
        assert!(
            crate::select_explanation(&query, name).is_ok(),
            "{source}: {name}"
        );
    }
    assert!(
        crate::select_explanation(&query, missed).is_err(),
        "{source}: {missed}"
    );
}

#[test]
fn environment_assignment_values_are_not_alias_groups() {
    for form in ["FOO=one,two", "FOO=one|two", "\"FOO=one,two\""] {
        assert_names(
            &format!(".TH PROBE 1\n.SH ENVIRONMENT\n.TP\n.B {form}\nSet values.\n"),
            &["FOO"],
            "two",
        );
    }
    assert_names(
        ".TH PROBE 1\n.SH ENVIRONMENT\n.TP\n.B \"FOO, BAR\"\nSet values.\n",
        &["FOO", "BAR"],
        "missing",
    );
    for form in ["FOO=one, BAR=two", "FOO=one|BAR=two", "FOO=one BAR=two"] {
        let query = crate::query_roff_bytes(
            format!(".TH PROBE 1\n.SH ENVIRONMENT\n.TP\n.B \"{form}\"\nSet values.\n").as_bytes(),
        )
        .unwrap();
        assert!(crate::select_explanation(&query, "BAR").is_err());
        assert!(crate::select_explanation(&query, "FOO").is_err());
        assert!(query.document.unwrap().diagnostics.iter().any(|d| d.code.as_deref() == Some("manual.semantic-entry.unclassified-definition")));
    }
}

#[test]
fn command_alias_groups_survive_styled_names_and_argument_boundaries() {
    for head in [
        ".It Ic clone , Ic copy",
        ".It clone , copy",
        ".It Ic clone Ar PATH , Ic copy Ar PATH",
        ".It Ic clone | Ic copy",
    ] {
        assert_names(
            &format!(
                ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n{head}\nClone repository.\n.El\n"
            ),
            &["clone", "copy"],
            "PATH",
        );
    }
    for head in [
        ".B \"clone, copy\"",
        ".BR clone \", \" copy",
        ".B \"clone|copy\"",
    ] {
        assert_names(
            &format!(".TH PROBE 1\n.SH COMMANDS\n.TP\n{head}\nClone repository.\n"),
            &["clone", "copy"],
            "PATH",
        );
    }
}

#[test]
fn visual_indentation_does_not_change_a_top_level_command_role() {
    for (open, close) in [
        ("", ""),
        (".RS 4\n", ".RE\n"),
        (".RS 0\n.RS 8\n", ".RE\n.RE\n"),
    ] {
        assert_names(
            &format!(
                ".TH PROBE 1\n.SH COMMANDS\n{open}.TP\n.B clone\nClone a repository.\n{close}"
            ),
            &["clone"],
            "missing",
        );
    }
    for (open, close) in [
        ("", ""),
        (".Bd -ragged -offset 4n\n", ".Ed\n"),
        (".Bl -bullet\n.It\n", ".El\n"),
    ] {
        assert_names(
            &format!(
                ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n{open}.Bl -tag -width Ds\n.It Ic clone\nClone a repository.\n.El\n{close}"
            ),
            &["clone"],
            "missing",
        );
    }
}
