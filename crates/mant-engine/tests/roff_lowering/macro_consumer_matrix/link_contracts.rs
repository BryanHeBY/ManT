//! Link identities and native display are independent contracts. Exact inputs
//! were run with the pristine oracle before these assertions; HTML target
//! rules are `mdoc_html.c::mdoc__x_pre()` and `html.c::print_encode(norecurse=1)`.

use super::{LinkTargets, link_targets};
use mant_ir::{Inline, LinkTarget, ResolvedContent, visit::Visit};

const PRE: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn reference_source(field: &str, argument: &str) -> String {
    format!("{PRE}.Rs\n.{field} \"{argument}\"\n.Re\n.Sh NEXT\n.No END\n")
}

fn roundtrip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower link fixture");
    let json = mant_render::render_query_json(&query, false).expect("encode link bundle");
    let decoded: mant_protocol::QueryBundle =
        serde_json::from_str(&json).expect("decode actual serialized link bundle");
    decoded.into()
}

fn search(query: &ResolvedContent, pattern: &str) -> mant_protocol::QuerySearch {
    mant_query::search_query(
        query,
        &mant_protocol::SearchQuery {
            pattern: pattern.to_owned(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::default(),
            scope: mant_protocol::SearchScope::default(),
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .expect("search visible link text")
}

#[test]
fn reference_rfc_targets_use_original_operand_eligibility() {
    // mdoc__x_pre tests the raw first TEXT before print_encode. Font and
    // zero-width controls can produce identical display without making an
    // RFC reference. Its isdigit loop accepts even an empty numeric suffix.
    for (argument, expected) in [
        (
            "RFC 1149",
            Some("https://www.rfc-editor.org/rfc/rfc1149.html"),
        ),
        ("RFC ", Some("https://www.rfc-editor.org/rfc/rfc.html")),
        (
            "RFC 000",
            Some("https://www.rfc-editor.org/rfc/rfc000.html"),
        ),
        ("RFC", None),
        ("RFC abc", None),
        ("rfc 1149", None),
        ("RFC1149", None),
        ("RFC 1149 extra", None),
        (r"RFC 1149\&", None),
        (r"\fBRFC 1149", None),
        (r"RFC \fB1149", None),
        (r"RFC 1149\fP", None),
    ] {
        let query = roundtrip(&reference_source("%R", argument));
        let expected: Vec<_> = expected.into_iter().map(str::to_owned).collect();
        assert_eq!(link_targets(&query), expected, "argument {argument:?}");
        let rendered = mant_render::render_query_man(&query);
        assert!(
            !rendered.contains("www.rfc-editor.org"),
            "typed target was inserted as body text: {argument:?}: {rendered}"
        );
    }
}

#[test]
fn reference_uri_identity_decodes_without_reexecuting_visible_words() {
    // print_encode's norecurse target projection skips fonts, \\& and the
    // glyph following \\z; the real terminal source stream remains separate.
    for (argument, expected) in [
        (r"https://e.example/a\&b", "https://e.example/ab"),
        (r"https://e.example/\fBa\fPb", "https://e.example/ab"),
        (r"https://e.example/\[u03B1]", "https://e.example/α"),
        (r"https://e.example/a\zb", "https://e.example/a"),
        (r"https://e.example/\z\fBa", "https://e.example/"),
        (r"https://e.example/a\z\&b", "https://e.example/ab"),
        (r"https://e.example/\z\cb", "https://e.example/b"),
        (r"https://e.example/\*[.T]", "https://e.example/html"),
        (r"https://e.example/\[nosuch]b", "https://e.example/b"),
        (r"https://e.example/\o'ab'", "https://e.example/b"),
    ] {
        let query = roundtrip(&reference_source("%U", argument));
        assert_eq!(link_targets(&query), [expected], "operand {argument:?}");
        let invalid_uri = query
            .document
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("ir.invalid-external-uri"));
        // CVS HTML encodes α as a numeric entity in href. Identity decoding
        // preserves that scalar, while the independent RFC 3986 URI policy
        // requires ASCII or percent triplets. Diagnose it without erasing
        // the safe Unicode source word.
        assert_eq!(invalid_uri, !expected.is_ascii(), "operand {argument:?}");
        if !expected.is_ascii() {
            assert!(mant_render::render_query_man(&query).contains('α'));
        }
    }
}

#[test]
fn man_link_identity_uses_html_decoding_while_its_target_word_stays_native() {
    // All 16 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint before
    // these assertions. man_UR_pre supplies the raw HEAD to print_otag;
    // print_encode(norecurse=1) skips a \\z glyph, uses device "html", and
    // takes the overstrike's final source character, including a blank.
    // post_UR executes that same source operand separately as terminal text.
    for (start, end, prefix, invalid_code) in [
        (
            "UR",
            "UE",
            "https://example.org/",
            "ir.invalid-external-uri",
        ),
        ("MT", "ME", "user@example.org", "ir.invalid-email-address"),
    ] {
        for (spelling, identity_suffix, visible_suffix, invalid) in [
            ("", "", "", false),
            (r"\zX", "", "", false),
            (r"\z\fBX\fP", "", "", false),
            (r"\&X", "X", "X", false),
            (r"\*[.T]", "html", "utf8", false),
            (r"\o'BC'", "C", "C", false),
            (r"\o'BC '", " ", "C", true),
            (r"\z\&X", "X", "", false),
        ] {
            let source = format!(
                ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.{start} \"{prefix}{spelling}\"\nLINKLABEL\n.{end}\nafter\n"
            );
            let query = roundtrip(&source);
            let identity = format!("{prefix}{identity_suffix}");
            assert_eq!(
                link_targets(&query),
                std::slice::from_ref(&identity),
                "{source}"
            );
            let expected = format!("LINKLABEL <{prefix}{visible_suffix}> after");
            let native = mant_render::render_query_man(&query);
            // Full-query content does not retain a final EOF delimiter.
            // Remove that terminator only; every authored interior row and
            // space stays part of the exact comparison.
            assert_eq!(
                native.trim_end_matches('\n'),
                format!("TEST(1)\n\nDESCRIPTION\n{expected}"),
                "{source}"
            );
            let document = query.document.as_ref().unwrap();
            assert_eq!(
                document
                    .diagnostics
                    .iter()
                    .any(|diagnostic| { diagnostic.code.as_deref() == Some(invalid_code) }),
                invalid,
                "{source}"
            );
            let result = search(&query, "LINKLABEL");
            let [line] = result.matches.as_slice() else {
                panic!("label search did not resolve: {source}\n{result:#?}");
            };
            let artifact = mant_codec::encode::render_addressable_markdown_with_options(
                &query,
                mant_codec::encode::MarkdownOptions {
                    native_text: true,
                    ..mant_codec::encode::MarkdownOptions::ADDRESSABLE
                },
            );
            let [occurrence] = line.occurrences.as_slice() else {
                panic!("expected one exact label occurrence: {source}\n{line:#?}");
            };
            let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                ..usize::try_from(occurrence.markdown.end_byte).unwrap();
            assert_eq!(&artifact.text()[range], "LINKLABEL", "{source}");
            if !invalid {
                assert_man_link_export(
                    &query,
                    start == "MT",
                    &identity,
                    &format!("{prefix}{visible_suffix}"),
                    &source,
                );
            }
        }
    }
}

fn assert_man_link_export(
    query: &ResolvedContent,
    email: bool,
    identity: &str,
    visible_target: &str,
    source: &str,
) {
    let destination = if email {
        format!("mailto:{identity}")
    } else {
        identity.to_owned()
    };
    let suffix_markdown = if email {
        format!("\\<{visible_target}\\>")
    } else {
        format!("<{visible_target}>")
    };
    assert_eq!(
        mant_codec::encode::render_markdown(query),
        format!("# TEST\n\n## DESCRIPTION\n\n[LINKLABEL]({destination}) {suffix_markdown} after"),
        "{source}"
    );
    // The established encoder preserves a visible <scheme:URI> as CommonMark
    // autolink syntax. That syntax is separate from the sole typed BODY
    // identity: device targets prove html href and visible utf8 can differ.
    let mut parsed_links = vec![(destination, "LINKLABEL".to_owned())];
    if !email {
        parsed_links.push((visible_target.to_owned(), visible_target.to_owned()));
    }
    assert_eq!(markdown_links(query), parsed_links, "{source}");
}

#[test]
fn mdoc_link_labels_exclude_prior_rows_after_an_output_checkpoint() {
    // All twelve exact sources ran pristine first: termp_lk_pre executes
    // description words after a caller's marker, and mdoc_lk_pre annotates
    // only those words. ta is a state request, not a new output owner.
    // The executed boundary remains native and Markdown text outside Link;
    // a prior cached z glyph remains outside that same semantic hit range.
    for (prefix, native_head, markdown_head) in [(r"X\p", "X\n", "X  \n"), (r"X\zZ", "XZ", "XZ")] {
        for (operand, label, styled) in [
            ("Y", "Y", "*Y*"),
            (r"\fBY", "Y", "**Y**"),
            (r"\fIY", "Y", "*Y*"),
            ("é名", "é名", "*é名*"),
            (r"\fBé名", "é名", "**é名**"),
            (r"\fIé名", "é名", "*é名*"),
        ] {
            let source = format!(
                "{PRE}.No {prefix}\n.ta 2n\n.Lk https://example.org {operand}\n.No AFTER\n.Sh NEXT\n.No END\n"
            );
            let query = roundtrip(&source);
            let outline = mant_query::build_outline_with_references(
                &query,
                mant_protocol::EntryProjection::All,
                None,
                &mant_protocol::ReferenceProjection {
                    mode: mant_protocol::ReferenceProjectionMode::All,
                    target_types: vec![mant_ir::ReferenceTargetType::External],
                    ..Default::default()
                },
            )
            .unwrap();
            let [record] = outline.references.records.as_slice() else {
                panic!("expected one original typed link: {source}\n{outline:#?}");
            };
            assert_eq!(record.label, label, "{source}");
            let document = query.document.as_ref().unwrap();
            let Some(Inline::Link { children, .. }) = record.origin.resolve_link(document) else {
                panic!("typed occurrence lost its native node: {source}");
            };
            assert_eq!(mant_ir::inline_plain_text(children), label, "{source}");
            let [mant_ir::Block::Paragraph { children, .. }] =
                document.sections[1].blocks.as_slice()
            else {
                panic!("native paragraph expected: {source}\n{document:#?}");
            };
            assert_eq!(
                mant_ir::inline_plain_text(children),
                format!("{native_head}{label}: https://example.org AFTER"),
                "{source}"
            );
            assert_eq!(
                mant_codec::encode::render_inline_fragment(
                    children,
                    mant_codec::encode::MarkdownFragmentOptions::default(),
                ),
                format!("{markdown_head}[{styled}](https://example.org) AFTER"),
                "{source}"
            );
        }
    }
}

#[test]
fn no_fill_man_post_executes_even_when_the_decoded_identity_is_empty() {
    // Exact sources ran pristine before these assertions. post_UR always
    // executes its <, original HEAD and > words. A pending p+c is resolved
    // by that same execution; NODE_LINE then closes only the following row.
    for (start, end) in [("UR", "UE"), ("MT", "ME")] {
        for (operand, target_word) in [(r"\zX", ""), ("https://example.org", "https://example.org")]
        {
            let suffix = format!("<{target_word}>");
            for (body, expected) in [
                ("", format!("{suffix}\nafter")),
                ("LINKLABEL\\p\n", format!("LINKLABEL\n{suffix}\nafter")),
                ("LINKLABEL\\p\\c\n", format!("LINKLABEL{suffix}\nafter")),
            ] {
                let source = format!(
                    ".TH TEST 1 \"2026-10-01\"\n.SH DESCRIPTION\n.nf\n.{start} \"{operand}\"\n{body}.{end}\nafter\n.fi\n"
                );
                let query = roundtrip(&source);
                let actual = mant_render::render_query_man(&query);
                assert_eq!(
                    actual.trim_end_matches('\n'),
                    format!("TEST(1)\n\nDESCRIPTION\n{expected}"),
                    "{source}"
                );
                let expected_targets = if target_word.is_empty() {
                    Vec::new()
                } else {
                    vec![target_word.to_owned()]
                };
                assert_eq!(link_targets(&query), expected_targets, "{source}");
            }
        }
    }
}

#[test]
fn generic_links_do_not_invent_target_appendices_in_consumers() {
    let query = mant_loader::load_markdown_text(
        "# Title\n\n[label](https://example.com/x)\n",
        Some("link.md".to_owned()),
    )
    .unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    let query: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&json)
        .unwrap()
        .into();
    for rendered in [
        mant_render::render_query_text(&query),
        mant_render::render_query_man(&query),
    ] {
        assert!(rendered.contains("label"), "{rendered}");
        assert!(!rendered.contains("https://example.com/x"), "{rendered}");
    }
    let document = query.document.as_ref().unwrap();
    let mut targets = LinkTargets::default();
    targets.visit_document(document);
    assert_eq!(targets.0, ["https://example.com/x"]);
    assert_eq!(search(&query, "label").matches.len(), 1);
    assert_eq!(search(&query, "https://example.com/x").matches.len(), 0);
}

fn markdown_links(query: &ResolvedContent) -> Vec<(String, String)> {
    let markdown = mant_codec::encode::render_markdown(query);
    let mut output = Vec::new();
    let mut active = None;
    for event in pulldown_cmark::Parser::new(&markdown) {
        match event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                active = Some((dest_url.into_string(), String::new()));
            }
            pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text) => {
                if let Some((_, label)) = &mut active {
                    label.push_str(&text);
                }
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Link) => {
                output.push(active.take().expect("active Markdown link"));
            }
            _ => {}
        }
    }
    output
}

