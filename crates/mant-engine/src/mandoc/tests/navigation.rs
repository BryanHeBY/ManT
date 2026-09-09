//! Existing regressions grouped by navigation behavior; expected values remain independent.
use super::*;

#[test]
fn lowers_man_sections_fonts_definitions_and_literal_blocks() {
    let path = temporary_source(
        "man",
        ".TH MANT 1 \"July 2026\"\n\
         .SH NAME\n\
         mant \\- a viewer\n\
         .SH OPTIONS\n\
         .TP\n\
         \\fB\\-h\\fR\n\
         Show help.\n\
         .nf\n\
         mant --help\n\
         mant git\n\
         .fi\n",
    );

    let document = parse_manual_source(&path).expect("lower man source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.source.format, SourceFormat::Man);
    assert_eq!(
        document
            .sections
            .iter()
            .map(|section| section.heading.plain_text())
            .collect::<Vec<_>>(),
        vec!["NAME", "OPTIONS"]
    );
    assert!(
        document.sections[1]
            .blocks
            .iter()
            .any(|block| matches!(block, Block::DefinitionList { .. }))
    );
    assert!(document.sections[1].blocks.iter().any(|block| matches!(
        block,
        Block::DefinitionList { items, .. }
            if items.iter().any(|item| item.description.iter().any(
                |description| matches!(description, Block::Preformatted { .. })
            ))
    )));
}

#[test]
fn lowers_mdoc_semantic_inline_nodes_and_nested_sections() {
    let path = temporary_source(
        "mdoc",
        ".Dd July 19, 2026\n\
         .Dt MANT 1\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         Use\n\
         .Nm mant\n\
         with\n\
         .Xr man 1\n\
         Read\n\
         .Lk https://example.test/docs \"the documentation\"\n\
         or contact\n\
         .Mt docs@example.test\n\
         .Ss Details\n\
         .Fl h\n",
    );

    let document = parse_manual_source(&path).expect("lower mdoc source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.source.format, SourceFormat::Mdoc);
    assert_eq!(
        document.sections[0].children[0].heading.plain_text(),
        "Details"
    );
    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected description paragraph");
    };
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
    assert!(
        children.iter().any(
            |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Manual { name, .. }, .. } if name == "man")
        )
    );
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. } if uri == "https://example.test/docs")
    ));
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Email { address }, .. } if address == "docs@example.test")
    ));
}

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
fn argumentless_target_requests_bind_each_following_source_owner() {
    let document = parse_manual_bytes(
        std::path::Path::new("derived-target-owners.7"),
        b".Dd September 4, 2026\n.Dt DERIVED-TARGET-OWNERS 7\n.Os\n.Sh DESCRIPTION\n\
.Tg\n\
.Ic first-command\n\
.Pp\n\
Paragraph before the second request.\n\
.Tg\n\
.Ic second-command\n\
.Tg explicit-third\n\
.Ic third-command\n",
    )
    .expect("lower consecutive target ownership forms");

    let index = mant_ir::DocumentIndex::build(&document);
    for target in ["first-command", "second-command", "explicit-third"] {
        assert!(
            index.fragment_target(target).is_some(),
            "missing target request {target}"
        );
    }
    assert_eq!(
        index.fragment_target("third-command"),
        None,
        "an explicit .Tg spelling must replace the automatic owner spelling"
    );
}

