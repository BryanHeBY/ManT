//! Xr/MR attribute identities and executed labels have separate contracts.
//!
//! All exact sources below ran against the pinned pristine mandoc first with
//! `-Thtml -Oman=%N.%S`, `-Tutf8 -Owidth=120`, and `-Tascii -Owidth=120`.
//! HTML follows `mdoc_html.c::mdoc_xr_pre/man_html.c::man_MR_pre` and
//! `html.c::print_encode(norecurse=1)`; labels execute `termp_xr_pre/pre_MR`.

use mant_ir::{Block, Inline, LinkTarget, ReferenceTargetType, ResolvedContent, visit::Visit};
use mant_protocol::{
    EntryProjection, ReferenceCount, ReferenceProjection, ReferenceProjectionMode,
};

const MDOC: &str = ".Dd November 3, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n";
const MAN: &str = ".TH TEST 1\n.SH DESCRIPTION\n";

fn source(macro_name: &str, body: &str) -> String {
    if macro_name == "Xr" {
        format!("{MDOC}{body}.No AFTER\n.Sh NEXT\n.No END\n")
    } else {
        format!("{MAN}{body}AFTER\n.SH NEXT\nEND\n")
    }
}

fn round_trip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower reference source");
    let encoded = mant_render::render_query_json(&query, false).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&encoded).unwrap();
    let restored: ResolvedContent = decoded.into();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
        serde_json::from_str::<serde_json::Value>(
            &mant_render::render_query_json(&restored, false).unwrap()
        )
        .unwrap(),
        "JSON changed a target or its accepted content: {source}"
    );
    restored
}

fn body(query: &ResolvedContent) -> &[Inline] {
    let [Block::Paragraph { children, .. }] = query.document.as_ref().unwrap().sections[0]
        .blocks
        .as_slice()
    else {
        panic!("one reference paragraph expected: {query:#?}");
    };
    children
}

fn markdown_text(markdown: &str) -> String {
    pulldown_cmark::Parser::new(markdown)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text) => {
                Some(text.into_string())
            }
            pulldown_cmark::Event::SoftBreak | pulldown_cmark::Event::HardBreak => {
                Some("\n".into())
            }
            _ => None,
        })
        .collect()
}

fn assert_manual(source: &str, name: &str, section: Option<&str>, label: &str, text: &str) {
    let query = round_trip(source);
    assert_eq!(mant_ir::inline_plain_text(body(&query)), text, "{source}");
    let target = LinkTarget::Manual {
        name: name.into(),
        manual_section: section.map(str::to_owned),
    };
    assert_eq!(
        query
            .document
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code.as_deref() == Some("ir.invalid-manual-reference")
            }),
        target.to_uri().is_none(),
        "validation must preserve and diagnose the exact identity: {source}"
    );
    let outline = mant_query::build_outline_with_references(
        &query,
        EntryProjection::All,
        None,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::Manual],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        outline.references.occurrences,
        ReferenceCount::Exact { value: 1 }
    );
    assert_eq!(
        outline.references.targets,
        ReferenceCount::Exact { value: 1 }
    );
    let [record] = outline.references.records.as_slice() else {
        panic!("exactly one original manual occurrence expected: {source}\n{outline:#?}");
    };
    assert_eq!(record.target, target, "identity: {source}");
    assert_eq!(record.label, label, "outline label: {source}");
    let Some(Inline::Link {
        target: actual,
        children,
        ..
    }) = record.origin.resolve_link(query.document.as_ref().unwrap())
    else {
        panic!("outline occurrence lost its original link: {source}");
    };
    assert_eq!(actual, &target, "occurrence identity: {source}");
    assert_eq!(
        mant_ir::inline_plain_text(children),
        label,
        "label range: {source}"
    );

    // Portable body Markdown deliberately keeps manual links transparent.
    // The independent typed target survives JSON and outline projection.
    let markdown = mant_codec::encode::render_markdown(&query);
    let paragraph = markdown
        .split_once("## DESCRIPTION\n\n")
        .unwrap()
        .1
        .split_once("\n\n## NEXT")
        .unwrap()
        .0;
    assert_eq!(markdown_text(paragraph), text, "Markdown body: {source}");
    assert!(!paragraph.contains("man:"), "target became prose: {source}");
    let search = mant_query::search_query(
        &query,
        &mant_protocol::SearchQuery {
            pattern: label.trim().into(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Sensitive,
            scope: mant_protocol::SearchScope::Visible,
            context_lines: 0,
            limit: 10,
            offset: 0,
            word: false,
        },
    )
    .unwrap();
    let occurrences: Vec<_> = search
        .matches
        .iter()
        .flat_map(|hit| &hit.occurrences)
        .collect();
    let [occurrence] = occurrences.as_slice() else {
        panic!("visible label must occur once: {source}\n{search:#?}");
    };
    assert_eq!(
        occurrence.matched_text,
        label.trim(),
        "search label: {source}"
    );
    assert!(occurrence.markdown.end_byte > occurrence.markdown.start_byte);
    assert_ne!(occurrence.line_ranges, []);
}