#[test]
fn lk_operand_topology_drives_display_without_changing_anchor_text() {
    for (operand, label, native_text) in [
        (
            ".Lk https://e.example/label label\n",
            "label",
            "label: https://e.example/label AFTER",
        ),
        (
            ".Lk https://e.example/x https://e.example/x\n",
            "https://e.example/x",
            "https://e.example/x: https://e.example/x AFTER",
        ),
        (
            ".Lk https://e.example/x\n",
            "https://e.example/x",
            "https://e.example/x AFTER",
        ),
    ] {
        let source = format!("{PRE}{operand}.No AFTER\n.Sh NEXT\n.No END\n");
        let query = roundtrip(&source);
        let uri = if operand.contains("/label") {
            "https://e.example/label"
        } else {
            "https://e.example/x"
        };
        let rendered = mant_render::render_query_man(&query);
        assert!(rendered.contains(native_text), "{rendered}");
        assert_eq!(markdown_links(&query), [(uri.to_owned(), label.to_owned())]);
        assert_eq!(link_targets(&query), [uri]);
        assert_eq!(search(&query, "AFTER").matches.len(), 1);
    }
}

#[test]
fn invalid_targets_and_invisible_descriptions_keep_native_control_execution() {
    // termp_lk_pre emits ':' because the descr node exists, including with an
    // empty target. Safe label text survives URI validation failures.
    for description in ["\"\"", r"\&"] {
        let source = format!("{PRE}.Lk \"\" {description}\n.No AFTER\n.Sh NEXT\n.No END\n");
        let query = roundtrip(&source);
        assert!(mant_render::render_query_man(&query).contains(":  AFTER"));
        assert_eq!(link_targets(&query).len(), 0);
    }
    let source = format!("{PRE}.Lk \"::not a uri::\" label\n.No AFTER\n.Sh NEXT\n.No END\n");
    let query = roundtrip(&source);
    assert!(mant_render::render_query_man(&query).contains("label: ::not a uri:: AFTER"));
    assert_eq!(search(&query, "label").matches.len(), 1);
    let diagnostics = &query.document.as_ref().unwrap().diagnostics;
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code.as_deref() == Some("ir.invalid-external-uri") })
    );
    assert!(matches!(
        query.document.as_ref().unwrap().sections[1].blocks.first(),
        Some(mant_ir::Block::Paragraph { .. })
    ));
}