#[test]
fn target_identity_is_independent_from_optional_raw_source_recovery() {
    let path = std::path::Path::new("target-source-parity.7");
    let source = b".Dd September 4, 2026\n.Dt TARGET-SOURCE-PARITY 7\n.Os\n\
.Tg Mixed.Section\n\
.Sh HEADING\n\
.Pp\n\
Paragraph before a target request.\n\
.Tg\n\
.Ic derived-command\n\
.Pp\n\
.Fn automatic_function\n";
    let with_source = parse_manual_bytes(path, source).expect("lower source-aware document");
    let report = Parser::default()
        .parse_bytes(path, source)
        .expect("parse owned native tree");
    let without_source = lower_mandoc_document(path, &report);

    let with_source_index = mant_ir::DocumentIndex::build(&with_source);
    let without_source_index = mant_ir::DocumentIndex::build(&without_source);
    for target in ["Mixed.Section", "derived-command", "automatic_function"] {
        assert_eq!(
            with_source_index
                .fragment_target(target)
                .map(mant_ir::NodeId::as_str),
            without_source_index
                .fragment_target(target)
                .map(mant_ir::NodeId::as_str),
            "target {target} changed when raw source recovery was unavailable"
        );
    }
}

#[test]
fn preserves_targets_moved_to_paragraphs_and_displays() {
    let paragraph = parse_manual_bytes(
        std::path::Path::new("paragraph-target.3"),
        b".Dd September 3, 2026\n.Dt PARAGRAPH-TARGET 3\n.Os\n.Sh DESCRIPTION\n\
intro\n.Pp\n.Fn alpha\n",
    )
    .expect("lower an automatic function target moved to Pp");
    assert!(anchor_ids(&paragraph).iter().any(|id| id == "alpha"));
    assert!(
        anchor_owner_lines(&paragraph)
            .iter()
            .any(|(id, line)| id == "alpha" && *line == 6)
    );
    assert!(visible_document_text(&paragraph).contains("alpha"));

    for (name, display) in [
        ("Bd", ".Bd -literal\nhello\n.Ed"),
        ("D1", ".D1 hello"),
        ("Dl", ".Dl hello"),
    ] {
        let source = format!(
            ".Dd September 3, 2026\n.Dt DISPLAY-TARGET 1\n.Os\n.Sh DESCRIPTION\n.Tg display-target\n{display}\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("{name}-target.1")),
            source.as_bytes(),
        )
        .unwrap_or_else(|error| panic!("lower {name} target: {error}"));
        assert_eq!(anchor_ids(&document), ["display-target"]);
        assert_eq!(visible_document_text(&document).trim(), "DESCRIPTION hello");
        assert!(matches!(
            document.sections[0].blocks.first(),
            Some(Block::Preformatted { children, .. })
                if matches!(children.first(), Some(Inline::Anchor { id, .. }) if id == "display-target")
        ));
    }
}

#[test]
fn preserves_targets_moved_to_list_items_and_containers() {
    let item = parse_manual_bytes(
        std::path::Path::new("list-item-target.1"),
        b".Dd September 3, 2026\n.Dt LIST-ITEM-TARGET 1\n.Os\n.Sh DESCRIPTION\n\
.Bl -bullet\n.It\n.Tg bullet-target\n.Em bullet text\n.El\n",
    )
    .expect("lower a target moved to an ordinary list item");
    assert_eq!(anchor_ids(&item), ["bullet-target"]);
    assert!(visible_document_text(&item).contains("bullet text"));

    let container = parse_manual_bytes(
        std::path::Path::new("list-container-target.1"),
        b".Dd September 3, 2026\n.Dt LIST-CONTAINER-TARGET 1\n.Os\n.Sh DESCRIPTION\n\
.Tg list-target\n.Bl -bullet\n.It\nhello\n.El\n",
    )
    .expect("lower a target moved to a list container");
    assert_eq!(anchor_ids(&container), ["list-target"]);
    assert_eq!(
        visible_document_text(&container).trim(),
        "DESCRIPTION hello"
    );
}

#[test]
fn same_named_list_target_owners_retain_their_individual_source_positions() {
    let document = parse_manual_bytes(
        std::path::Path::new("list-target-owner-positions.7"),
        b".Dd September 4, 2026\n.Dt LIST-TARGET-OWNER-POSITIONS 7\n.Os\n.Sh DESCRIPTION\n\
.Bl -bullet\n.Tg same\n.It\nfirst\n.Tg same\n.It\nsecond\n.El\n",
    )
    .expect("lower repeated targets onto separate list items");

    let owners = anchor_owner_lines(&document)
        .into_iter()
        .filter(|(id, _)| id == "same" || id.starts_with("same-"))
        .collect::<Vec<_>>();
    assert_eq!(owners, [("same".to_owned(), 7), ("same-2".to_owned(), 10)]);
}

