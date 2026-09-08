//! Existing regressions grouped by entries behavior; expected values remain independent.
use super::*;

#[test]
fn composite_environment_options_do_not_promote_shell_labels() {
    let path = temporary_source(
        "environment-options",
        ".TH DEMO 1\n\
         .SH \"ENVIRONMENT OPTIONS\"\n\
         .TP\n\
         Unix Bourne shell:\n\
         UNZIP=-qq; export UNZIP\n\
         .TP\n\
         \\-q\n\
         Be quiet.\n",
    );

    let document = parse_manual_source(&path).expect("lower environment option fixture");
    fs::remove_file(path).expect("remove fixture");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("definitions");
    };
    assert_eq!(
        items[0].entry.as_ref().expect("term").kind,
        mant_ir::EntryKind::Term
    );
    assert_eq!(items[1].entry.as_ref().expect("option").names, ["-q"]);
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.semantic-entry.unclassified-definition")
            && diagnostic.message.contains("Unix Bourne shell:")
    }));
}

#[test]
fn separates_definition_layout_arguments_from_visible_terms() {
    let path = temporary_source(
        "definition-head-roles",
        ".TH HEAD-ROLES 1\n\
         .SH EXAMPLES\n\
         .TP \\w'man\\ 'u\n\
         .BI man \\ ls\n\
         Display ls.\n\
         .TP 4\n\
         4\n\
         A numeric term remains visible.\n\
         .IP \"1\" 8n\n\
         An IP width remains layout-only.\n",
    );

    let document = parse_manual_source(&path).expect("lower definition head roles");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(
        items
            .iter()
            .flat_map(|item| item.terms.iter())
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["man ls", "4", "1"]
    );
    assert!(matches!(
        items[0].terms[0].as_slice(),
        [
            Inline::Anchor { id, .. },
            Inline::Anchor { .. },
            Inline::Strong { .. },
            Inline::Emphasis { .. }
        ]
            if items[0].entry.as_ref().is_some_and(|identity| &identity.id == id)
    ));
    assert!(
        items
            .iter()
            .flat_map(|item| item.terms.iter())
            .all(|term| !inline_text(term).contains("96u"))
    );
}

#[test]
fn line_continuations_do_not_merge_independent_tp_owners() {
    let path = temporary_source(
        "continued-definition-aliases",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .BI \"\\-symbols=\" \"file\"\\c\n\
         .TP\n\
         .BI \"\\-s \" \"file\"\\c\n\
         \\&\n\
         Read symbols.\n",
    );

    let document = parse_manual_source(&path).expect("lower consecutive TP aliases");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["-symbols=file", "-s file"]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..1].iter().all(|item| item.description.is_empty()));
    assert!(!items[1].description.is_empty());
}

#[test]
fn keeps_unrelated_consecutive_tp_definitions_separate() {
    let path = temporary_source(
        "distinct-consecutive-definitions",
        ".TH DISTINCT 1\n\
         .SH OPTIONS\n\
         .TP\n\
         -a\n\
         .TP\n\
         -b\n\
         Description only for b.\n",
    );

    let document = parse_manual_source(&path).expect("lower distinct tagged paragraphs");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "-a");
    assert!(items[0].description.is_empty());
    assert_eq!(inline_text(&items[1].terms[0]), "-b");
    let Block::Paragraph { children, .. } = &items[1].description[0] else {
        panic!("expected second tagged paragraph description");
    };
    assert_eq!(inline_text(children), "Description only for b.");
}

#[test]
fn paragraph_distance_zero_does_not_turn_tp_items_into_aliases() {
    let path = temporary_source(
        "distinct-zero-distance-definitions",
        ".TH DISTINCT 1\n\
         .SH OPTIONS\n\
         .PD 0\n\
         .TP\n\
         -a\n\
         .TP\n\
         -b\n\
         Description only for b.\n",
    );

    let document = parse_manual_source(&path).expect("lower compact tagged paragraphs");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "-a");
    assert!(items[0].description.is_empty());
    assert_eq!(inline_text(&items[1].terms[0]), "-b");
    let Block::Paragraph { children, .. } = &items[1].description[0] else {
        panic!("expected second tagged paragraph description");
    };
    assert_eq!(inline_text(children), "Description only for b.");
}

