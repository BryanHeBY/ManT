//! Output ownership never changes the native execution boundary or link label.

use std::fmt::Write;

use mant_ir::{
    Block, Inline, LinkTarget, ReferenceTargetType, ResolvedContent,
    visit::{self, Visit},
};
use mant_protocol::{
    EntryProjection, QueryBundle, QueryOutline, ReferenceCount, ReferenceProjection,
    ReferenceProjectionMode,
};

const MAN: &str = ".TH TEST 1\n.SH DESCRIPTION\n";
const MDOC: &str = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n";

fn load_round_trip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let encoded = serde_json::to_string(&QueryBundle::from(&query)).unwrap();
    assert!(
        !encoded.contains("\\u0000mant:"),
        "private owner leaked: {source}\n{encoded}"
    );
    let restored: QueryBundle = serde_json::from_str(&encoded).unwrap();
    restored.into()
}

fn references(query: &ResolvedContent) -> QueryOutline {
    let outline = mant_query::build_outline_with_references(
        query,
        EntryProjection::All,
        None,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::External, ReferenceTargetType::Email],
            ..Default::default()
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&outline).unwrap();
    serde_json::from_str(&encoded).unwrap()
}

fn assert_one_label(query: &ResolvedContent, expected: &str, target: &LinkTarget) {
    let outline = references(query);
    assert_eq!(
        outline.references.occurrences,
        ReferenceCount::Exact { value: 1 }
    );
    assert_eq!(
        outline.references.targets,
        ReferenceCount::Exact { value: 1 }
    );
    let [record] = outline.references.records.as_slice() else {
        panic!("unexpected references: {:?}", outline.references);
    };
    assert_eq!(&record.target, target);
    assert_eq!(record.label, expected);
    let Some(Inline::Link {
        children,
        target: actual,
        ..
    }) = record.origin.resolve_link(query.document.as_ref().unwrap())
    else {
        panic!("reference position does not resolve its original link");
    };
    assert_eq!(actual, target);
    assert_eq!(mant_ir::inline_plain_text(children), expected);
}

#[test]
fn man_links_keep_the_first_label_after_owner_drains() {
    // Every exact source ran pristine CVS UTF-8/ASCII/HTML/tree/lint first.
    // man_term.c::print_man_node executes ordinary BODY blocks; man_html.c
    // closes the initial <a> at PP/IP/nf, so first remains the label while
    // item/body/prefix never become that link. Coordinate drains must not
    // reinterpret an IR Vec index after private field anchors are removed.
    for (prefix, body) in [
        ("prefix", "first\n.IP item 4\nbody\n"),
        ("prefix words", "first\n.IP item 4\nbody\n"),
        (".B prefix", "first\n.IP item 4\nbody\n"),
        ("prefix", "first\n.PP\nbody\n"),
        ("prefix", "first\n.nf\nbody\n.fi\n"),
        ("前缀 café", "first\n.IP item 4\nbody\n"),
        (".B 前缀", "first\n.IP item 4\nbody\n"),
        (".I café", "first\n.IP item 4\nbody\n"),
        ("前缀", ".B first\n.IP item 4\nbody\n"),
    ] {
        let source = format!("{MAN}{prefix}\n.UR outer\n{body}.UE\nafter\n");
        let query = load_round_trip(&source);
        assert_one_label(
            &query,
            "first",
            &LinkTarget::External {
                uri: "outer".into(),
            },
        );
        assert!(
            references(&query).references.records[0].owner.is_none(),
            "{source}"
        );
        let markdown = mant_codec::encode::render_markdown(&query);
        assert!(
            markdown.contains("[first](outer)") || markdown.contains("[**first**](outer)"),
            "{source}\n{markdown}"
        );
        assert!(
            !markdown.contains("[item](outer)") && !markdown.contains("[body](outer)"),
            "{source}\n{markdown}"
        );
    }
    let source =
        format!("{MAN}prefix\n.MT user@example.org\nfirst\n.IP item 4\nbody\n.ME\nafter\n");
    assert_one_label(
        &load_round_trip(&source),
        "first",
        &LinkTarget::Email {
            address: "user@example.org".into(),
        },
    );
}

#[test]
fn pending_glyphs_and_continued_prefixes_keep_their_original_owner() {
    // Exact pristine terminal runs establish the visible overstrike result.
    // pre_UR creates no execution boundary; post_UR's generated word may
    // release a pending glyph belonging to the caller, outside the label.
    for (prefix, expected) in [
        (r"prefix\zX\c", "first"),
        (r"prefix\zX", "first"),
        (r"prefix\z", "irst"),
    ] {
        let source = format!("{MAN}{prefix}\n.UR outer\nfirst\n.IP item 4\nbody\n.UE\nafter\n");
        assert_one_label(
            &load_round_trip(&source),
            expected,
            &LinkTarget::External {
                uri: "outer".into(),
            },
        );
    }
    let source = format!("{MAN}.nf\nprefix\\c\n.UR outer\nfirst\n.UE\nafter\n.fi\n");
    assert_one_label(
        &load_round_trip(&source),
        "first",
        &LinkTarget::External {
            uri: "outer".into(),
        },
    );
}