#[test]
fn list_target_recovery_preserves_native_and_authored_fragment_spellings() {
    for style in ["-bullet", "-enum", "-item", "-tag", "-column one two"] {
        for (request, target, body) in [
            (".Tg\n.Sm off", "off", "CONTENT"),
            (".Tg Mixed.Target", "Mixed.Target", ""),
        ] {
            let source = format!(
                ".Dd September 5, 2026\n.Dt TARGET 7\n.Os\n.Sh DESCRIPTION\n.Bl {style}\n{request}\n.It\n{body}\n.El\n"
            );
            let document =
                parse_manual_bytes(std::path::Path::new("target.7"), source.as_bytes()).unwrap();
            let index = mant_ir::DocumentIndex::build(&document);
            assert!(index.fragment_target(target).is_some(), "{style}: {target}");
            assert!(
                !document
                    .diagnostics
                    .iter()
                    .any(|d| d.code.as_deref() == Some("ir.invalid-identity"))
            );
            assert!(!visible_document_text(&document).contains(target));
        }
    }
}

#[test]
fn pending_targets_retain_their_native_source_lines_when_no_item_owns_them() {
    let source = b".Dd September 5, 2026\n.Dt TARGETS 7\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.Tg\n.Sm off\n.It\nCONTENT\n.El\n.Sm on\n.Bl -column one two\n.Tg Mixed.Target\n.It\n.El\n";
    let document = parse_manual_bytes(std::path::Path::new("target-provenance.7"), source).unwrap();
    let owners = anchor_owner_lines(&document);
    assert!(owners.contains(&("off".to_owned(), 6)));
    assert!(owners.contains(&("mixed-target".to_owned(), 13)));
}

#[test]
fn preserves_explicit_targets_on_empty_mdoc_list_items() {
    let document = parse_manual_bytes(
        std::path::Path::new("empty-list-targets.7"),
        b".Dd September 4, 2026\n.Dt EMPTY-LIST-TARGETS 7\n.Os\n.Sh DESCRIPTION\n\
.Bl -bullet\n.Tg empty-bullet-target\n.It\n.El\n\
.Bl -enum\n.Tg empty-enum-target\n.It\n.El\n\
.Bl -item\n.Tg empty-plain-target\n.It\n.El\n\
.Bl -column \"one\" \"two\"\n.Tg empty-column-target\n.It\n.El\n\
.Bl -tag\n.Tg empty-definition-target\n.It\n.El\n\
.Bl -bullet\n.It\nbody\n.Tg trailing-list-target\n.El\n\
.Bl -bullet\n.Tg target-without-item\n.El\n",
    )
    .expect("lower targets at empty and trailing mdoc list positions");

    let index = mant_ir::DocumentIndex::build(&document);
    for target in [
        "empty-bullet-target",
        "empty-enum-target",
        "empty-plain-target",
        "empty-column-target",
        "empty-definition-target",
        "trailing-list-target",
        "target-without-item",
    ] {
        assert_eq!(
            index.fragment_target(target).map(mant_ir::NodeId::as_str),
            Some(target),
            "missing {target}"
        );
    }
    assert!(document.diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-identity" | "ir.duplicate-identity" | "ir.ambiguous-fragment-alias")
        )
    }));
    let visible = visible_document_text(&document);
    assert_eq!(visible.trim(), "DESCRIPTION body");
    assert!(!visible.contains("target"));
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
    assert!(anchor_ids(&document).is_empty());
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
fn root_blocks_receive_the_same_navigation_passes_as_sections() {
    let mdoc = parse_manual_bytes(
        std::path::Path::new("root-section-reference.1"),
        b".Dd September 3, 2026\n.Dt ROOT-SECTION-REFERENCE 1\n.Os\n\
See\n.Sx NAME\n.Sh NAME\n.Nm root-section-reference\n.Nd root reference\n",
    )
    .expect("lower a root-level mdoc section reference");
    assert!(mdoc.blocks.iter().any(|block| {
        matches!(block, Block::Paragraph { children, .. } if children.iter().any(|inline| {
            matches!(inline, Inline::Link {
                target: mant_ir::LinkTarget::Section { id }, ..
            } if id == "name")
        }))
    }));
    assert!(mdoc.diagnostics.iter().all(|diagnostic| {
        diagnostic.code.as_deref() != Some("ir.dangling-section-link")
            && diagnostic.code.as_deref() != Some("unresolved-section-reference")
    }));

    let man = parse_manual_bytes(
        std::path::Path::new("root-manual-reference.1"),
        b".TH ROOT-MANUAL-REFERENCE 1\n.BR printf (3)\n.SH NAME\nroot-manual-reference \\- root reference\n",
    )
    .expect("lower a root-level traditional manual reference");
    assert!(man.blocks.iter().any(|block| {
        matches!(block, Block::Paragraph { children, .. } if children.iter().any(|inline| {
            matches!(inline, Inline::Link {
                target: mant_ir::LinkTarget::Manual {
                    name,
                    manual_section: Some(section),
                }, ..
            } if name == "printf" && section == "3")
        }))
    }));
}