#[test]
fn restoring_paragraph_distance_keeps_tp_owners_independent() {
    let path = temporary_source(
        "compact-alias-group",
        ".TH ALIASES 1\n\
         .SH COMMANDS\n\
         .TP\n\
         bind first-form\n\
         .PD 0\n\
         .TP\n\
         bind second-form\n\
         .PD\n\
         Shared description.\n",
    );

    let document = parse_manual_source(&path).expect("lower compact alias group");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["bind first-form", "bind second-form"]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..1].iter().all(|item| item.description.is_empty()));
    assert!(!items[1].description.is_empty());
}

#[test]
fn head_paragraph_distance_keeps_tp_owners_independent() {
    let path = temporary_source(
        "head-owned-compact-alias-group",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .PD 0\n\
         --first\n\
         .TP\n\
         --second\n\
         .PD\n\
         Shared description.\n",
    );

    let document = parse_manual_source(&path).expect("lower head-owned compact alias group");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["--first", "--second"]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..1].iter().all(|item| item.description.is_empty()));
    assert!(!items[1].description.is_empty());
}

#[test]
fn compact_tp_heads_and_preceding_orphan_remain_independent() {
    let path = temporary_source(
        "bounded-compact-alias-group",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         orphan\n\
         .TP\n\
         .PD 0\n\
         --first\n\
         .TP\n\
         --second\n\
         .PD\n\
         Shared description.\n",
    );

    let document = parse_manual_source(&path).expect("lower bounded compact alias group");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["orphan", "--first", "--second"]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..2].iter().all(|item| item.description.is_empty()));
    assert!(!items[2].description.is_empty());
}

#[test]
fn adjacent_compact_tp_runs_keep_all_owner_boundaries() {
    let path = temporary_source(
        "adjacent-compact-alias-groups",
        ".TH ALIASES 1\n\
         .SH OPTIONS\n\
         .TP\n\
         .PD 0\n\
         --first\n\
         .TP\n\
         --second\n\
         .PD\n\
         First description.\n\
         .TP\n\
         .PD 0\n\
         --third\n\
         .TP\n\
         --fourth\n\
         .PD\n\
         Second description.\n",
    );

    let document = parse_manual_source(&path).expect("lower adjacent compact alias groups");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 4);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["--first", "--second", "--third", "--fourth"]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[0].description.is_empty() && items[2].description.is_empty());
    assert_eq!(
        inline_text(match &items[1].description[0] {
            Block::Paragraph { children, .. } => children,
            _ => panic!("paragraph"),
        }),
        "First description."
    );
    assert_eq!(
        inline_text(match &items[3].description[0] {
            Block::Paragraph { children, .. } => children,
            _ => panic!("paragraph"),
        }),
        "Second description."
    );
}

#[test]
fn compact_ip_heads_keep_independent_descriptions() {
    let path = temporary_source(
        "indented-aliases",
        ".TH CONTROL 1\n\
         .SH OPTIONS\n\
         .PD 0\n\
         .IP \"\\fB-a\\fR\" 4\n\
         .IP \"\\fB--all\\fR\" 4\n\
         Show all entries.\n\
         .PD\n\
         .in 168u\n",
    );

    let document = parse_manual_source(&path).expect("lower indented aliases");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["-a", "--all"]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..1].iter().all(|item| item.description.is_empty()));
    assert!(!items[1].description.is_empty());
}