#[test]
fn invisible_lk_descriptions_preserve_executed_word_end_breaks() {
    // termp_lk_pre always executes descr -> ':' -> address. ESCAPE_BREAK
    // remains buffered until the address word: an invisible descr is not
    // permission to discard the preceding native hard boundary.
    for no_fill in [false, true] {
        let source = format!(
            "{PRE}{}.Lk https://e.example/x \\p\n.No AFTER\n{}.Sh NEXT\n.No END\n",
            if no_fill { ".nf\n" } else { "" },
            if no_fill { ".fi\n" } else { "" },
        );
        let query = roundtrip(&source);
        let rendered = mant_render::render_query_man(&query);
        let rows: Vec<_> = rendered.lines().map(str::trim_start).collect();
        let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
        let end = rows.iter().position(|row| *row == "NEXT").unwrap();
        let body = &rows[start..end];
        let expected: &[&str] = if no_fill {
            &[":", "https://e.example/x", "AFTER", ""]
        } else {
            &[":", "https://e.example/x AFTER", ""]
        };
        assert_eq!(body, expected, "no_fill={no_fill}: {rendered}");
    }
}

#[test]
fn link_annotations_cover_only_authored_labels() {
    struct Labels(Vec<String>);
    impl<'ir> Visit<'ir> for Labels {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link {
                target: LinkTarget::External { .. },
                children,
                ..
            } = inline
            {
                self.0.push(mant_ir::inline_plain_text(children));
            }
            mant_ir::visit::walk_inline(self, inline);
        }
    }

    let query = roundtrip(&format!(
        "{PRE}.Lk https://e.example/x label\n.No AFTER\n.Sh NEXT\n.No END\n"
    ));
    let mut labels = Labels(Vec::new());
    labels.visit_document(query.document.as_ref().unwrap());
    assert_eq!(labels.0, ["label"]);
}