#[test]
fn manual_operand_identities_follow_the_attribute_escape_rules() {
    // Complete 2 macros × 2 operand positions × 9 escape families. Motion
    // retains the documented one-space reading approximation; unsupported
    // numbered glyphs retain their source spelling as semantic recovery
    // (mant-roff.md), although both are omitted from the HTML identity.
    for macro_name in ["Xr", "MR"] {
        for name_operand in [true, false] {
            for (escape, identity_suffix, reading_suffix) in [
                (r"\zX", "", ""),
                (r"\*[.T]", "html", "utf8"),
                (r"\o'BC '", " ", "C"),
                (r"\h'2n'X", "X", " X"),
                (r"\:X", "\u{200b}X", "X"),
                (r"\N'256'X", "X", r"\N'256'X"),
                (r"\fBX\fP", "X", "X"),
                (r"\c", "", ""),
                (r"\zXY", "Y", "Y"),
            ] {
                let authored_name = format!("printf{}", if name_operand { escape } else { "" });
                let authored_section = format!("3{}", if name_operand { "" } else { escape });
                let name = format!("printf{}", if name_operand { identity_suffix } else { "" });
                let section = format!("3{}", if name_operand { "" } else { identity_suffix });
                let label = if name_operand {
                    format!("printf{reading_suffix}(3)")
                } else {
                    format!("printf(3{reading_suffix})")
                };
                let source = source(
                    macro_name,
                    &format!(".{macro_name} \"{authored_name}\" \"{authored_section}\"\n"),
                );
                assert_manual(
                    &source,
                    &name,
                    Some(&section),
                    &label,
                    &format!("{label} AFTER"),
                );
            }
        }
    }
}