#[test]
fn headless_ip_macros_continue_the_preceding_definition() {
    let path = temporary_source(
        "headless-ip-continuations",
        ".TH CONTINUATIONS 1\n\
         .SH DESCRIPTION\n\
         .IP foo\n\
         First paragraph.\n\
         .IP\n\
         Second paragraph.\n\
         .IP\n\
         Third paragraph.\n\
         .IP bar\n\
         Bar body.\n",
    );

    let document = parse_manual_source(&path).expect("lower headless IP continuations");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "foo");
    assert_eq!(items[0].description.len(), 3);
    assert_eq!(
        items[0]
            .description
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["First paragraph.", "Second paragraph.", "Third paragraph."]
    );
    assert_eq!(inline_text(&items[1].terms[0]), "bar");
}

#[test]
fn tq_terms_share_one_semantic_option_identity() {
    let path = temporary_source(
        "tq-aliases",
        ".TH TQ-ALIASES 7\n\
         .SH OPTIONS\n\
         .TP\n\
         .B \\-\\-alpha\n\
         .TQ\n\
         .B \\-a\n\
         .TQ\n\
         .B \\-\\-ALPHA\n\
         Enable alpha mode.\n",
    );

    let document = parse_manual_source(&path).expect("lower TQ aliases");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["--alpha", "-a", "--ALPHA"]
    );
    assert_eq!(
        items[0].entry.as_ref().expect("option identity").names,
        ["--alpha", "-a", "--ALPHA"]
    );
}

#[test]
fn ip_does_not_absorb_unproven_definition_heads() {
    let path = temporary_source(
        "bounded-ip-aliases",
        ".TH IP-BOUNDARY 7\n\
         .SH OPTIONS\n\
         .TP\n\
         -a\n\
         .TP\n\
         -b\n\
         .TP\n\
         -c\n\
         .IP -d\n\
         Description only for d.\n",
    );

    let document = parse_manual_source(&path).expect("lower bounded IP definitions");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 4);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["-a", "-b", "-c", "-d"]
    );
    assert!(items[..3].iter().all(|item| item.description.is_empty()));
    assert!(!items[3].description.is_empty());
    assert!(!document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.definition-alias-boundary")
    }));
}

#[test]
fn expands_mdoc_bsd_lifecycle_and_release_forms() {
    let source = b".Dd August 19, 2026\n.Dt BSD-LIFECYCLE 7\n.Os\n.Sh DESCRIPTION\n.Bx\n.Bx -alpha\n.Bx -beta\n.Bx -devel .\n.Bx 4.3 .\n.Bx 4.3 Net/2 .\n.Bx 386 0.1 .\n";
    let document = parse_manual_bytes(std::path::Path::new("bsd-lifecycle.7"), source)
        .expect("lower mdoc BSD lifecycle forms");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one BSD lifecycle paragraph");
    };
    assert_eq!(
        inline_text(children),
        "BSD BSD (currently in alpha test) BSD (currently in beta test) BSD (currently under development). 4.3BSD. 4.3BSD Net/2. 386BSD 0.1."
    );
}

#[test]
fn preserves_mdoc_command_names_in_each_synopsis_form() {
    let document = parse_manual_bytes(
        std::path::Path::new("fido2-cred.1"),
        b".Dd August 19, 2026\n.Dt FIDO2-CRED 1\n.Os\n.Sh NAME\n.Nm fido2-cred\n.Nd make a credential\n.Sh SYNOPSIS\n.Nm\n.Fl M\n.Op Fl i Ar input_file\n.Nm fido2-cred\n.Fl V\n.Nm helper\n.Op Fl q\n",
    )
    .expect("lower mdoc synopsis names");
    let synopsis = &document.sections[1];
    let rendered = synopsis
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected synopsis paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        [
            "fido2-cred -M [-i input_file]",
            "fido2-cred -V",
            "helper [-q]",
        ]
    );
}

