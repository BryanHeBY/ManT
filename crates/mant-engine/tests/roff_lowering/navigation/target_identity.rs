//! Target identity behavior contracts.

use super::*;
#[test]
fn native_generated_anchors_share_one_normalized_unique_namespace() {
    let document = parse_manual_bytes(
        std::path::Path::new("anchors.1"),
        b".TH ANCHORS 1\n.SH ALPHA\nProse.\n.SH OPTIONS\n.TP\n.B --ALPHA\nFirst.\n.TP\n.B --ALPHA\nSecond.\n",
    )
    .expect("lower repeated uppercase definition tags");
    let anchors = anchor_ids(&document);

    assert!(anchors.iter().any(|id| id == "alpha-2"));
    assert!(anchors.iter().any(|id| id == "alpha-3"));
    assert_eq!(anchors.len(), anchors.iter().collect::<HashSet<_>>().len());
    assert!(document.diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-identity" | "ir.duplicate-identity")
        )
    }));
}

#[test]
fn explicit_mdoc_targets_are_zero_width_and_unique() {
    let document = parse_manual_bytes(
        std::path::Path::new("target-only.1"),
        b".Dd September 3, 2026\n.Dt TARGET-ONLY 1\n.Os\n.Sh DESCRIPTION\n\
.Tg explicit-target\n\
ordinary text\n\
.Tg repeated-target\n\
more text\n\
.Tg repeated-target\n\
last text\n\
.Tg\n",
    )
    .expect("lower explicit target-only requests");

    let anchors = anchor_ids(&document);
    assert_eq!(
        anchors
            .iter()
            .filter(|target| target.as_str() == "explicit-target")
            .count(),
        1
    );
    assert_eq!(
        anchors
            .iter()
            .filter(|target| target.as_str() == "repeated-target")
            .count(),
        1
    );
    assert!(anchors.iter().all(|target| !target.is_empty()));
    let visible = visible_document_text(&document);
    assert!(!visible.contains("explicit-target"));
    assert!(!visible.contains("repeated-target"));
    assert!(visible.contains("ordinary text"));
    assert!(visible.contains("last text"));
}

#[test]
fn automatic_targets_use_source_tokens_independently_of_spacing_mode() {
    let document = parse_manual_bytes(
        std::path::Path::new("spacing-target.7"),
        b".Dd September 4, 2026\n.Dt SPACING-TARGET 7\n.Os\n.Sh DESCRIPTION\n\
.Sm off\n\
.Sy (U + S) / R .\n\
.Sm on\n",
    )
    .expect("lower an automatic target while mdoc spacing is disabled");

    assert!(anchor_ids(&document).iter().any(|id| id == "u"));
    assert!(!anchor_ids(&document).iter().any(|id| id == "u-s-r"));
}

#[test]
fn argumentless_targets_keep_non_relocated_owners_and_exact_prefixes() {
    for (macro_name, target) in [
        ("Va", "myvariable"),
        ("Pa", "mypath"),
        ("Ar", "myargument"),
        ("Cm", "--long"),
        ("Fl", "-long"),
    ] {
        let source = format!(
            ".Dd September 5, 2026\n.Dt TARGET 7\n.Os\n.Sh DESCRIPTION\n.Tg\n.{macro_name} {target}\nordinary text\n.Sh {target}\nOther section.\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("target.7"), source.as_bytes()).unwrap();
        let index = mant_ir::DocumentIndex::build(&document);
        assert!(
            index.fragment_target(target).is_some(),
            "missing {macro_name} {target}"
        );
        assert!(
            document
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code.as_deref() != Some("ir.invalid-identity"))
        );
        let anchors = anchor_ids(&document);
        assert!(
            !anchors.is_empty(),
            "a displaced section alone must not satisfy {target}"
        );
    }
}

