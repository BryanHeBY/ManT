//! References behavior contracts.

use super::*;
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
        let query = mant_loader::load_roff_bytes(source).expect("query link fixture");
        let result = mant_engine::project_query_view(
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
        let mant_engine::QueryViewResult::Search(search) = result else {
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