#[test]
fn only_tag_definition_lists_recover_ordered_procedures() {
    for style in ["tag", "diag", "hang", "inset", "ohang"] {
        let source = format!(
            ".Dd September 4, 2026\n.Dt MDOC-{style} 4\n.Os\n.Sh EXAMPLES\n\
.Bl -{style} -offset 4n -width \"1.\"\n\
.It 1.\nFirst step.\n\
.It 2.\nSecond step.\n\
.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("mdoc-definition-style.4"),
            source.as_bytes(),
        )
        .expect("lower mdoc definition style");
        if style == "tag" {
            let Block::List {
                kind: ListKind::Ordered { .. },
                items,
                layout,
                ..
            } = &document.sections[0].blocks[0]
            else {
                panic!("-{style} must recover one ordered list")
            };
            assert_eq!(layout.indent_columns, 0);
            assert!(items.iter().all(|item| {
                matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 4)
            }));
        } else {
            assert!(
                matches!(document.sections[0].blocks[0], Block::DefinitionList { .. }),
                "-{style} must retain definition semantics"
            );
        }
    }

    let native = parse_manual_bytes(
        std::path::Path::new("mdoc-native-enum.4"),
        b".Dd September 4, 2026\n.Dt MDOC-ENUM 4\n.Os\n.Sh EXAMPLES\n\
.Bl -enum -offset 4n\n\
.It\nFirst step.\n\
.It\nSecond step.\n\
.El\n",
    )
    .expect("lower native mdoc enum");
    let Block::List { items, .. } = &native.sections[0].blocks[0] else {
        panic!("native enum must remain an ordered list")
    };
    assert!(items.iter().all(|item| {
        matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 4)
    }));
}

#[test]
fn distinguishes_man_ip_enumeration_from_numeric_option_values() {
    let document = parse_manual_bytes(
        std::path::Path::new("ip-enumeration.1"),
        b".TH IP-ENUMERATION 1\n.SH OPTIONS\n\
.TP\n.B -fchanges\nThis flag makes these changes:\n.RS 4\n\
.IP 1. 4\nfirst change\n.IP 2. 4\nsecond change\n.IP 3. 4\nthird change\n.RE\n\
.TP\n.B -fcounter\nThese are generated steps:\n.RS 4\n\
.nr step 0 1\n.IP \\n+[step]\nfirst step\n.IP \\n+[step]\nsecond step\n.RE\n\
.TP\n.B -flevel=level\nThe level can be one of:\n.RS 4\n\
.IP 0 4\ndisabled\n.IP 1 4\nenabled\n.RE\n",
    )
    .expect("lower man IP lists and values");

    let options = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.as_slice()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(options.len(), 3);
    for option in &options[..2] {
        assert!(option.description.iter().any(|block| {
            let Block::List {
                kind: ListKind::Ordered { start: Some(1) },
                items,
                ..
            } = block
            else {
                return false;
            };
            items.len() >= 2
                && items.iter().all(|item| {
                    matches!(
                        item.blocks.first(),
                        Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 0
                    )
                })
        }));
    }
    assert!(
        options[2]
            .description
            .iter()
            .any(|block| matches!(block, Block::DefinitionList { items, .. } if items.len() == 2))
    );

    let index = SemanticIndex::build(&document);
    let entries = index.section("options");
    assert_eq!(entries.len(), 3);
    assert!(
        entries[..2]
            .iter()
            .all(|entry| entry.children.is_empty() && entry.value_domain.is_none())
    );
    assert_eq!(
        entries[2].value_domain,
        Some(ValueDomain::Choices { exhaustive: false })
    );
    assert_eq!(
        entries[2]
            .children
            .iter()
            .flat_map(|entry| &entry.names)
            .collect::<Vec<_>>(),
        ["0", "1"]
    );
}

#[test]
fn recognizes_one_source_proven_ip_ordinal_without_semantic_entry() {
    let document = parse_manual_bytes(
        std::path::Path::new("ip-reference-candidate.1"),
        b".TH IP-REFERENCE-CANDIDATE 1\n.SH NOTES\n\
.IP \" 9.\" 4\nOnly reference\n.RS 4\nfile:///only\n.RE\n",
    )
    .expect("lower one numbered IP reference");

    let Block::List {
        kind: ListKind::Ordered { start: Some(9) },
        compact: false,
        items,
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("one source-proven ordinal is an ordered list");
    };
    assert!(matches!(
        items[0].blocks.as_slice(),
        [
            Block::Paragraph { layout: body, .. },
            Block::Paragraph {
                layout: continuation,
                ..
            }
        ] if body.indent_columns == 0 && continuation.indent_columns == 0
    ));
    assert!(SemanticIndex::build(&document).section("notes").is_empty());
}