#[test]
fn preserves_targets_moved_to_paragraph_breaks_inside_no_fill_displays() {
    let document = parse_manual_bytes(
        std::path::Path::new("no-fill-paragraph-target.8"),
        b".Dd September 4, 2026\n.Dt NO-FILL-PARAGRAPH-TARGET 8\n.Os\n\
.Sh EXAMPLES\n\
.Bd -unfilled\n\
.Li first line\n\
.Pp\n\
.Li prompt Sy hp(0,0)\n\
.Ed\n",
    )
    .expect("lower a target moved onto a no-fill paragraph break");

    let [
        Block::Preformatted {
            children: before, ..
        },
        Block::VerticalSpace { lines: 1, .. },
        Block::Preformatted { children, .. },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!(
            "literal runs must retain their intervening paragraph request: {:?}",
            document.sections[0].blocks
        );
    };
    assert_eq!(inline_text(before), "first line");
    assert!(
        !before
            .iter()
            .any(|inline| matches!(inline, Inline::Anchor { .. }))
    );
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline,
                Inline::Anchor { id, owner_source: Some(source), .. }
                if id == "hp-0-0" && source.line == 7))
            .count(),
        1
    );
    assert!(inline_text(children).contains("prompt hp(0,0)"));
}

#[test]
fn retains_unlabelled_mdoc_link_targets_before_trailing_punctuation() {
    let document = parse_manual_bytes(
        std::path::Path::new("external-link.9"),
        b".Dd August 19, 2026\n.Dt EXTERNAL-LINK 9\n.Os\n.Sh DESCRIPTION\n.Lk https://example.test/books .\n",
    )
    .expect("lower an unlabelled mdoc external link");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one external-link paragraph");
    };
    assert_eq!(inline_text(children), "https://example.test/books.");
    assert!(matches!(
        children.as_slice(),
        [
            Inline::Link {
                target: mant_ir::LinkTarget::External { uri },
                children: link_children,
                ..
            },
            Inline::Text { value },
        ] if uri == "https://example.test/books"
            && inline_text(link_children) == "https://example.test/books"
            && value == "."
    ));
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
fn recognizes_legacy_sphinx_manual_links_in_roff_inputs() {
    let path = temporary_source(
        "sphinx-manual-links",
        ".TH BTRFS 8\n\
         .SH COMMANDS\n\
         See btrfs\\-subvolume(8) \\%<> and btrfs(5) \\%<> for details.\n\
         .EX\n\
         btrfs-subvolume(8) \\%<>\n\
         .EE\n",
    );

    let document = parse_manual_source(&path).expect("lower legacy Sphinx references");
    fs::remove_file(path).expect("remove temporary roff fixture");
    let section = &document.sections[0];
    let paragraph = section
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. } => Some(children),
            _ => None,
        })
        .expect("commands paragraph");
    assert_eq!(
        inline_text(paragraph),
        "See btrfs-subvolume(8) and btrfs(5) for details."
    );
    let references = paragraph
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link {
                target:
                    mant_ir::LinkTarget::Manual {
                        name,
                        manual_section: Some(manual_section),
                    },
                ..
            } => Some((name.as_str(), manual_section.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(references, [("btrfs-subvolume", "8"), ("btrfs", "5")]);

    let literal = section
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Preformatted { children, .. } => Some(children),
            _ => None,
        })
        .expect("literal display");
    assert_eq!(inline_text(literal), "btrfs-subvolume(8) <>");
    assert!(!literal.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Manual { .. },
            ..
        }
    )));
}

