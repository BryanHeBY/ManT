//! Definition owners, term geometry, and source-ordered list boundaries.

use super::super::*;

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
        // CVS html.c::print_encode preserves the authored `\ ` in the
        // .BI head as a nonbreaking space; the fixed oracle emits &#x00A0;.
        ["man\u{a0}ls", "4", "1"]
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
    assert_ne!(items[1].description.len(), 0);
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
    assert_eq!(items[0].description.len(), 0);
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
    assert_eq!(items[0].description.len(), 0);
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
    assert_ne!(items[1].description.len(), 0);
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
    assert_ne!(items[1].description.len(), 0);
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
    assert_ne!(items[2].description.len(), 0);
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
    assert_ne!(items[1].description.len(), 0);
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
    assert_ne!(items[3].description.len(), 0);
    assert!(!document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("manual.definition-alias-boundary")
    }));
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
    assert!(lists[0][0].inline_term());
    assert!(!lists[1][0].inline_term());
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
    assert_eq!(first_items[0].description.len(), 0);
    assert_eq!(second_items.len(), 1);
    assert_eq!(inline_text(&second_items[0].terms[0]), "second");
    assert_ne!(second_items[0].description.len(), 0);
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
    assert_eq!(items[0].description.len(), 0);
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