#[test]
fn recognizes_tp_enumerations_nested_below_a_definition() {
    let document = parse_manual_bytes(
        std::path::Path::new("tp-enumeration.1"),
        b".TH TP-ENUMERATION 1\n.SH OPTIONS\n\
.TP\n.B extdebug\nThis setting has the following effects:\n.RS 4\n\
.TP\n.B 1.\nFirst effect.\n\
.TP\n.B 2.\nSecond effect.\n\
.TP\n.B 3.\nThird effect.\n.RE\n",
    )
    .expect("lower nested TP enumeration");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("outer setting remains a definition");
    };
    assert!(items[0].description.iter().any(|block| {
        matches!(
            block,
            Block::List {
                kind: ListKind::Ordered { start: Some(1) },
                items,
                ..
            } if items.len() == 3
        )
    }));
    let index = SemanticIndex::build(&document);
    let entries = index.section("options");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].children.is_empty());
}

#[test]
fn mdoc_definition_layout_uses_the_normalized_list_width() {
    let path = temporary_source(
        "mdoc-definition-widths",
        ".Dd July 23, 2026\n.Dt WIDTHS 1\n.Os\n.Sh ITEMS\n\
         .Bl -tag -width 20n\n.It tenletters\nwide description\n.El\n\
         .Bl -tag -width 3n\n.It short\nnarrow description\n.El\n",
    );

    let document = parse_manual_source(&path).expect("lower mdoc definition widths");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let lists = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 2);
    assert!(lists[0][0].layout.inline_term);
    assert!(!lists[1][0].layout.inline_term);
}

#[test]
fn man_optional_arguments_keep_brackets_and_argument_styles() {
    for (requests, expected) in [
        (".OP --verbose", "[--verbose]"),
        (".OP --file FILE", "[--file FILE]"),
        (
            ".OP --file FILE\n.OP --verbose",
            "[--file FILE] [--verbose]",
        ),
    ] {
        for wrapper in [false, true] {
            let source = if wrapper {
                format!(".TH PROBE 1\n.SH SYNOPSIS\n.SY probe\n{requests}\n.YS\n")
            } else {
                format!(".TH PROBE 1\n.SH DESCRIPTION\n{requests}\n")
            };
            let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
            let text = crate::render_query_text(&query);
            assert!(text.contains(expected), "{source}: {text}");
            let markdown = crate::render_markdown(&query);
            assert!(markdown.contains("**--"), "{markdown}");
            if requests.contains("FILE") {
                assert!(markdown.contains("*FILE*"), "{markdown}");
            }
        }
    }
}

#[test]
fn keeps_command_names_in_extended_mdoc_synopsis_terms() {
    let document = parse_manual_bytes(
        std::path::Path::new("extended-synopsis.8"),
        b".Dd August 19, 2026\n.Dt EXTENDED-SYNOPSIS 8\n.Os\n.Sh NAME\n\
.Nm zinject\n.Nd inject faults\n.Sh SYNOPSIS\n.Bl -tag -width Ds\n\
.It Xo\n.Nm zinject\n.Xc\nList injections.\n\
.It Xo\n.Nm zinject\n.Fl b Ar bookmark\n.Xc\nInject a bookmark.\n.El\n",
    )
    .expect("lower extended mdoc synopsis terms");

    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("expected synopsis definition list");
    };
    assert_eq!(inline_text(&items[0].terms[0]), "zinject");
    assert_eq!(inline_text(&items[1].terms[0]), "zinject -b bookmark");
    assert!(matches!(
        items[0].terms[0].as_slice(),
        [Inline::Anchor { id, .. }, Inline::Strong { .. }]
            if items[0].entry.as_ref().is_some_and(|identity| &identity.id == id)
    ));
    assert!(
        items[1].terms[0]
            .iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
}