#[test]
fn manual_reference_arity_and_suffix_remain_independent_of_identity() {
    for macro_name in ["Xr", "MR"] {
        // pre_MR emits () even with no section; termp_xr_pre returns after
        // the name. An omitted section stays Manual(None) in either case.
        let no_section_label = if macro_name == "MR" {
            "printf()"
        } else {
            "printf"
        };
        for (operands, name, section, label, text) in [
            (
                "printf",
                "printf",
                None,
                no_section_label,
                format!("{no_section_label} AFTER"),
            ),
            ("\"\" 3", "", Some("3"), "(3)", "(3) AFTER".into()),
            (
                "printf \"\"",
                "printf",
                Some(""),
                "printf()",
                "printf() AFTER".into(),
            ),
            ("\"\" \"\"", "", Some(""), "()", "() AFTER".into()),
            (r#""\zX" "3""#, "", Some("3"), "(3)", "(3) AFTER".into()),
            (
                r#""printf" "\zX""#,
                "printf",
                Some(""),
                "printf()",
                "printf() AFTER".into(),
            ),
            (
                r#""printf\zX" "3\zX" ,"#,
                "printf",
                Some("3"),
                "printf(3)",
                "printf(3), AFTER".into(),
            ),
        ] {
            assert_manual(
                &source(macro_name, &format!(".{macro_name} {operands}\n")),
                name,
                section,
                label,
                &text,
            );
        }
    }
    // With no name at all, pre_MR does not set NOSPACE before '('; a cached
    // prior glyph still settles at that generated word's ordinary boundary.
    for (prefix, expected) in [
        ("", "() AFTER"),
        ("PRE\n", "PRE () AFTER"),
        ("PRE\\zQ\n", "PREQ() AFTER"),
    ] {
        let query = round_trip(&source("MR", &format!("{prefix}.MR\n")));
        assert_eq!(mant_ir::inline_plain_text(body(&query)), expected);
        let mut targets = Targets::default();
        targets.visit_document(query.document.as_ref().unwrap());
        assert!(
            targets.0.is_empty(),
            "an omitted target must not be invented"
        );
    }
}

#[test]
fn manual_identities_do_not_consume_pending_glyph_or_continuation_state() {
    for macro_name in ["Xr", "MR"] {
        for (prefix, label, text) in [
            (r"PRE\zQ", "printf(3)", "PREQprintf(3) AFTER"),
            (r"PRE\z", " rintf(3)", "PRE rintf(3) AFTER"),
        ] {
            let prefix = if macro_name == "Xr" {
                format!(".No {prefix}\n")
            } else {
                format!("{prefix}\n")
            };
            let source = source(
                macro_name,
                &format!("{prefix}.{macro_name} \"printf\\zX\" 3\n"),
            );
            assert_manual(&source, "printf", Some("3"), label, text);
        }
        let source = if macro_name == "Xr" {
            format!(
                "{MDOC}.No \\fIPRE\n.Xr \"\\fBprintf\\zX\" 3\n.No \\fPAFTER\\c\n.No TAIL\n.Sh NEXT\n.No END\n"
            )
        } else {
            format!("{MAN}.I PRE\n.MR \"\\fBprintf\\zX\" 3\n\\fPAFTER\\c\nTAIL\n.SH NEXT\nEND\n")
        };
        assert_manual(
            &source,
            "printf",
            Some("3"),
            " printf(3)",
            "PRE printf(3) AFTERTAIL",
        );
    }
}

#[test]
fn manual_identity_operands_do_not_inherit_html_template_skip_state() {
    // Exact pristine HTML with man=%N.%S has href="printf3" when the name
    // ends in bare z: print_href() shares HTML_SKIPCHAR with template '.'.
    // Typed Manual components instead each start a fresh identity decoder.
    // term_word()/encode1() still execute bare z across generated syntax;
    // section-final z delays ')' until AFTER's separator consumes retreat.
    for macro_name in ["Xr", "MR"] {
        for (operands, label, text) in [
            (r#""printf\z" "3""#, "printf3)", "printf3) AFTER"),
            (r#""printf" "3\z""#, "printf(3)", "printf(3)AFTER"),
        ] {
            let source = source(macro_name, &format!(".{macro_name} {operands}\n"));
            assert_manual(&source, "printf", Some("3"), label, text);
            assert_eq!(
                LinkTarget::Manual {
                    name: "printf".into(),
                    manual_section: Some("3".into())
                }
                .to_uri()
                .as_deref(),
                Some("man:printf(3)")
            );
        }
    }
}

#[test]
fn man_manual_references_execute_both_pre_resets_and_the_post_reset() {
    // Every exact source ran pristine first. print_man_node() replaces the
    // font before/after MR; pre_MR() replaces it again before its name. Those
    // are three register updates, including when Roman replaces Roman.
    // Terminal overstrikes establish the style of every source word here.
    fn styled(nodes: &[Inline], strong: &mut String, emphasis: &mut String) {
        for node in nodes {
            match node {
                Inline::Strong { children } => {
                    strong.push_str(&mant_ir::inline_plain_text(children));
                }
                Inline::Emphasis { children } => {
                    emphasis.push_str(&mant_ir::inline_plain_text(children));
                }
                Inline::Link { children, .. } => styled(children, strong, emphasis),
                _ => {}
            }
        }
    }
    for (input, expected_body, expected_strong, expected_emphasis) in [
        (
            ".MR \"\\fBprintf\" 3\n\\fPAFTER\\c\nTAIL\n",
            "printf(3) AFTERTAIL",
            "printf(3)AFTERTAIL",
            "",
        ),
        (
            ".ft B\n.MR \"\\fPprintf\" 3\n\\fPAFTER\\c\nTAIL\n",
            "printf(3) AFTERTAIL",
            "",
            "",
        ),
        (
            ".MR printf \"\\fI3\" \"\\fBSUFFIX\"\n\\fPAFTER\\c\nTAIL\n",
            "printf(3)SUFFIX AFTERTAIL",
            "SUFFIXAFTERTAIL",
            "3)",
        ),
    ] {
        let source = format!("{MAN}{input}.SH NEXT\nEND\n");
        let query = round_trip(&source);
        assert_eq!(
            mant_ir::inline_plain_text(body(&query)),
            expected_body,
            "{source}"
        );
        let (mut strong, mut emphasis) = (String::new(), String::new());
        styled(body(&query), &mut strong, &mut emphasis);
        assert_eq!(strong, expected_strong, "bold glyphs: {source}");
        assert_eq!(emphasis, expected_emphasis, "italic glyphs: {source}");
    }
}

#[derive(Default)]
struct Targets(Vec<LinkTarget>);

impl<'ir> Visit<'ir> for Targets {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Link { target, .. } = inline {
            self.0.push(target.clone());
        }
        mant_ir::visit::walk_inline(self, inline);
    }
}

#[test]
fn section_phrases_keep_the_deroff_namespace_separate_from_link_attributes() {
    // The exact pristine HTML has Sx href="#A_zBC", Xr href="AC.3",
    // and two headings whose displayed text is AC but whose IDs differ.
    // html_make_id() calls roff.c::deroff() for Sh/Sx; Xr instead calls
    // print_encode(norecurse=1), which consumes the skipped B glyph.
    let source = format!(
        "{MDOC}.Sx \"A\\zBC\"\n.Xr \"A\\zBC\" 3\n.Sh AC\n.No FIRST\n.Sh \"A\\zBC\"\n.No SECOND\n"
    );
    let query = round_trip(&source);
    let document = query.document.as_ref().unwrap();
    assert_eq!(document.sections[1].heading.plain_text(), "AC");
    assert_eq!(document.sections[2].heading.plain_text(), "AC");
    assert_ne!(document.sections[1].id, document.sections[2].id);
    let mut targets = Targets::default();
    targets.visit_document(document);
    assert_eq!(
        targets.0,
        [
            LinkTarget::Section {
                id: document.sections[2].id.clone()
            },
            LinkTarget::Manual {
                name: "AC".into(),
                manual_section: Some("3".into())
            },
        ]
    );
    assert_eq!(mant_ir::inline_plain_text(body(&query)), "AC AC(3)");
}

#[test]
fn adjacent_link_families_share_identity_without_repairing_invalid_targets() {
    // Pairwise adjacent coverage: all five families with trailing z, device,
    // motion, and an invalid overstrike-space identity. Font/c/state combinations
    // above focus on Xr/MR rather than claiming the full Cartesian product.
    for family in ["UR", "MT", "Lk", "Mt", "%U"] {
        let email = matches!(family, "MT" | "Mt");
        let prefix = if email {
            "user@example.org"
        } else {
            "https://example.org/"
        };
        for (escape, suffix, invalid) in [
            (r"\zX", "", false),
            (r"\*[.T]", "html", false),
            (r"\h'2n'X", "X", false),
            (r"\o'BC '", " ", true),
        ] {
            let address = format!("{prefix}{escape}");
            let source = if matches!(family, "UR" | "MT") {
                let end = if email { "ME" } else { "UE" };
                source(
                    "MR",
                    &format!(".{family} \"{address}\"\nLINKLABEL\n.{end}\n"),
                )
            } else {
                let field = format!(".{family} \"{address}\"\n");
                let body = if family == "%U" {
                    format!(".Rs\n{field}.Re\n")
                } else {
                    field
                };
                source("Xr", &body)
            };
            let query = round_trip(&source);
            let mut targets = Targets::default();
            targets.visit_document(query.document.as_ref().unwrap());
            let identity = format!("{prefix}{suffix}");
            let target = if email {
                LinkTarget::Email { address: identity }
            } else {
                LinkTarget::External { uri: identity }
            };
            assert_eq!(targets.0, [target], "{source}");
            let code = if email {
                "ir.invalid-email-address"
            } else {
                "ir.invalid-external-uri"
            };
            assert_eq!(
                query
                    .document
                    .as_ref()
                    .unwrap()
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code.as_deref() == Some(code)),
                invalid,
                "{source}"
            );
            let text = mant_ir::inline_plain_text(body(&query));
            assert!(text.contains("AFTER"), "lost following source: {source}");
            if invalid {
                // print_encode retains the space; validation must diagnose
                // that exact identity while native overstrike prose survives.
                assert!(text.contains(&format!("{prefix}C")), "{source}");
            }
        }
    }
}