#[test]
fn searches_across_man_link_labels_and_visible_targets() {
    let source = b".TH LINK-SEARCH 1\n\
.SH REPORTING BUGS\n\
Mail comments, suggestions and bug reports to\n\
.MT docs@example.test\n\
Sean\n\
.ME .\n";

    for pattern in ["bug reports to Sean", "docs@example.test"] {
        let query = crate::query_roff_bytes(source).expect("query link fixture");
        let result = crate::project_query_view(
            query,
            &mant_protocol::QueryView::Search {
                pattern: pattern.to_owned(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 100,
                offset: 0,
            },
        )
        .expect("search link fixture");
        let crate::QueryViewResult::Search(search) = result else {
            panic!("expected search result");
        };
        assert_eq!(search.total, 1, "pattern={pattern:?}");
    }
}

#[test]
fn resolves_mdoc_section_references_and_explicit_targets() {
    let path = temporary_source(
        "mdoc-navigation",
        ".Dd July 19, 2026\n\
         .Dt NAVIGATION 1\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         Continue with\n\
         .Sx DETAILS\n\
         .Tg explicit-option\n\
         .Fl x\n\
         .Sh DETAILS\n\
         Target content.\n",
    );

    let document = parse_manual_source(&path).expect("lower navigation mdoc source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    assert_eq!(document.sections[0].id, "description");
    assert_eq!(document.sections[1].id, "details");
    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected navigation paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Section { id },
            children,
            ..
        } if id == "details" && inline_text(children) == "DETAILS"
    )));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Anchor { id, .. } if id == "explicit-option"
    )));
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
fn keeps_semantic_links_inside_tbl_text_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-text-link.1"),
        b".TH TABLE-TEXT-LINK 1\n\
.nr do-fallback 0\n\
.if !\\n(.f .nr do-fallback 1\n\
.if \\n[do-fallback] \\{\\\n\
.  de MR\n\
.    ie \\\\n(.$=1 \\\n\
.      I \\%\\\\$1\n\
.    el \\\n\
.      IR \\%\\\\$1 (\\\\$2)\\\\$3\n\
.  .\n\
.\\}\n\
.rr do-fallback\n\
.SH DESCRIPTION\n\
.TS\ntab($);\nl l.\ngrn$T{\nrenders\n.MR gremlin 1\ndiagrams;\nT}\n\
gperl$T{\npopulates\n.I groff\nregisters using\n.MR perl 1 ;\nT}\n.TE\n",
    )
    .expect("lower semantic tbl text block");

    let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("semantic table content must not escape into a separate paragraph");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[1].blocks.as_slice() else {
        panic!("expected semantic table cell paragraph");
    };
    assert_eq!(inline_text(children), "renders gremlin(1) diagrams;");
    assert!(children.iter().any(|child| matches!(
        child,
        Inline::Link {
            target: mant_ir::LinkTarget::Manual { name, manual_section },
            ..
        } if name == "gremlin" && manual_section.as_deref() == Some("1")
    )));
    let [Block::Paragraph { children, .. }] = rows[1].cells[1].blocks.as_slice() else {
        panic!("expected styled semantic table cell paragraph");
    };
    assert_eq!(
        inline_text(children),
        "populates groff registers using perl(1);"
    );
    assert!(
        children
            .iter()
            .any(|child| matches!(child, Inline::Emphasis { .. }))
    );
}