#[test]
fn preserves_nested_mdoc_spacing_state_in_definition_terms() {
    let document = parse_manual_bytes(
        std::path::Path::new("nested-spacing.1"),
        b".Dd August 19, 2026\n.Dt NESTED-SPACING 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n.It Fl L Xo\n.Sm off\n.Ar local_socket : host : hostport\n.Sm on\n.Xc\nForward a socket.\n.El\n",
    )
    .expect("lower nested mdoc spacing controls");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(
        inline_text(&items[0].terms[0]),
        "-L local_socket:host:hostport"
    );
}

#[test]
fn independent_mdoc_option_forms_do_not_borrow_the_following_description() {
    let document = parse_manual_bytes(
        std::path::Path::new("shared-option-forms.1"),
        b".Dd August 27, 2026\n.Dt SHARED-OPTION-FORMS 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl L Xo\n.Sm off\n.Oo Ar bind_address : Oc\n.Ar port : host : hostport\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Oo Ar bind_address : Oc\n.Ar port : remote_socket\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Ar local_socket : host : hostport\n.Sm on\n.Xc\n\
.It Fl L Xo\n.Sm off\n.Ar local_socket : remote_socket\n.Sm on\n.Xc\n\
Forward a local socket.\n.El\n",
    )
    .expect("lower option forms with a shared description");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 4);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        [
            "-L [bind_address:]port:host:hostport",
            "-L [bind_address:]port:remote_socket",
            "-L local_socket:host:hostport",
            "-L local_socket:remote_socket"
        ]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..3].iter().all(|item| item.description.is_empty()));
    assert!(!items[3].description.is_empty());
}

#[test]
fn independent_mdoc_options_keep_their_own_descriptions() {
    let document = parse_manual_bytes(
        std::path::Path::new("shared-option-description.1"),
        b".Dd August 29, 2026\n.Dt SHARED-OPTION-DESCRIPTION 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl I Ar encoding\n\
.It Fl O Ar encoding\n\
Convert filenames from the specified encoding.\n\
.El\n",
    )
    .expect("lower distinct options with a shared description");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items
            .iter()
            .map(|item| inline_text(&item.terms[0]))
            .collect::<Vec<_>>(),
        ["-I encoding", "-O encoding"]
    );
    assert!(items.iter().all(|item| item.terms.len() == 1));
    assert!(items[..1].iter().all(|item| item.description.is_empty()));
    assert!(!items[1].description.is_empty());
}

#[test]
fn preserves_a_single_mdoc_option_argument_and_its_description() {
    let document = parse_manual_bytes(
        std::path::Path::new("option-with-argument.1"),
        b".Dd August 29, 2026\n.Dt OPTION-WITH-ARGUMENT 1\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n\
.It Fl Z Ar mode\n\
Select the archive mode without losing this description.\n\
.El\n",
    )
    .expect("lower an mdoc option with one argument");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected an option definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(inline_text(&items[0].terms[0]), "-Z mode");
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["-Z"]);
    assert!(items[0].description.iter().any(|description| {
        matches!(description, Block::Paragraph { children, .. }
            if inline_text(children)
                == "Select the archive mode without losing this description.")
    }));
}

#[test]
fn separates_alternative_terms_in_an_extended_mdoc_definition_head() {
    let document = parse_manual_bytes(
        std::path::Path::new("extended-term-alternatives.8"),
        b".Dd August 19, 2026\n.Dt EXTENDED-TERM-ALTERNATIVES 8\n.Os\n.Sh OPTIONS\n\
.Bl -tag -width Ds\n.It Xo\n.Sm off\n.Ar ipaddr\n.Op / Ar masklen\n.Pp\n\
.Ar ipaddr\n.Op / Ar prefixlen\n.Sm on\n.Xc\nAccept this peer.\n.El\n",
    )
    .expect("lower alternative extended definition terms");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a definition list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].terms.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "ipaddr[/masklen]");
    assert_eq!(inline_text(&items[0].terms[1]), "ipaddr[/prefixlen]");
}

