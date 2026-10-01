use super::*;

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
const FOOTER: &str = ".Sh NEXT\n.No END\n";
const URI: &str = "https://example.com";

fn source(payload: &str) -> String {
    format!("{HEADER}{payload}{FOOTER}")
}

fn parse(source: &str) -> mant_ir::Document {
    parse_manual_bytes(
        std::path::Path::new("lk-accepted-presentation.1"),
        source.as_bytes(),
    )
    .expect("parse exact Lk acceptance fixture")
}

fn paragraph(document: &mant_ir::Document) -> &[Inline] {
    let [Block::Paragraph { children, .. } | Block::Preformatted { children, .. }] =
        document.sections[1].blocks.as_slice()
    else {
        panic!("one native content owner expected: {document:#?}");
    };
    children
}

fn links(children: &[Inline]) -> Vec<(String, String)> {
    struct Links(Vec<(String, String)>);
    impl<'ir> Visit<'ir> for Links {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link {
                target: mant_ir::LinkTarget::External { uri },
                children,
                ..
            } = inline
            {
                self.0.push((uri.clone(), inline_text(children)));
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut visitor = Links(Vec::new());
    for child in children {
        visitor.visit_inline(child);
    }
    visitor.0
}

fn assert_no_private_metadata(document: &mant_ir::Document) {
    let json = serde_json::to_string(document).expect("serialize final IR");
    assert!(!json.contains("mant:lk-presentation"), "{json}");
    assert!(!json.contains("mant:output-scope"), "{json}");
    assert!(!json.contains("mant:field-word"), "{json}");
    assert!(!json.contains("mant:field-link-split"), "{json}");
    let decoded: mant_ir::Document = serde_json::from_str(&json).expect("actual JSON round trip");
    assert_eq!(&decoded, document);
}

#[test]
fn absent_accepted_descriptions_bind_only_the_accepted_uri() {
    // Exact complete sources were run with pristine ASCII/UTF-8/HTML/lint
    // before these assertions. termp_lk_pre() always prints colon + URI when
    // description operands exist, including empty TEXT, NBRZW, and BACKBEFORE.
    // Portable compaction needs a surviving description, rather than merely
    // the topology used by mdoc_lk_pre() to select its original HTML anchor.
    for label in [r#""""#, r"\&", r"\zX", r"\fB", r"\z"] {
        for no_fill in [false, true] {
            let control = if no_fill { ".nf\n" } else { "" };
            let end = if no_fill { ".fi\n" } else { "" };
            let source = source(&format!("{control}.Lk {URI} {label}\n.No AFTER\n{end}"));
            let document = parse(&source);
            let children = paragraph(&document);
            let separator = if label == r"\z" { "" } else { " " };
            let after = if no_fill { "\n" } else { " " };
            assert_eq!(
                inline_text(children),
                format!(":{separator}{URI}{after}AFTER"),
                "{source}"
            );
            assert_eq!(
                links(children),
                [(URI.to_owned(), URI.to_owned())],
                "{source}"
            );
            let markdown = crate::encode::render_inline_fragment(
                children,
                crate::encode::MarkdownFragmentOptions::default(),
            );
            let markdown_after = if no_fill { "  \n" } else { " " };
            // The existing default encoder uses an angle autolink when its
            // typed label exactly equals its URI; literal colons are escaped.
            assert_eq!(
                markdown,
                format!("\\:{separator}<{URI}>{markdown_after}AFTER"),
                "{source}"
            );
            assert_no_private_metadata(&document);
        }
    }
}

#[test]
fn zero_cell_descriptions_keep_native_content_and_a_readable_uri() {
    // These exact complete sources passed pristine ASCII/UTF-8/HTML/tree/lint
    // before assertions. term_word()/encode1()/term_fill() retain each Unicode
    // scalar as native graph even when locale_getwidth() returns zero. Only
    // the declared portable enhancement needs a readable, non-whitespace cell;
    // isolated accents/format scalars are retained, never rejected as content.
    for (operand, text, readable) in [
        (r"\[u200B]", "\u{200b}", false),
        (r"\[u200D]", "\u{200d}", false),
        (r"\[u0301]", "\u{301}", false),
        (r"e\[u0301]", "e\u{301}", true),
        (r#"" \[u200B]""#, " \u{200b}", false),
        (r"\[u1F469]\[u200D]\[u1F4BB]", "👩\u{200d}💻", true),
        (r#""\[u200B]\[u200D]""#, "\u{200b}\u{200d}", false),
        (r"\[u2060]", "\u{2060}", false),
        (r"\[uFEFF]", "\u{feff}", false),
        (r"\[uFE0F]", "\u{fe0f}", false),
        (r#"" \[u0301]""#, " \u{301}", false),
        (r#""\~\[u200B]""#, "\u{a0}\u{200b}", false),
        (r#""\0\[u200D]""#, "\u{a0}\u{200d}", false),
        (r"X\[u200B]", "X\u{200b}", true),
        (r#"" ""#, " ", false),
    ] {
        let source = source(&format!(".Lk {URI} {operand}\n.No AFTER\n"));
        let document = parse(&source);
        let children = paragraph(&document);
        assert_eq!(
            inline_text(children),
            format!("{text}: {URI} AFTER"),
            "{source}\n{children:#?}"
        );
        let label = if readable { text } else { URI };
        assert_eq!(
            links(children),
            [(URI.to_owned(), label.to_owned())],
            "{source}\n{children:#?}"
        );
        let markdown = crate::encode::render_inline_fragment(
            children,
            crate::encode::MarkdownFragmentOptions::default(),
        );
        // Style markers interrupt raw substrings; the established encoder
        // also retires ASCII padding at the beginning of a Markdown line.
        // Reparse the portable spelling to verify original Unicode glyphs
        // and ordering, while the exact native assertion above retains all
        // authored ASCII/nonbreaking cells independently of export policy.
        let portable_text = pulldown_cmark::Parser::new(&markdown)
            .filter_map(|event| match event {
                pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text) => {
                    Some(text.into_string())
                }
                _ => None,
            })
            .collect::<String>();
        let portable_label = text.trim_matches([' ', '\t']);
        let expected_portable = if readable {
            format!("{portable_label} AFTER")
        } else {
            format!("{portable_label}: {URI} AFTER")
        };
        assert_eq!(portable_text, expected_portable, "{source}\n{markdown:?}");
        if readable {
            assert!(!markdown.contains("\\:"), "compact label: {markdown:?}");
        } else {
            assert!(
                markdown.contains(&format!("\\: <{URI}> AFTER")),
                "readable URI fallback: {markdown:?}"
            );
        }
        assert_no_private_metadata(&document);
    }
}

#[test]
fn previous_pending_glyphs_do_not_qualify_as_a_descriptive_label() {
    // term_word's automatic blank releases the caller's cached glyph before
    // the label/colon words. encode1() ownership is retained by the shared
    // pending-output receipt, even though the IR glyph appears inside Lk.
    for (prefix, label, expected, expected_label) in [
        (r"\zX", r#""""#, format!("X: {URI} AFTER"), URI.to_owned()),
        (
            r"PREFIX\zX",
            r#""""#,
            format!("PREFIXX: {URI} AFTER"),
            URI.to_owned(),
        ),
        (
            r"PREFIX\zX",
            "label",
            format!("PREFIXXlabel: {URI} AFTER"),
            "label".to_owned(),
        ),
        (
            r"PREFIX\p",
            r#""""#,
            format!("PREFIX\n: {URI} AFTER"),
            URI.to_owned(),
        ),
    ] {
        let source = source(&format!(".No {prefix}\n.Lk {URI} {label}\n.No AFTER\n"));
        let document = parse(&source);
        let children = paragraph(&document);
        assert_eq!(inline_text(children), expected, "{source}");
        assert_eq!(
            links(children),
            [(URI.to_owned(), expected_label)],
            "{source}"
        );
        assert_no_private_metadata(&document);
    }
}

#[test]
fn accepted_descriptive_labels_keep_the_single_compact_anchor() {
    // Complete URI and suffix equality never shortcut native termp_lk_pre().
    // Only export presentation is compact; all accepted native words remain.
    for label in ["label", URI, "example.com", "first second"] {
        let source = source(&format!(".Lk {URI} {label}\n.No AFTER\n"));
        let document = parse(&source);
        let children = paragraph(&document);
        assert_eq!(
            inline_text(children),
            format!("{label}: {URI} AFTER"),
            "{source}"
        );
        assert_eq!(
            links(children),
            [(URI.to_owned(), label.to_owned())],
            "{source}"
        );
        let markdown = crate::encode::render_inline_fragment(
            children,
            crate::encode::MarkdownFragmentOptions::default(),
        );
        assert_eq!(markdown.matches(URI).count(), 1, "{markdown}");
        assert!(
            !markdown.contains(": "),
            "native suffix is redundant only here: {markdown}"
        );
        assert_no_private_metadata(&document);
    }
}

#[test]
fn field_rejection_decides_the_final_link_region() {
    // Exact TAG/HANG inputs were run first. term_fill() accepts X and Y but
    // refuses the later marker-led field before colon/URI; a preliminary
    // checkpoint cannot prove any of that future suffix remains accepted.
    for kind in ["hang", "tag"] {
        for (arguments, expected_terms, expected_label) in [
            (r#""X\p Y" "\p Z""#, "X\nY", Some("X\nY")),
            (r#""\p Z""#, "", Some("")),
        ] {
            let source = source(&format!(
                ".Bl -{kind} -width 4n\n.It Xo\n.Lk {URI} {arguments}\n.Xc\n.No BodyWord\n.El\n"
            ));
            let document = parse(&source);
            let [Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice()
            else {
                panic!("expected definition owner: {document:#?}");
            };
            let [item] = items.as_slice() else {
                panic!("one item");
            };
            let term = item
                .terms
                .iter()
                .map(|children| inline_text(children))
                .collect::<String>();
            assert_eq!(
                term.trim_end_matches('\n'),
                expected_terms,
                "{source}\n{item:#?}"
            );
            let identities = item
                .terms
                .iter()
                .flat_map(|children| links(children))
                .collect::<Vec<_>>();
            let expected = expected_label
                .map(|label| (URI.to_owned(), label.to_owned()))
                .into_iter()
                .collect::<Vec<_>>();
            assert_eq!(identities, expected, "{source}\n{item:#?}");
            assert!(
                item.description.iter().any(|block| matches!(block,
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. }
                    if inline_text(children) == "BodyWord")),
                "{source}\n{item:#?}"
            );
            assert_no_private_metadata(&document);
        }
    }
}

#[test]
fn rejected_links_keep_authored_identity_without_a_visible_range() {
    // Each complete source was rerun with pristine ASCII/UTF-8/HTML/tree/lint
    // before these assertions. term_fill() rejects the current HANG interval,
    // while mdoc_lk_pre() still derives href from each authored URI operand.
    // Empty decoded identities cannot invent a typed destination; repeated
    // equal addresses remain two occurrences with distinct source owners.
    for (link_source, count) in [
        (".Lk https://example.com visible", 1),
        (".Lk https://example.com", 1),
        (
            ".Lk https://example.com first\n.Lk https://example.com second",
            2,
        ),
        (r#".Lk "" visible"#, 0),
        (r".Lk \zX visible", 0),
    ] {
        let source = format!(
            "{HEADER}.Bl -hang -width 4n\n.It Xo X\n.br\n.No \\p\n{link_source}\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse(&source);
        let [Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice() else {
            panic!("expected definition owner: {document:#?}");
        };
        let [item] = items.as_slice() else {
            panic!("one item");
        };
        assert_eq!(
            item.terms
                .iter()
                .map(|term| inline_text(term))
                .collect::<String>(),
            // Rejected link fields print none of their deferred cells.
            // X's actual BODY gap remains layout (term_field:389-427),
            // rather than text owned by a rejected label's HEAD projection.
            "X",
            "{source}\n{item:#?}"
        );
        let identities = item
            .terms
            .iter()
            .flat_map(|term| links(term))
            .collect::<Vec<_>>();
        assert_eq!(
            identities,
            vec![(URI.to_owned(), String::new()); count],
            "{source}\n{item:#?}"
        );
        assert!(
            item.description.iter().any(|block| matches!(block,
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. }
                    if inline_text(children) == "BODY")),
            "{source}\n{item:#?}"
        );
        assert_no_private_metadata(&document);
    }
}

#[test]
fn unusable_or_empty_targets_never_hide_accepted_native_text() {
    // print_encode(norecurse=1) supplies identity only. The native colon and
    // URI still follow termp_lk_pre(), even when central RFC 3986 validation
    // prevents an external destination from being activated or exported.
    for (target, label, expected, readable_uri) in [
        (r#""""#, "label", "label:  AFTER", None),
        (r#""""#, r#""""#, ":  AFTER", None),
        (r"\zX", r#""""#, ": XAFTER", None),
        (
            r#""bad target""#,
            r#""""#,
            ": bad target AFTER",
            Some("bad target"),
        ),
        (
            r#""bad target""#,
            "label",
            "label: bad target AFTER",
            Some("bad target"),
        ),
        (
            "https://example.com/%xx",
            r#""""#,
            ": https://example.com/%xx AFTER",
            Some("https://example.com/%xx"),
        ),
    ] {
        let source = source(&format!(".Lk {target} {label}\n.No AFTER\n"));
        let document = parse(&source);
        let children = paragraph(&document);
        assert_eq!(inline_text(children), expected, "{source}\n{children:#?}");
        let markdown = crate::encode::render_inline_fragment(
            children,
            crate::encode::MarkdownFragmentOptions::default(),
        );
        if let Some(readable_uri) = readable_uri {
            assert!(markdown.contains(readable_uri), "{source}\n{markdown}");
        } else {
            assert!(
                links(children).is_empty(),
                "no identity was decoded: {children:#?}"
            );
        }
        assert_no_private_metadata(&document);
    }
}
