//! Native markers and ordered lists recovered from roff definitions.

use super::super::*;

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
            assert_eq!(layout.indent_columns, 4);
            assert!(items.iter().all(|item| {
                // Source sample "1." is two cells plus the two-cell tag
                // buffer; the rendered ordinal marker consumes three.
                matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 1)
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
        // mdoc validation supplies 3n; the renderer adds two cells, leaving
        // two cells beyond the three-cell "1. " marker.
        matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 2)
    }), "{items:#?}");
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
    for (option, body_delta) in options[..2].iter().zip([1, 4]) {
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
                        Some(Block::Paragraph { layout, .. }) if layout.indent_columns == body_delta
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
        items,
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("one source-proven ordinal is an ordered list");
    };
    assert_eq!(items[0].layout.spacing_before_lines, Some(0));
    assert!(matches!(
        items[0].blocks.as_slice(),
        [
            Block::Paragraph { layout: body, .. },
            Block::Paragraph {
                layout: continuation,
                ..
            }
        ] if body.indent_columns == 1 && continuation.indent_columns == 1
    ));
    assert_eq!(SemanticIndex::build(&document).section("notes").len(), 0);
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
    assert_eq!(entries[0].children.len(), 0);
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
        matches!(item.blocks.first(), Some(Block::Paragraph { layout, .. }) if layout.indent_columns == 1)
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
    let rendered = mant_render::render_query_text(&ResolvedContent {
        label: "mdoc-tag-enumeration".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    });
    assert!(
        rendered.contains("1.  First step.\n\n2.  Second step.\n\n3.  Third step."),
        "{rendered}"
    );
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