#[test]
fn reads_incrementing_registers_only_from_ip_markers() {
    let context = LoweringContext::new(
        None,
        Some(".IP \\n+[step] 4\n.IP 1 \\n+[width]\n.IPX \\n+[other]\n'IP \"\\n+[quoted]\" 4\n"),
    );

    assert!(context.man_ip_uses_incrementing_register(1));
    assert!(!context.man_ip_uses_incrementing_register(2));
    assert!(!context.man_ip_uses_incrementing_register(3));
    assert!(context.man_ip_uses_incrementing_register(4));
}

#[test]
fn unclosed_compact_run_does_not_cross_a_section_boundary() {
    let path = temporary_source(
        "section-bounded-compact-alias-group",
        ".TH ALIASES 1\n\
         .SH FIRST\n\
         .TP\n\
         .PD 0\n\
         first\n\
         .SH SECOND\n\
         .TP\n\
         second\n\
         .PD\n\
         Second description.\n",
    );

    let document = parse_manual_source(&path).expect("lower section-bounded compact run");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [first, second] = document.sections.as_slice() else {
        panic!("expected two sections");
    };
    let [
        Block::DefinitionList {
            items: first_items, ..
        },
    ] = first.blocks.as_slice()
    else {
        panic!("expected first definition list");
    };
    let [
        Block::DefinitionList {
            items: second_items,
            ..
        },
    ] = second.blocks.as_slice()
    else {
        panic!("expected second definition list");
    };
    assert_eq!(first_items.len(), 1);
    assert_eq!(inline_text(&first_items[0].terms[0]), "first");
    assert!(first_items[0].description.is_empty());
    assert_eq!(second_items.len(), 1);
    assert_eq!(inline_text(&second_items[0].terms[0]), "second");
    assert!(!second_items[0].description.is_empty());
}

#[test]
fn tq_continuation_starts_at_the_immediately_preceding_head() {
    let path = temporary_source(
        "bounded-tq-aliases",
        ".TH TQ-BOUNDARY 7\n\
         .SH OPTIONS\n\
         .TP\n\
         -a\n\
         .TP\n\
         -b\n\
         .TQ\n\
         --beta\n\
         Description only for beta.\n",
    );

    let document = parse_manual_source(&path).expect("lower bounded TQ definitions");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let [Block::DefinitionList { items, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(inline_text(&items[0].terms[0]), "-a");
    assert!(items[0].description.is_empty());
    assert_eq!(
        items[1]
            .terms
            .iter()
            .map(|term| inline_text(term))
            .collect::<Vec<_>>(),
        ["-b", "--beta"]
    );
    assert!(!document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.definition-alias-boundary")
    }));
}

#[test]
fn recovers_complete_numbered_sequences_from_mdoc_tag_lists() {
    let document = parse_manual_bytes(
        std::path::Path::new("mdoc-tag-enumeration.4"),
        b".Dd September 4, 2026\n.Dt MDOC-TAG-ENUMERATION 4\n.Os\n.Sh EXAMPLES\n\
.Bl -tag -width \"1.\"\n\
.It 1.\nFirst step.\n\
.Tg second-step\n\
.It 2.\nSecond step.\n\
.It 3.\nThird step.\n\
.El\n\
.Bl -tag -width \"1.\"\n\
.It 1.\nA real singleton definition.\n\
.El\n\
.Bl -tag -width \"1.\"\n\
.It 1.\nFirst non-sequence term.\n\
.It 3.\nThird non-sequence term.\n\
.El\n",
    )
    .expect("lower mdoc tag lists with numeric terms");

    assert!(matches!(
        document.sections[0].blocks[0],
        Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            ref items,
            ..
        } if items.len() == 3
    ));
    let Block::List { items, .. } = &document.sections[0].blocks[0] else {
        unreachable!("numbered tag list was asserted above")
    };
    assert!(items.iter().all(|item| {
        matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 4)
    }));
    assert!(matches!(
        document.sections[0].blocks[1],
        Block::DefinitionList { ref items, .. } if items.len() == 1
    ));
    assert!(matches!(
        document.sections[0].blocks[2],
        Block::DefinitionList { ref items, .. } if items.len() == 2
    ));
    assert!(
        SemanticIndex::build(&document)
            .section("examples")
            .iter()
            .all(|entry| entry.names.iter().all(|alias| alias != "2."))
    );
    assert!(
        mant_ir::DocumentIndex::build(&document)
            .fragment_target("second-step")
            .is_some()
    );
    let rendered = crate::render_query_text(&ResolvedContent {
        label: "mdoc-tag-enumeration".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    });
    assert!(rendered.contains("1.     First step.\n2.     Second step.\n3.     Third step."));
    assert!(!rendered.contains("1.         First step."));
}