#[test]
fn word_end_breaks_and_zero_width_cells_do_not_lose_the_scope_boundary() {
    // Pristine term_fill starts each accepted/rejected pass from its native
    // buffer, where NBRZW contributes graph despite having no visible bytes.
    // An owner cursor must survive the accepted prefix and zero-width pass.
    for prefix in [r"prefix\p", r"prefix\p\p", r"prefix\p \p\&"] {
        for end in ["", ".PP\nsecond\n", ".IP item 4\nbody\n"] {
            let source = format!("{MAN}{prefix}\n.UR outer\nfirst\n{end}.UE\nafter\n");
            assert_one_label(
                &load_round_trip(&source),
                "first",
                &LinkTarget::External {
                    uri: "outer".into(),
                },
            );
        }
    }
}

#[test]
fn a_rejected_first_owner_does_not_move_the_link_to_a_later_block() {
    // All six exact sources ran pristine before these assertions. term_fill
    // rejects the first label after a graph-less marker pass; PP/IP retire
    // that buffer, so their later text survives. man_html.c closes the
    // initial anchor at that structural boundary: native rejection cannot
    // turn item/second/body into the earlier link's visible label.
    for prefix in [r"prefix\p \p DROP", r"\p DROP"] {
        for end in ["", ".PP\nsecond\n", ".IP item 4\nbody\n"] {
            let source = format!("{MAN}{prefix}\n.UR outer\nfirst\n{end}.UE\nafter\n");
            let query = load_round_trip(&source);
            let outline = references(&query);
            assert!(
                outline.references.records.iter().all(|record| {
                    record.label.is_empty()
                        && record.target
                            == LinkTarget::External {
                                uri: "outer".into(),
                            }
                }),
                "migrated rejected label: {source}\n{outline:#?}"
            );
            let text = mant_render::render_query_man(&query);
            assert!(
                !text.contains("first") && !text.contains("DROP"),
                "{source}\n{text}"
            );
            if end.starts_with(".PP") {
                assert!(
                    text.contains("second") && text.contains("after"),
                    "{source}\n{text}"
                );
            } else if end.starts_with(".IP") {
                assert!(
                    text.contains("item") && text.contains("body") && text.contains("after"),
                    "{source}\n{text}"
                );
            } else {
                assert!(!text.contains("after"), "{source}\n{text}");
            }
        }
    }
}

#[test]
fn empty_targets_and_empty_bodies_consume_their_private_owner() {
    // Exact pristine runs first: post_UR executes < and > even for an empty
    // target, while man_UR_pre selects HEAD as label only when BODY has no
    // child (a BODY containing \& has a child and remains visibly empty).
    for (start, end, label) in [
        ("UR", "UE", "label\\z"),
        ("MT", "ME", "label\\z"),
        ("UR", "UE", "label"),
        ("UR", "UE", ""),
    ] {
        let body = if label.is_empty() {
            String::new()
        } else {
            format!("{label}\n")
        };
        let source = format!("{MAN}.{start} \\&\n{body}.{end}\nafter\n");
        let query = load_round_trip(&source);
        assert!(references(&query).references.records.is_empty(), "{source}");
        assert!(query.document.as_ref().unwrap().sections[0].blocks.iter().all(|block| {
            !matches!(block, Block::Paragraph { children, .. } if mant_ir::inline_plain_text(children).is_empty())
        }), "fake empty paragraph: {source}");
        assert!(
            mant_render::render_query_man(&query).contains("after"),
            "{source}"
        );
    }
    for (start, end, address, target) in [
        (
            "UR",
            "UE",
            "https://example.org",
            LinkTarget::External {
                uri: "https://example.org".into(),
            },
        ),
        (
            "MT",
            "ME",
            "user@example.org",
            LinkTarget::Email {
                address: "user@example.org".into(),
            },
        ),
    ] {
        let source = format!("{MAN}.{start} {address}\n.{end}\nafter\n");
        assert_one_label(&load_round_trip(&source), address, &target);
    }
    let source = format!("{MAN}.UR outer\n\\&\n.UE\nafter\n");
    assert_one_label(
        &load_round_trip(&source),
        "",
        &LinkTarget::External {
            uri: "outer".into(),
        },
    );
}