#[test]
fn pending_mail_glyphs_keep_original_styles_and_stay_outside_address_identity() {
    struct Fonts(Vec<String>);
    impl<'ir> Visit<'ir> for Fonts {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Strong { children } = inline {
                self.0.push(mant_ir::inline_plain_text(children));
            }
            mant_ir::visit::walk_inline(self, inline);
        }
    }

    // Exact inputs checked with pristine UTF-8/ASCII/HTML/lint first.
    // Mt visits every operand via termp_under_pre; encode1's BACKBEFORE
    // retreat consumes the address's separator, preserving the prior glyph
    // with its original font and ownership (term.c:901-908).
    for (body, expected) in [
        (".Mt \\zX a@example.org\n", "Xa@example.org AFTER"),
        (".Mt \\z\\[u03B1] a@example.org\n", "αa@example.org AFTER"),
        (
            ".No \\fBX\\zY\n.Mt a@example.org\n",
            "XYa@example.org AFTER",
        ),
    ] {
        let query = roundtrip(&format!("{PRE}{body}.No AFTER\n.Sh NEXT\n.No END\n"));
        assert!(mant_render::render_query_man(&query).contains(expected));
        assert_eq!(link_targets(&query), ["a@example.org"]);
        assert_eq!(
            markdown_links(&query),
            [("mailto:a@example.org".into(), "a@example.org".into())]
        );
        let matched = search(&query, "a@example.org");
        assert_eq!(matched.matches.len(), 1);
        let artifact = mant_codec::encode::render_addressable_markdown_with_options(
            &query,
            mant_codec::encode::MarkdownOptions {
                native_text: true,
                ..mant_codec::encode::MarkdownOptions::ADDRESSABLE
            },
        );
        let occurrence = &matched.matches[0].occurrences[0];
        assert_eq!(occurrence.matched_text, "a@example.org");
        let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
            ..usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(&artifact.text()[range], "a@example.org");
        if body.starts_with(".No") {
            let mut fonts = Fonts(Vec::new());
            fonts.visit_document(query.document.as_ref().unwrap());
            assert!(fonts.0.concat().contains("XY"));
        }
    }
}