#[test]
fn argumentless_targets_retain_libmandoc_derived_destinations() {
    let document = parse_manual_bytes(
        std::path::Path::new("derived-target.8"),
        b".Dd September 4, 2026\n.Dt DERIVED-TARGET 8\n.Os\n.Sh DESCRIPTION\n\
.Bl -tag -width Ds\n\
.It Xo\n\
.Ic route\n\
.Op Fl dtv\n\
.Op Fl T Ar rtable\n\
.Tg\n\
.Cm nameserver\n\
.Xc\n\
First description.\n\
.It Xo\n\
.Ic route\n\
.Tg\n\
.Cm sourceaddr\n\
.Xc\n\
Second description.\n\
.El\n",
    )
    .expect("lower argument-less targets in extended list heads");

    let index = mant_ir::DocumentIndex::build(&document);
    assert!(index.fragment_target("nameserver").is_some());
    assert!(index.fragment_target("sourceaddr").is_some());
    assert!(
        document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("ir.ambiguous-fragment-alias")
        })
    );
}

#[test]
fn explicit_section_targets_preserve_fragments_beside_normalized_ids() {
    let document = parse_manual_bytes(
        std::path::Path::new("section-targets.1"),
        b".Dd September 3, 2026\n.Dt SECTION-TARGETS 1\n.Os\n\
.Tg custom-section\n.Sh HEADING\ntext\n\
.Tg custom-subsection\n.Ss SUBHEADING\nmore text\n",
    )
    .expect("lower explicit section and subsection targets");

    assert_eq!(document.sections[0].id.as_str(), "heading");
    assert_eq!(
        document.sections[0]
            .fragment_aliases
            .iter()
            .map(mant_ir::FragmentAlias::as_str)
            .collect::<Vec<_>>(),
        ["custom-section"]
    );
    assert_eq!(document.sections[0].heading.plain_text(), "HEADING");
    assert_eq!(document.sections[0].children[0].id.as_str(), "subheading");
    assert_eq!(
        document.sections[0].children[0]
            .fragment_aliases
            .iter()
            .map(mant_ir::FragmentAlias::as_str)
            .collect::<Vec<_>>(),
        ["custom-subsection"]
    );
    assert_eq!(
        document.sections[0].children[0].heading.plain_text(),
        "SUBHEADING"
    );
    assert_eq!(anchor_ids(&document).len(), 0);
}

#[test]
fn noncanonical_mdoc_targets_keep_exact_fragments_without_invalid_ids() {
    let document = parse_manual_bytes(
        std::path::Path::new("authored-fragments.7"),
        b".Dd September 4, 2026\n.Dt AUTHORED-FRAGMENTS 7\n.Os\n\
.Tg Mixed.Section\n.Sh HEADING\n\
.Tg --option\noption target\n",
    )
    .expect("lower exact authored fragments");

    let index = mant_ir::DocumentIndex::build(&document);
    assert_eq!(
        index
            .fragment_target("Mixed.Section")
            .map(mant_ir::NodeId::as_str),
        Some("heading")
    );
    assert_eq!(
        index
            .fragment_target("--option")
            .map(mant_ir::NodeId::as_str),
        Some("option")
    );
    assert!(document.diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-identity" | "ir.invalid-fragment-alias")
        )
    }));
}

#[test]
fn function_block_anchor_keeps_the_fo_head_as_its_owner() {
    let source = b".Dd September 4, 2026\n\
.Dt FUNCTION-OWNER 3\n\
.Os\n\
.Sh DESCRIPTION\n\
.Bl -tag -width Ds\n\
.It Ev CALLBACK\n\
The callback is\n\
.Fo callback\n\
.Fa \"int value\"\n\
.Fc\n\
.El\n";
    let document = parse_manual_bytes(std::path::Path::new("function-owner.3"), source)
        .expect("lower function target nested in a definition");

    let owners = anchor_owner_lines(&document);
    assert!(
        owners
            .iter()
            .any(|(id, line)| id == "callback-2" && *line == 8),
        "the function destination must retain the Fo head line rather than the surrounding definition: {owners:?}"
    );
}

#[test]
fn explicit_targets_reserve_the_native_section_namespace() {
    let path = temporary_source(
        "mdoc-target-section-collision",
        ".Dd August 30, 2026\n\
         .Dt TARGET-COLLISION 1\n\
         .Os\n\
         .Sh FOO\n\
         .Tg bar\n\
         First.\n\
         .Sh BAR\n\
         Second.\n",
    );

    let document = parse_manual_source(&path).expect("lower reserved explicit target");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.sections[0].id, "foo");
    assert_eq!(document.sections[1].id, "bar-2");
    assert!(document.sections[0].blocks.iter().any(|block| matches!(
        block,
        Block::Paragraph { children, .. }
            if children.iter().any(|inline| matches!(
                inline,
                Inline::Anchor { id, .. } if id == "bar"
            ))
    )));
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code.as_deref() != Some("ir.identity-role-collision") })
    );
}