#[test]
fn display_owner_drains_strip_only_unforgeable_private_field_anchors() {
    #[derive(Default)]
    struct Anchors(Vec<String>);
    impl Visit<'_> for Anchors {
        fn visit_inline(&mut self, inline: &Inline) {
            if let Inline::Anchor { id, .. } = inline {
                self.0.push(id.as_str().into());
            }
            visit::walk_inline(self, inline);
        }
    }
    // Exact pristine runs first. D1/Dl are ordinary inline executions inside
    // a display owner; an authored Tg spelling resembling a private marker
    // still denotes a real destination. Only the NUL-prefixed marker is private.
    for (macro_name, operand) in [
        ("D1", "hello"),
        ("Dl", "hello"),
        ("D1", "Em hello"),
        ("Dl", "Sy hello"),
        ("D1", "Lk https://example.org label"),
        ("Dl", "Lk https://example.org label"),
    ] {
        let source = format!("{MDOC}.{macro_name} {operand}\n");
        let query = load_round_trip(&source);
        let outline = references(&query);
        let count = u64::from(operand.starts_with("Lk "));
        assert_eq!(
            outline.references.occurrences,
            ReferenceCount::Exact { value: count }
        );
        assert_eq!(
            outline.references.targets,
            ReferenceCount::Exact { value: count }
        );
        if count == 1 {
            assert_one_label(
                &query,
                "label",
                &LinkTarget::External {
                    uri: "https://example.org".into(),
                },
            );
        }
    }
    let source = format!("{MDOC}.Tg mant-field-word-2-0\n.Dl hello\n");
    let query = load_round_trip(&source);
    let mut anchors = Anchors::default();
    visit::walk_document(&mut anchors, query.document.as_ref().unwrap());
    assert!(anchors.0.iter().any(|id| id == "mant-field-word-2-0"));
    assert!(anchors.0.iter().all(|id| !id.starts_with('\0')));
}

#[test]
fn adjacent_link_scopes_in_one_owner_keep_independent_labels_and_positions() {
    // This exact 128-scope source ran pristine UTF-8/ASCII/HTML/tree/lint
    // first. man_UR_pre opens 128 independent anchors in one paragraph;
    // each post_UR target belongs to that scope, not the accumulated prefix.
    let mut source = format!("{MAN}prefix\n");
    for index in 0..128 {
        writeln!(source, ".UR https://example.org/{index}\nlabel{index}\n.UE").unwrap();
    }
    source.push_str("after\n");
    let query = load_round_trip(&source);
    let outline = mant_query::build_outline_with_references(
        &query,
        EntryProjection::All,
        None,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::External],
            limit: 256,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        outline.references.occurrences,
        ReferenceCount::Exact { value: 128 }
    );
    assert_eq!(
        outline.references.targets,
        ReferenceCount::Exact { value: 128 }
    );
    assert_eq!(outline.references.records.len(), 128);
    for (index, record) in outline.references.records.iter().enumerate() {
        assert_eq!(record.label, format!("label{index}"));
        assert_eq!(
            record.target,
            LinkTarget::External {
                uri: format!("https://example.org/{index}")
            }
        );
        let Some(Inline::Link { children, .. }) =
            record.origin.resolve_link(query.document.as_ref().unwrap())
        else {
            panic!("scope {index} lost its exact content position");
        };
        assert_eq!(mant_ir::inline_plain_text(children), record.label);
        assert!(record.owner.is_none());
    }
}

#[test]
fn final_plain_receipts_keep_physical_rows_through_json_and_native_reading() {
    // Every exact source ran pristine UTF-8/ASCII/tree/lint first. The
    // pending buffer is consumed by term_flushln(), including accepted
    // marker passes after ta and an invisible NBRZW graph before Sm off
    // (term.c:143-220, mdoc_term.c:1820-1835). Compare physical rows, not
    // folded whitespace; the final newline is the normal ENDTEST spacing.
    const HEADER: &str =
        ".Dd October 1, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (body, expected) in [
        (".No X\\p\n.ta 2n\n.No Y\n.No AFTER\n", "X\nY AFTER\n"),
        (".No \\zX\\p\n.ta 2n\n.No Y\n.No AFTER\n", "XY\nAFTER\n"),
        (".No \\p\n.ta 2n\n.No Y\n.No AFTER\n", "\n"),
        (".No X\\p\n.ta 2n\n.No \\p\n.No Z\n.No AFTER\n", "X\n\n"),
        (
            ".No \\p\\&\n.Sm off\n.No Y\n.Sm on\n.No AFTER\n",
            "\nY AFTER\n",
        ),
        (
            ".No X\\p\n.ta 2n\n.Lk https://example.org Y\n.No AFTER\n",
            "X\nY: https://example.org AFTER\n",
        ),
    ] {
        let source = format!("{HEADER}{body}.Sh ENDTEST\n.No FINISH\n");
        let query = load_round_trip(&source);
        let rendered = mant_render::render_query_man(&query);
        let rows = rendered
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("\nENDTEST\n")
            .unwrap()
            .0;
        assert_eq!(rows, expected, "{source}\n{rendered}");
        if body.contains(".Lk") {
            assert_one_label(
                &query,
                "Y",
                &LinkTarget::External {
                    uri: "https://example.org".into(),
                },
            );
        }
    }
}
