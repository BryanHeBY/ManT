//! References behavior contracts.

use super::*;

#[test]
fn native_uri_macros_publish_single_mailboxes_as_portable_email_links() {
    use mant_ir::LinkTarget;

    // Pinned CVS man_html.c::man_UR_pre and mdoc_html.c::mdoc_lk_pre
    // put the authored URI on the label. Their terminal handlers retain
    // the separate address word and put trailing punctuation outside it.
    let email = |address: &str| LinkTarget::Email {
        address: address.into(),
    };
    let external = |uri: &str| LinkTarget::External { uri: uri.into() };
    for (uri, expected, portable) in [
        (
            "mailto:rsync@lists.samba.org",
            Some(email("rsync@lists.samba.org")),
            true,
        ),
        (
            "MAILTO:percent%252B@example.org",
            Some(email("percent%2B@example.org")),
            true,
        ),
        (
            "mailto:local@example.org?subject=hello",
            Some(external("mailto:local@example.org?subject=hello")),
            true,
        ),
        (
            "mailto:local@example.org#topic",
            Some(external("mailto:local@example.org#topic")),
            true,
        ),
        (
            "mailto:a@example.org,b@example.org",
            Some(external("mailto:a@example.org,b@example.org")),
            true,
        ),
        (
            "https://example.org/home",
            Some(external("https://example.org/home")),
            true,
        ),
        ("mailto:invalid", Some(external("mailto:invalid")), false),
        (
            "mailto:invalid%GG@example.org",
            Some(external("mailto:invalid%GG@example.org")),
            false,
        ),
        ("man:printf(3)", Some(external("man:printf(3)")), false),
        ("", None, false),
    ] {
        for mdoc in [false, true] {
            let source = if mdoc {
                format!(
                    ".Dd November 18, 2026\n.Dt MAIL 1\n.Os\n.Sh CREDITS\n.Lk \"{uri}\" LINKLABEL .\n"
                )
            } else {
                format!(".TH MAIL 1\n.SH CREDITS\n.UR \"{uri}\"\n\\fBLINKLABEL\\fP\n.UE .\n")
            };
            assert_native_uri_export(&source, uri, expected.as_ref(), portable, mdoc);
        }
    }
}

fn assert_native_uri_export(
    source: &str,
    uri: &str,
    expected: Option<&mant_ir::LinkTarget>,
    portable: bool,
    mdoc: bool,
) {
    use mant_ir::{LinkTarget, ReferenceTargetType};

    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    for kind in [ReferenceTargetType::Email, ReferenceTargetType::External] {
        let records = credit_references(&query, vec![kind]);
        let targets = records
            .iter()
            .map(|record| {
                assert_eq!(record.label, "LINKLABEL", "{source}");
                &record.target
            })
            .collect::<Vec<_>>();
        let expected = expected
            .into_iter()
            .filter(|target| match kind {
                ReferenceTargetType::Email => matches!(target, LinkTarget::Email { .. }),
                ReferenceTargetType::External => {
                    matches!(target, LinkTarget::External { .. })
                }
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(targets, expected, "{source}");
    }
    let native_text = mant_render::render_query_text(&query);
    let expected_label = if mdoc {
        format!("LINKLABEL: {uri}.")
    } else {
        format!("LINKLABEL <{uri}>.")
    };
    if !uri.is_empty() {
        assert!(
            native_text.contains(&expected_label),
            "{source}: {native_text}"
        );
    }

    let markdown = mant_codec::encode::render_markdown_with_options(
        &query,
        mant_codec::encode::MarkdownOptions::ADDRESSABLE,
    );
    let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let records = credit_references(
        &decoded,
        vec![
            ReferenceTargetType::Email,
            ReferenceTargetType::External,
            ReferenceTargetType::Manual,
        ],
    );
    let expected_records = expected
        .into_iter()
        .filter(|_| portable)
        .map(|target| ("LINKLABEL", target))
        .collect::<Vec<_>>();
    let actual = records
        .iter()
        .map(|record| (record.label.as_str(), &record.target))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected_records, "{source}: {markdown}");
    let style = if mdoc { "*LINKLABEL*" } else { "**LINKLABEL**" };
    assert!(markdown.contains(style), "{source}: {markdown}");
    let decoded_text = mant_render::render_query_text(&decoded);
    assert_eq!(decoded_text.matches("LINKLABEL").count(), 1, "{markdown}");
    if !uri.is_empty() {
        assert!(
            decoded_text.contains(&expected_label),
            "{source}: {markdown}"
        );
    }
}

#[test]
fn native_mail_links_keep_unlabelled_uri_and_explicit_address_labels() {
    use mant_ir::{LinkTarget, ReferenceTargetType};

    // termp_lk_pre executes the target word as the label when there is no
    // description, while Mt visits the mailbox operand without a suffix.
    let address = "rsync@lists.samba.org";
    let uri = format!("mailto:{address}");
    let mdoc = ".Dd November 18, 2026\n.Dt MAIL 1\n.Os\n.Sh CREDITS\n";
    let man = ".TH MAIL 1\n.SH CREDITS\n";
    for (source, label) in [
        (format!("{mdoc}.Lk \"{uri}\" .\n"), uri.as_str()),
        (format!("{mdoc}.Lk \"{uri}\" \\& .\n"), uri.as_str()),
        (format!("{man}.UR \"{uri}\"\n.UE .\n"), uri.as_str()),
        (
            format!("{mdoc}.Rs\n.%T Reference title\n.%U {uri}\n.Re\n"),
            uri.as_str(),
        ),
        (
            format!("{man}.MT {address}\nLINKLABEL\n.ME .\n"),
            "LINKLABEL",
        ),
        (format!("{mdoc}.Mt {address} .\n"), address),
    ] {
        let input = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let markdown = mant_codec::encode::render_markdown_with_options(
            &input,
            mant_codec::encode::MarkdownOptions::ADDRESSABLE,
        );
        let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for query in [&input, &decoded] {
            let records = credit_references(
                query,
                vec![ReferenceTargetType::Email, ReferenceTargetType::External],
            );
            let [record] = records.as_slice() else {
                panic!("one email occurrence: {source}: {markdown}: {records:?}");
            };
            assert_eq!(
                record.target,
                LinkTarget::Email {
                    address: address.into()
                }
            );
            assert_eq!(record.label, label, "{source}: {markdown}");
        }
    }
}

fn credit_references(
    query: &mant_ir::ResolvedContent,
    target_types: Vec<mant_ir::ReferenceTargetType>,
) -> Vec<mant_protocol::ReferenceRecord> {
    mant_query::build_outline_with_references(
        query,
        mant_protocol::EntryProjection::All,
        Some(mant_protocol::ContentSelector::id("credits")),
        &mant_protocol::ReferenceProjection {
            mode: mant_protocol::ReferenceProjectionMode::All,
            target_types,
            ..Default::default()
        },
    )
    .unwrap()
    .references
    .records
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