#[test]
fn keeps_man_ordinal_boundaries_explicit_without_reclassifying_numeric_terms() {
    let document = parse_manual_bytes(
        std::path::Path::new("ordinal-boundaries.1"),
        b".TH ORDINAL-BOUNDARIES 1\n.SH BREAKS\n\
.IP 1. 4\none\n.IP 3. 4\nthree\n.IP 1) 4\nparen\n.IP 2. 4\nperiod\n\
.SH MIXED\n.IP 1. 4\none\n.TP\n.B 2.\ntwo\n\
.SH VALUES\n.TP\n.B 1\none\n.TP\n.B 2.2\ndecimal\n.TP\n.B v1.\nversion\n.TP\n.B 1.2.\nrelease\n",
    )
    .expect("lower ordinal boundaries");

    let starts = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::List {
                kind: ListKind::Ordered { start },
                ..
            } => *start,
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts, [1, 3, 1, 2]);
    assert!(matches!(
        document.sections[1].blocks.as_slice(),
        [Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            items,
            ..
        }] if items.len() == 2
    ));
    assert!(matches!(
        document.sections[2].blocks.as_slice(),
        [Block::DefinitionList { items, .. }] if items.len() == 4
    ));
}

#[test]
fn keeps_each_adjacent_rs_scope_in_the_current_ordinal_item() {
    let document = parse_manual_bytes(
        std::path::Path::new("ordinal-continuations.1"),
        b".TH ORDINAL-CONTINUATIONS 1\n.SH NOTES\n\
.IP 1. 4\none\n.RS 4\nfirst continuation\n.RE\n.RS 4\nsecond continuation\n.RE\n\
.PP\nseparate paragraph\n.IP 2. 4\ntwo\n",
    )
    .expect("lower adjacent relative-indent continuations");

    assert!(matches!(
        document.sections[0].blocks.as_slice(),
        [
            Block::List {
                kind: ListKind::Ordered { start: Some(1) },
                items: first,
                ..
            },
            Block::Paragraph { .. },
            Block::List {
                kind: ListKind::Ordered { start: Some(2) },
                items: second,
                ..
            }
        ] if first.len() == 1 && first[0].blocks.len() == 3 && second.len() == 1
    ));
}

#[test]
fn preserves_the_boundary_that_enters_a_compact_mdoc_term() {
    let document = parse_manual_bytes(
        std::path::Path::new("spacing-transition.5"),
        b".Dd August 19, 2026\n.Dt SPACING-TRANSITION 5\n.Os\n.Sh KEYWORDS\n\
.Bl -tag -width Ds\n.It Xo\n.Cm @newuser\n.Sm off\n.Ar name : uid : gid\n.Sm on\n.Xc\nCreate a user.\n.El\n",
    )
    .expect("lower an mdoc spacing transition inside a term");

    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("expected a keyword definition list");
    };
    assert_eq!(inline_text(&items[0].terms[0]), "@newuser name:uid:gid");
}