#[test]
fn automatic_function_target_survives_a_section_identity_collision() {
    let path = temporary_source(
        "mdoc-function-section-collision",
        ".Dd September 4, 2026\n\
         .Dt TARGET-COLLISION 3\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         .Ss acl_delete_def_file_at\n\
         .Fn acl_delete_def_file_at \"const char *path\"\n",
    );

    let document = parse_manual_source(&path).expect("lower automatic function target");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(
        document.sections[0].children[0].id,
        "acl-delete-def-file-at"
    );
    assert!(
        anchor_ids(&document)
            .iter()
            .any(|id| id == "acl-delete-def-file-at-2")
    );
}

#[test]
fn recognizes_explicitly_styled_traditional_man_references_in_any_section() {
    let path = temporary_source(
        "man-see-also",
        ".TH TOOL 1\n\
         .SH DESCRIPTION\n\
         The styled reference \\fBprintf\\fP(3) is usable here.\n\
         .SH SEE ALSO\n\
         .BR printf (3),\n\
         .BR man (1)\n",
    );

    let document = parse_manual_source(&path).expect("lower man references");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let see_also = document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SEE ALSO")
        .expect("SEE ALSO");
    let Block::Paragraph { children, .. } = &see_also.blocks[0] else {
        panic!("references are a paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
            if name == "printf" && manual_section == "3"
    )));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
            if name == "man" && manual_section == "1"
    )));

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("description is a paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Manual { name, manual_section: Some(manual_section) }, .. }
            if name == "printf" && manual_section == "3"
    )));
}

#[test]
fn lowers_modern_groff_manual_uri_and_mail_macros() {
    // Exact source rerun with pristine CVS -Tutf8/-Thtml before updating
    // these expectations. man_term.c::post_UR() generates literal '<' and
    // '>' for UR and MT; the general enclosure glyph table does not apply.
    let path = temporary_source(
        "man-modern-links",
        ".TH TOOL 1\n\
         .SH DESCRIPTION\n\
         .MR git-add 1 ,\n\
         .PP\n\
         Read\n\
         .UR https://example.test/docs\n\
         Documentation\n\
         .UE\n\
         now.\n\
         .PP\n\
         Mail comments, suggestions and bug reports to\n\
         .MT docs@example.test\n\
         Sean\n\
         .ME .\n",
    );

    let document = parse_manual_source(&path).expect("lower modern man links");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let section = &document.sections[0];
    let mut manual = false;
    let mut web = false;
    let mut mail = false;
    for children in section.blocks.iter().filter_map(|block| match block {
        Block::Paragraph { children, .. } => Some(children),
        _ => None,
    }) {
        for inline in children {
            match inline {
                Inline::Link {
                    target:
                        mant_ir::LinkTarget::Manual {
                            name,
                            manual_section: Some(manual_section),
                        },
                    ..
                } if name == "git-add" && manual_section == "1" => manual = true,
                Inline::Link {
                    target: mant_ir::LinkTarget::External { uri },
                    ..
                } if uri == "https://example.test/docs" => {
                    web = true;
                }
                Inline::Link {
                    target: mant_ir::LinkTarget::Email { address },
                    ..
                } if address == "docs@example.test" => {
                    mail = true;
                }
                _ => {}
            }
        }
    }

    assert!(manual && web && mail);
    assert!(section.blocks.iter().any(|block| match block {
        Block::Paragraph { children, .. } => inline_text(children).contains("git-add(1),"),
        _ => false,
    }));
    let linked_paragraphs = section
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. }
                if children.iter().any(|inline| {
                    matches!(
                        inline,
                        Inline::Link {
                            target: mant_ir::LinkTarget::External { .. },
                            ..
                        } | Inline::Link {
                            target: mant_ir::LinkTarget::Email { .. },
                            ..
                        }
                    )
                }) =>
            {
                Some(inline_text(children))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        linked_paragraphs,
        [
            "Read Documentation <https://example.test/docs> now.",
            "Mail comments, suggestions and bug reports to Sean <docs@example.test>."
        ]
    );
}