#[test]
fn preserves_printable_roff_content_outside_formal_sections() {
    let document = parse_manual_bytes(
        std::path::Path::new("manweb.1"),
        b".TH MANWEB 1\n .SH NAME\nmanweb - browse generated documentation\n.SH SYNOPSIS\n.B manweb\n",
    )
    .expect("lower root prose");
    let [Block::Paragraph { children, .. }] = document.blocks.as_slice() else {
        panic!("expected one root paragraph, got {:?}", document.blocks);
    };

    assert_eq!(
        inline_text(children),
        " .SH NAME manweb - browse generated documentation"
    );
    assert_eq!(document.sections[0].heading.plain_text(), "SYNOPSIS");
}

#[test]
fn native_section_ids_ignore_unrelated_section_insertions() {
    let mut original = LoweringContext::new(None, None);
    let original_name = original.section_id("NAME");
    let original_options = original.section_id("OPTIONS");

    let mut edited = LoweringContext::new(None, None);
    assert_eq!(edited.section_id("NOTES"), "notes");
    assert_eq!(edited.section_id("NAME"), original_name);
    assert_eq!(edited.section_id("OPTIONS"), original_options);
    assert_eq!(edited.section_id("OPTIONS"), "options-2");
}

#[test]
fn native_section_ids_disambiguate_final_slug_collisions() {
    let mut context = LoweringContext::new(None, None);
    assert_eq!(context.section_id("FOO"), "foo");
    assert_eq!(context.section_id("FOO"), "foo-2");
    assert_eq!(context.section_id("FOO 2"), "foo-2-2");
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
            "Read Documentation ⟨https://example.test/docs⟩ now.",
            "Mail comments, suggestions and bug reports to Sean ⟨docs@example.test⟩."
        ]
    );
}

#[test]
fn resolves_a_unique_parenthetically_qualified_mdoc_section_reference() {
    let path = temporary_source(
        "mdoc-qualified-navigation",
        ".Dd July 19, 2026\n\
         .Dt NAVIGATION 1\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         See\n\
         .Sx White Space Splitting\n\
         .Sh \"White Space Splitting (Field Splitting)\"\n\
         Target content.\n",
    );

    let document = parse_manual_source(&path).expect("lower qualified navigation source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected navigation paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Section { id },
            children,
            ..
        } if id == "white-space-splitting-field-splitting"
            && inline_text(children) == "White Space Splitting"
    )));
    assert!(
        document.diagnostics.iter().all(|diagnostic| {
            diagnostic.code.as_deref() != Some("unresolved-section-reference")
        })
    );
}

#[test]
fn degrades_unresolved_mdoc_section_references_to_text() {
    let path = temporary_source(
        "mdoc-missing-section",
        ".Dd July 19, 2026\n.Dt NAVIGATION 1\n.Os\n.Sh DESCRIPTION\n.Sx MISSING\n",
    );

    let document = parse_manual_source(&path).expect("lower unresolved navigation source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected reference paragraph");
    };
    assert_eq!(inline_text(children), "MISSING");
    assert!(children.iter().all(|inline| !matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Section { .. },
            ..
        }
    )));
    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("unresolved-section-reference")
        })
    );
}
