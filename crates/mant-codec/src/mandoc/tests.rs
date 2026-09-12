use std::{fmt::Write as _, fs, process};

use mant_ir::{
    Block, DiagnosticLevel, Inline,
    visit::{self, Visit},
};

use super::parse_plain_manual as parse_manual_bytes;
use super::{LoweringContext, MAX_INLINE_EQUATION_NORMALIZATIONS, Parser, lower_mandoc_document};

// Lowering tests acquire their own plain-text fixtures, then exercise only the
// byte codec. Product IO, compression and redirect policy tests live under
// manual_input; codec tests must not import that higher-level loader.
fn parse_manual_source(
    path: &std::path::Path,
) -> Result<mant_ir::Document, Box<dyn std::error::Error>> {
    Ok(parse_manual_bytes(path, &fs::read(path)?)?)
}

mod parser_contracts;

fn temporary_source(label: &str, source: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("mant-lower-{label}-{}.1", process::id()));
    fs::write(&path, source).expect("write temporary roff fixture");
    path
}

fn visible_document_text(document: &mant_ir::Document) -> String {
    struct TextCollector(String);

    impl<'ir> Visit<'ir> for TextCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            match inline {
                Inline::Text { value } | Inline::Code { value } => {
                    self.0.push_str(value);
                    self.0.push(' ');
                }
                Inline::LineBreak => self.0.push('\n'),
                Inline::Strong { .. }
                | Inline::Emphasis { .. }
                | Inline::Link { .. }
                | Inline::Anchor { .. } => {}
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut collector = TextCollector(String::new());
    collector.visit_document(document);
    collector.0
}

fn find_macro_mut<'a>(
    node: &'a mut libmandoc_rs::Node,
    name: &str,
) -> Option<&'a mut libmandoc_rs::Node> {
    if node.macro_name.as_deref() == Some(name) {
        return Some(node);
    }
    node.children
        .iter_mut()
        .find_map(|child| find_macro_mut(child, name))
}

fn replace_first_text(node: &mut libmandoc_rs::Node, value: &str) -> bool {
    if let Some(text) = node.text.as_mut() {
        *text = value.to_owned();
        return true;
    }
    node.children
        .iter_mut()
        .any(|child| replace_first_text(child, value))
}

fn inline_text(children: &[Inline]) -> String {
    children
        .iter()
        .map(|child| match child {
            Inline::Text { value } | Inline::Code { value } => value.clone(),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => inline_text(children),
            Inline::Anchor { .. } => String::new(),
            Inline::LineBreak => "\n".to_owned(),
        })
        .collect()
}

mod entries;

mod navigation;
mod tables;

#[test]
fn zero_advance_crosses_alternating_man_macro_arguments() {
    let document = parse_manual_bytes(
        std::path::Path::new("zero-advance-man-font-scope.1"),
        b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\n.BR A\\zX B\n",
    )
    .expect("parse alternating man font scope");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph");
    };

    // `\\zX` writes X without advancing. CVS term.c later writes B at that
    // same position even though the operands are separate `term_word()`
    // calls, so the semantic projection must be AB rather than AXB.
    assert_eq!(inline_text(children), "AB");
}

#[test]
fn zero_advance_projects_implicit_words_and_generated_op_brackets_in_output_order() {
    let document = parse_manual_bytes(
        std::path::Path::new("zero-advance-output-order.1"),
        b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\nA\\zX\nB\n.OP A\\zX B\n.OP A\\zX\n",
    )
    .expect("parse zero-advance formatter boundaries");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections[0].blocks);
    };

    // CVS term.c inserts the filled-word blank before the next glyph, which
    // preserves X without making the blank visible. man_term.c emits `.OP`
    // brackets through term_word(), so B and ] respectively overprint or
    // preserve the pending glyph according to their actual output order.
    assert_eq!(inline_text(children), "AXB [AXB] [A]");
}

#[test]
fn zero_advance_crosses_leading_scopes_generated_prefixes_and_link_labels() {
    let cases = [
        (
            "leading-word",
            b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\n\\zX\nB\n".as_slice(),
            "XB",
        ),
        (
            "man-font-scope",
            b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\nA\\zX\n.B B\n".as_slice(),
            "AXB",
        ),
        (
            "optional-arguments",
            b".TH ZERO-ADVANCE 1\n.SH DESCRIPTION\nA\\zX\n.OP B C\n".as_slice(),
            "AX[B C]",
        ),
        (
            "mdoc-prefix",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX Fl b\n".as_slice(),
            "AX-b",
        ),
        (
            "link-label",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX Lk https://example.org B\n".as_slice(),
            "AXB",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-{label}.1")),
            source,
        )
        .expect("parse cross-scope zero-advance fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.sections[0].blocks
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        if label == "link-label" {
            assert!(
                matches!(children.as_slice(), [Inline::Text { value: prefix }, Inline::Text { value: glyph }, Inline::Link { .. }]
                    if prefix == "A" && glyph == "X"),
                "the pending glyph must precede the atomically lowered link: {children:?}"
            );
        }
    }
}

#[test]
fn declaration_witnesses_close_on_unclassified_bodies_and_survive_split_macro_lists() {
    let body_closed = parse_manual_bytes(
        std::path::Path::new("declaration-body-closure.1"),
        b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B \"This is explanatory prose.\"\nOWN DESCRIPTION.\n.TP\n.B --alpha\n.TP\n.B --beta\nSHARED DESCRIPTION.\n",
    )
    .expect("parse an unclassified definition with its own body");
    let [
        Block::DefinitionList {
            items,
            declaration_groups,
            ..
        },
    ] = body_closed.sections[0].blocks.as_slice()
    else {
        panic!(
            "expected one definition list: {:#?}",
            body_closed.sections[0].blocks
        );
    };
    assert_eq!(items.len(), 3);
    assert_eq!(
        declaration_groups,
        &[mant_ir::DeclarationGroup {
            start_item: 1,
            end_item: 3,
        }],
        "the prose owner's own body closes its physical run"
    );

    let split_macro = parse_manual_bytes(
        std::path::Path::new("declaration-split-macro.1"),
        b".TH PROBE 1\n.SH OPTIONS\n.de XX\n.TP\n.B --alpha\n.TP\n.B --beta\nSHARED DESCRIPTION.\n..\n.XX\n.PP\nSEPARATOR.\n.XX\n",
    )
    .expect("parse two macro-expanded declaration lists");
    let lists = split_macro.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList {
                items,
                declaration_groups,
                ..
            } => Some((items, declaration_groups)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 2, "the paragraph splits physical lists");
    for (items, declaration_groups) in lists {
        assert_eq!(
            items
                .iter()
                .map(|item| inline_text(&item.terms[0]))
                .collect::<Vec<_>>(),
            ["--alpha", "--beta"]
        );
        assert_eq!(
            declaration_groups,
            &[mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }],
            "each complete expansion retains its own shared description"
        );
    }
    assert!(
        !format!("{split_macro:?}").contains("mant-native-definition-owner"),
        "parse-local owner markers must be removed before public IR escapes"
    );
}

#[test]
fn declaration_witnesses_keep_tq_groups_across_repeated_macro_expansions() {
    let repeated_tq = parse_manual_bytes(
        std::path::Path::new("declaration-repeated-tq-macro.1"),
        b".TH PROBE 1\n.SH OPTIONS\n.de XX\n.TP\n.B -a\n.TQ\n.B --alpha\n.TP\n.B --beta\nSHARED BODY.\n.PP\nSEPARATOR.\n.TP\n.B -a\n.TQ\n.B --alpha\n.TP\n.B --beta\nSHARED BODY.\n..\n.XX\n",
    )
    .expect("parse repeated TQ macro expansion");
    let lists = repeated_tq.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList {
                items,
                declaration_groups,
                ..
            } => Some((items, declaration_groups)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 2, "the macro contains two physical runs");
    for (items, declaration_groups) in lists {
        assert_eq!(
            items
                .iter()
                .map(|item| item
                    .terms
                    .iter()
                    .map(|term| inline_text(term))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![
                vec!["-a".to_owned(), "--alpha".to_owned()],
                vec!["--beta".to_owned()],
            ]
        );
        assert_eq!(
            declaration_groups,
            &[mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }],
            "each repeated TQ expansion retains its own shared declaration body"
        );
    }
    assert_eq!(
        visible_document_text(&repeated_tq)
            .matches("SHARED BODY.")
            .count(),
        2,
        "the trailing description remains with each repeated declaration run"
    );
}

#[test]
fn zero_advance_crosses_empty_enclosures_and_atomic_mdoc_output() {
    let cases = [
        (
            "empty-enclosure",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\n.Dq\n.No B\n".as_slice(),
            "AX“” B",
        ),
        (
            "include",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.In A\\zX\n".as_slice(),
            "<A>",
        ),
        (
            "bsd",
            b".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.Bx A\\zX\n".as_slice(),
            "ABSD",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-{label}.1")),
            source,
        )
        .expect("parse zero-advance atomic output");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!(
                "{label}: expected one paragraph: {:#?}",
                document.sections[0].blocks
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn bsd_reference_executes_suppressed_font_operands_before_generated_text() {
    let document = parse_manual_bytes(
        std::path::Path::new("bsd-reference-font-state.1"),
        b".Dd September 12, 2026\n.Dt BSD-FONT 1\n.Os\n.Sh DESCRIPTION\n.Bx \\fB\n.Li \\fPZ\n.Bx \\fB-devel\n.Li \\fPZ\n",
    )
    .expect("parse Bx formatter-state fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections[0].blocks);
    };

    // CVS mdoc_validate.c appends generated BSD after source argument
    // processing. A control-only operand remains executable even though the
    // semantic lifecycle form replaces its visible spelling.
    assert_eq!(
        children,
        &[
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "BSD".to_owned(),
                }],
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "Z".to_owned(),
                }],
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "BSD (currently under development)".to_owned(),
                }],
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "Z".to_owned(),
                }],
            },
        ],
        "suppressed Bx operands must retain their font transitions"
    );
}

#[test]
fn bsd_reference_replacements_execute_complete_hidden_word_state() {
    for (label, operand, expected) in [
        (
            "lifecycle-word-end-break",
            r"-alpha\p",
            "BSD (currently in alpha test)\nAFTER",
        ),
        ("control-only-word-end-break", r"\p", "BSD\nAFTER"),
        (
            "lifecycle-source-continuation",
            r"-beta\c",
            "BSD (currently in beta test)AFTER",
        ),
        (
            "noncanonical-lifecycle-zero-advance",
            r"-devel\zX",
            "-develBSD AFTER",
        ),
        ("control-only-zero-advance", r"\z", "SD AFTER"),
        ("completed-zero-advance", r"\zX", "BSD AFTER"),
        ("canceled-zero-advance", r"\z\c", "BSD AFTER"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt BSD-STATE 1\n.Os\n.Sh DESCRIPTION\n.Bx {operand}\n.No AFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("bsd-reference-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse Bx hidden execution fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}");
    }

    let document = parse_manual_bytes(
        std::path::Path::new("bsd-reference-hidden-font-break.1"),
        b".Dd September 12, 2026\n.Dt BSD-STATE 1\n.Os\n.Sh DESCRIPTION\n.Bx \\fB-devel\\p\n.Li \\fPZ\n",
    )
    .expect("parse styled Bx hidden execution fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections);
    };
    assert_eq!(
        inline_text(children),
        "BSD (currently under development)\nZ"
    );
    assert!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::Strong { .. }))
            .count()
            >= 2,
        "hidden font execution must style replacement and follower: {children:?}"
    );
}

#[test]
fn bsd_reference_replacement_preserves_boundaries_and_exact_arity() {
    let document = parse_manual_bytes(
        std::path::Path::new("bsd-reference-replacement-boundaries.1"),
        b".Dd September 12, 2026\n.Dt BSD-STATE 1\n.Os\n.Sh DESCRIPTION\n.No A\n.Bx \\fB\n.Li \\fPZ\n.No A\n.Bx \\p\n.No Z\n.Bx -alpha \"\"\n.Bx 4.3 Tahoe\n",
    )
    .expect("parse Bx replacement boundary fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections);
    };

    assert_eq!(
        inline_text(children),
        "A BSD Z A BSD\nZ -alphaBSD 4.3BSD Tahoe"
    );
    assert!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::Strong { .. }))
            .count()
            >= 2,
        "control-only Bx operands must retain font execution: {children:?}"
    );
}

#[test]
fn mail_and_link_labels_share_the_zero_advance_stream() {
    let cases = [
        (
            "mail-address",
            b".Dd September 12, 2026\n.Dt ZERO-LINK 1\n.Os\n.Sh DESCRIPTION\n.No a Mt b@example.org\\zX Ns c\n".as_slice(),
            "a b@example.orgc",
        ),
        (
            "link-label",
            b".Dd September 12, 2026\n.Dt ZERO-LINK 1\n.Os\n.Sh DESCRIPTION\n.No a Lk https://example.org b\\zX Ns c\n".as_slice(),
            "a bc",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-{label}.1")),
            source,
        )
        .expect("parse link formatter-state fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph");
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert!(
            children
                .iter()
                .any(|inline| matches!(inline, Inline::Link { .. })),
            "{label}: preserving formatter state must not discard link identity"
        );
        let target = children.iter().find_map(|inline| match inline {
            Inline::Link { target, .. } => Some(target),
            _ => None,
        });
        match label {
            "mail-address" => assert!(
                matches!(target, Some(mant_ir::LinkTarget::Email { address }) if address == "b@example.org"),
                "{label}: wrong email target: {target:?}"
            ),
            "link-label" => assert!(
                matches!(target, Some(mant_ir::LinkTarget::External { uri }) if uri == "https://example.org"),
                "{label}: wrong external target: {target:?}"
            ),
            _ => unreachable!("unrecognized zero-advance case"),
        }
    }
}

#[test]
fn semantic_links_execute_hidden_operands_and_preserve_empty_label_fallbacks() {
    let cases = [
        (
            "link-zero-width-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org A\\zX\n.No B\n".as_slice(),
            "A B",
        ),
        (
            "empty-link-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \"\"\n.No B\n".as_slice(),
            "https://example.org B",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-{label}.1")),
            source,
        )
        .expect("parse semantic link formatter-state fixture");
        let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
            panic!(
                "{label}: expected one description paragraph: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert!(
            children.iter().any(|inline| {
                matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. } if uri == "https://example.org")
            }),
            "{label}: visible text must retain the typed external target"
        );
    }

    let link_controls = parse_manual_bytes(
        std::path::Path::new("semantic-link-hidden-uri-font.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk \\fBhttps://example.org label\n.Li \\fPZ\n",
    )
    .expect("parse hidden link URI controls");
    let [Block::Paragraph { children, .. }] = link_controls.sections[1].blocks.as_slice() else {
        panic!(
            "expected one description paragraph: {:#?}",
            link_controls.sections
        );
    };
    assert!(
        matches!(children.last(), Some(Inline::Strong { children }) if inline_text(children) == "Z"),
        "controls hidden with the URI must still affect following siblings: {children:?}"
    );

    let mail_controls = parse_manual_bytes(
        std::path::Path::new("semantic-mail-hidden-font.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Mt \\fB a@example.org\n.Li \\fPZ\n",
    )
    .expect("parse hidden mail operand controls");
    let [Block::Paragraph { children, .. }] = mail_controls.sections[1].blocks.as_slice() else {
        panic!(
            "expected one description paragraph: {:#?}",
            mail_controls.sections
        );
    };
    assert!(
        children.iter().any(|inline| {
            matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Email { address }, children, .. }
                if address == "a@example.org"
                    && matches!(children.as_slice(), [Inline::Strong { children }] if inline_text(children) == "a@example.org"))
        }),
        "a control-only Mt operand must set the address font: {children:?}"
    );
}

#[test]
fn semantic_links_choose_visible_output_after_executing_operands() {
    let cases = [
        (
            "hidden-uri-zero-width",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org/\\zXY label\n.No AFTER\n".as_slice(),
            "label AFTER",
        ),
        (
            "mail-zero-width",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Mt \\zX a@example.org\n.No AFTER\n".as_slice(),
            "a@example.org AFTER",
        ),
        (
            "projected-empty-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\zX\n.No AFTER\n".as_slice(),
            "https://example.org AFTER",
        ),
        (
            "empty-target-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk \\fB label\n.Li \\fPZ\n".as_slice(),
            "label Z",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-execution-{label}.1")),
            source,
        )
        .expect("parse semantic link execution fixture");
        let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
            panic!(
                "{label}: expected one description paragraph: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert!(
            !children.iter().any(|inline| {
                matches!(inline, Inline::Link { children, .. } if children.is_empty())
            }),
            "{label}: visible source must not leave an empty link: {children:?}"
        );
        match label {
            "mail-zero-width" => assert!(
                children.iter().any(|inline| {
                    matches!(inline, Inline::Link { target: mant_ir::LinkTarget::Email { address }, .. }
                        if address == "a@example.org")
                }),
                "{label}: the recovered address must retain its email target: {children:?}"
            ),
            "hidden-uri-zero-width" | "projected-empty-label" => assert!(
                children.iter().any(|inline| {
                    matches!(inline, Inline::Link { target: mant_ir::LinkTarget::External { uri }, .. }
                        if uri == "https://example.org/Y" || uri == "https://example.org")
                }),
                "{label}: the visible link must retain its external target: {children:?}"
            ),
            "empty-target-label" => {
                assert!(
                    !children.iter().any(|inline| matches!(inline, Inline::Link { .. })),
                    "{label}: an empty target must degrade to ordinary text: {children:?}"
                );
                assert!(
                    matches!(children.last(), Some(Inline::Strong { children }) if inline_text(children) == "Z"),
                    "{label}: hidden target controls must still affect later siblings: {children:?}"
                );
            }
            _ => unreachable!("unrecognized semantic link case"),
        }
    }
}

#[test]
fn control_only_link_labels_keep_their_structural_font_scope() {
    let document = parse_manual_bytes(
        std::path::Path::new("semantic-link-control-only-label.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\fB\n.Li \\fPZ\n",
    )
    .expect("parse control-only Lk label");
    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!(
            "expected one description paragraph: {:#?}",
            document.sections
        );
    };
    assert_eq!(
        inline_text(children),
        "https://example.org Z",
        "{children:?}"
    );
    assert!(
        matches!(children.last(), Some(Inline::Text { value }) if value == "Z"),
        "the Lk scope must not turn the URI or following Z bold: {children:?}"
    );
}

#[test]
fn semantic_links_execute_hidden_word_boundaries_and_native_delimiters() {
    let cases = [
        (
            "label-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org label\\c\n.No AFTER\n".as_slice(),
            "label AFTER",
        ),
        (
            "uri-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org\\c label\n.No AFTER\n".as_slice(),
            "labelAFTER",
        ),
        (
            "escaped-punctuation-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\fB.\n.Li \\fPZ\n".as_slice(),
            ". Z",
        ),
        (
            "bare-closing-punctuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org .\n.Li Z\n".as_slice(),
            "https://example.org. Z",
        ),
        (
            "zero-width-punctuation-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \\&.\n.Li Z\n".as_slice(),
            ". Z",
        ),
        (
            "bare-hidden-zero-advance",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org/\\z label\n.No AFTER\n".as_slice(),
            "label FTER",
        ),
        (
            "whitespace-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.org \" \"\n.No AFTER\n".as_slice(),
            "https://example.org AFTER",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-boundary-{label}.1")),
            source,
        )
        .expect("parse semantic link boundary fixture");
        let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
            panic!(
                "{label}: expected one description paragraph: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert!(
            !children.iter().any(|inline| {
                matches!(inline, Inline::Link { children, .. } if children.is_empty())
            }),
            "{label}: semantic link output must not be empty: {children:?}"
        );
        if label == "escaped-punctuation-label" {
            assert!(
                matches!(children.last(), Some(Inline::Text { value }) if value == "Z"),
                "{label}: an authored label font scope must not leak: {children:?}"
            );
        }
    }
}

#[test]
fn semantic_link_continuations_preserve_literal_rows() {
    for (label, source, expected) in [
        (
            "label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bd -literal\n.Lk https://example.org label\\c\n.No AFTER\n.Ed\n".as_slice(),
            "label\nAFTER",
        ),
        (
            "uri",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bd -literal\n.Lk https://example.org\\c label\n.No AFTER\n.Ed\n".as_slice(),
            "labelAFTER",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-literal-continuation-{label}.1")),
            source,
        )
        .expect("parse literal semantic link continuation");
        let [Block::Preformatted { children, .. }] = document.sections[1].blocks.as_slice() else {
            panic!("{label}: expected one literal display: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn semantic_link_compaction_preserves_layout_and_final_execution_boundaries() {
    for (label, source, expected) in [
        (
            "empty-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \"\"\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\nhttps://example.org\nAFTER",
        ),
        (
            "control-only-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \\fB\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\nhttps://example.org\nAFTER",
        ),
        (
            "zero-width-label-row",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\n.Lk https://example.org \\zX\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\nhttps://example.org\nAFTER",
        ),
        (
            "hidden-target-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org\\p label\n.No AFTER\n".as_slice(),
            "label\nAFTER",
        ),
        (
            "trailing-punctuation-consumes-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org\\c label .\n.No AFTER\n".as_slice(),
            "label. AFTER",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-link-layout-{label}.1")),
            source,
        )
        .expect("parse semantic link layout fixture");
        let blocks = &document.sections[0].blocks;
        let [block] = blocks.as_slice() else {
            panic!("{label}: expected one flow block: {blocks:#?}");
        };
        let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = block else {
            panic!("{label}: expected one flow block: {blocks:#?}");
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn empty_operands_are_words_before_generated_semantic_punctuation() {
    for (label, source, expected) in [
        (
            "optional-argument",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Op A\\zX \"\"\n.No AFTER\n".as_slice(),
            "[AX] AFTER",
        ),
        (
            "link-label",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org \\zX \"\"\n.No AFTER\n".as_slice(),
            "X AFTER",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("semantic-empty-word-{label}.1")),
            source,
        )
        .expect("parse semantic empty word fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        if label == "link-label" {
            assert!(
                matches!(children.first(), Some(Inline::Link { children, .. }) if inline_text(children) == "X"),
                "{label}: the resolved label must remain a visible link: {children:?}"
            );
        }
    }
}

#[test]
fn inline_execution_keeps_word_joins_glyph_ownership_and_literal_breaks_distinct() {
    // CVS `term_word()` carries `TERMP_BACKAFTER`, `TERMP_NOSPACE`, and an
    // emitted `\\p` break as independent state.  These cases deliberately
    // cross semantic wrappers because they are where a flattened AST tail is
    // most tempting (and wrong) to use as a replacement for execution order.
    for (label, source, expected) in [
        (
            "enclosure-closes-a-source-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\\c\n.Ec\n.No AFTER\n".as_slice(),
            "[A AFTER",
        ),
        (
            "include-closes-a-source-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.In stdio.h\\c\n.No AFTER\n".as_slice(),
            "<stdio.h> AFTER",
        ),
        (
            "no-space-cancels-a-bare-zero-advance",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\z\\c\n.No AFTER\n".as_slice(),
            "BEFORE AFTER",
        ),
        (
            "hidden-empty-mail-operand-cannot-own-a-prior-glyph",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt a@example.org\\zX \"\"\n.No AFTER\n".as_slice(),
            "a@example.orgX AFTER",
        ),
        (
            "literal-explicit-break-closes-the-source-row-once",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No BEFORE\\p\n.No AFTER\n.Ed\n".as_slice(),
            "BEFORE\nAFTER",
        ),
        (
            "hidden-uri-explicit-break-closes-the-source-row-once",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Lk https://example.org\\p label\n.No AFTER\n.Ed\n".as_slice(),
            "label\nAFTER",
        ),
        (
            "literal-enclosure-retains-formatter-and-authored-blanks",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.Eo [\n.No BEFORE\\c\n.Ec\n AFTER\n.Ed\n".as_slice(),
            "[\nBEFORE  AFTER",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-execution-{label}.1")),
            source,
        )
        .expect("parse inline execution boundary fixture");
        let blocks = &document.sections[0].blocks;
        let [block] = blocks.as_slice() else {
            panic!("{label}: expected one flow block: {blocks:#?}");
        };
        let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = block else {
            panic!("{label}: expected flow content: {blocks:#?}");
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn inline_execution_tracks_zero_advance_word_and_physical_line_states_independently() {
    let cases = [
        (
            "completed-zero-advance-survives-no-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\zX\\c\n.No AFTER\n".as_slice(),
            "BEFOREAFTER",
        ),
        (
            "later-no-space-remains-effective",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\z\\c\\c\n.No AFTER\n".as_slice(),
            "BEFOREAFTER",
        ),
        (
            "armed-and-pending-zero-advance-stages-coexist",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\zX\\z\\c\n.No AFTER\n".as_slice(),
            "BEFOREXAFTER",
        ),
        (
            "word-end-break-waits-for-the-complete-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\pTAIL\n.No AFTER\n".as_slice(),
            "BEFORETAIL\nAFTER",
        ),
        (
            "word-end-break-crosses-tight-ns-boundary",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\pB Ns C\n.No D\n".as_slice(),
            "ABC\nD",
        ),
        (
            "word-end-break-realizes-inside-a-later-tight-text-node",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p Ns No \"B C\"\n.No D\n".as_slice(),
            "AB\nC D",
        ),
        (
            "word-end-break-crosses-a-generated-tight-prefix-before-an-inner-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p Ns Fl \"B C\"\n.No D\n".as_slice(),
            "A-B\nC D",
        ),
        (
            "disabled-spacing-defers-word-end-break-to-the-next-inner-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.No A\\p No \"B C\"\n.Sm on\n.No D\n".as_slice(),
            "AB\nC D",
        ),
        (
            "word-end-break-stops-at-intra-operand-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB C\"\n.No D\n".as_slice(),
            "AB\nC D",
        ),
        (
            "word-end-break-and-trailing-continuation-remain-orthogonal",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\p B\\c\"\n.No C\n".as_slice(),
            "A\nBC",
        ),
        (
            "intra-word-break-does-not-cancel-a-later-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB C\\c\"\n.No D\n".as_slice(),
            "AB\nCD",
        ),
        (
            "word-end-break-crosses-unpaddable-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB\\ C\"\n.No D\n".as_slice(),
            "AB C\nD",
        ),
        (
            "word-end-break-crosses-nonbreaking-space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB\\~C\"\n.No D\n".as_slice(),
            "AB C\nD",
        ),
        (
            "word-end-break-crosses_fixed-width_space",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB\\0C\"\n.No D\n".as_slice(),
            "AB C\nD",
        ),
        (
            "projected-glyph-ends-only-the-consumed-break-whitespace-run",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\(em C\"\n".as_slice(),
            "AB\n— C",
        ),
        (
            "nonbreaking-glyph-preserves-the-following-ordinary-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\~ C\"\n".as_slice(),
            "AB\n  C",
        ),
        (
            "fixed-width-glyph-preserves-the-following-ordinary-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\0 C\"\n".as_slice(),
            "AB\n  C",
        ),
        (
            "overstrike-glyph-preserves-the-following-ordinary-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\o'XY' C\"\n".as_slice(),
            "AB\nY C",
        ),
        (
            "device-glyph-preserves-the-following-ordinary-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\*[.T] C\"\n".as_slice(),
            "AB\nutf8 C",
        ),
        (
            "font-state-does-not-end-consumed-break-whitespace",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\pB \\fB C\"\n".as_slice(),
            "AB\nC",
        ),
        (
            "repeated-word-end-break-is-idempotent",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\\pB\n.No C\n".as_slice(),
            "AB\nC",
        ),
        (
            "word-end-break-and-zero-advance-remain-orthogonal",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\\zX Ns B\n.No C\n".as_slice(),
            "AB\nC",
        ),
        (
            "zero-advance-glyph-defers-a-trailing-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\\p\n.No D\n.No C\n".as_slice(),
            "AXD\nC",
        ),
        (
            "word-end-break-before-zero-advance-survives-the-settling-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\\zX\n.No B\n.No C\n".as_slice(),
            "AXB\nC",
        ),
        (
            "empty-word-settles-zero-advance-before-realizing-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\\p\n.No \"\"\n.No B\n.No C\n".as_slice(),
            "AX\nB C",
        ),
        (
            "empty-word-realizes-word-end-break-with-one-following-boundary",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\n.No \"\"\n.No B\n.No C\n".as_slice(),
            "A\n B C",
        ),
        (
            "zero-advance-word-blank-defers-the-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \"A\\zX\\p B\"\n.No D\n".as_slice(),
            "AXB\nD",
        ),
        (
            "zero-advance-overwrite-keeps-the-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\\pB\n.No D\n".as_slice(),
            "AB\nD",
        ),
        (
            "enclosure-releases-spacing-not-physical-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No BEFORE\\c\n.Ec\n AFTER\n".as_slice(),
            "[BEFORE  AFTER",
        ),
        (
            "styled-word-retains-formatter-and-authored-blanks",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No BEFORE\\c\n.Ec\n.No \" AFTER\"\n".as_slice(),
            "[BEFORE  AFTER",
        ),
        (
            "explicit-empty-es-close-is-a-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Es \"\" \"\"\n.En BEFORE\\c\n.No AFTER\n".as_slice(),
            "BEFORE AFTER",
        ),
        (
            "missing-es-close-releases-word-spacing",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Es \"\"\n.En BEFORE\\c\n.No AFTER\n".as_slice(),
            "BEFORE AFTER",
        ),
        (
            "en-without-es-releases-word-spacing",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.En BEFORE\\c\n.No AFTER\n".as_slice(),
            "BEFORE AFTER",
        ),
        (
            "generated-close-consumes-physical-continuation",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Op BEFORE\\c\n AFTER\n".as_slice(),
            "[BEFORE]\n AFTER",
        ),
        (
            "man-font-scope-carries-word-end-break",
            b".TH PROBE 1\n.SH DESCRIPTION\n.B A\\p\nB\n".as_slice(),
            "A\nB",
        ),
        (
            "alternating-man-font-scope-carries-word-end-break",
            b".TH PROBE 1\n.SH DESCRIPTION\n.BR A\\p B\nC\n".as_slice(),
            "AB\nC",
        ),
        (
            "alternating-man-font-scope-realizes-word-end-break-inside-an-operand",
            b".TH PROBE 1\n.SH DESCRIPTION\n.BR A\\p \"B C\"\nD\n".as_slice(),
            "AB\nC D",
        ),
        (
            "man-op-keeps-operands-before-word-end-break",
            b".TH PROBE 1\n.SH DESCRIPTION\n.OP A\\p B\nC\n".as_slice(),
            "[A B]\nC",
        ),
        (
            "man-op-no-space-suppresses-the-kept-operand-boundary",
            b".TH PROBE 1\n.SH DESCRIPTION\n.OP \"A\\c\" C\nD\n".as_slice(),
            "[AC] D",
        ),
        (
            "man-op-no-space-after-word-end-break-keeps-the-second-operand",
            b".TH PROBE 1\n.SH DESCRIPTION\n.OP \"A\\p B\\c\" C\nD\n".as_slice(),
            "[A\nBC] D",
        ),
        (
            "man-op-empty-kept-operand-consumes-no-space-without-padding",
            b".TH PROBE 1\n.SH DESCRIPTION\n.OP \"A\\p B\\c\" \"\"\nD\n".as_slice(),
            "[A\nB] D",
        ),
        (
            "include-scope-carries-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.In stdio.h\\p\n.No AFTER\n".as_slice(),
            "<stdio.h>\nAFTER",
        ),
        (
            "mdoc-keep-group-defers-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p No B\n.Ek\n.No C\n".as_slice(),
            "A B\nC",
        ),
        (
            "mdoc-keep-group-respects-source-line-end",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p\n.No B\n.Ek\n.No C\n".as_slice(),
            "A\nB C",
        ),
        (
            "macro-expanded-continuation-retains-authored-blank",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.de X\n.Eo [\n.No BEFORE\\c\n.Ec\n.No \" AFTER\"\n..\n.Sh DESCRIPTION\n.X\n".as_slice(),
            "[BEFORE  AFTER",
        ),
    ];
    for (label, source, expected) in cases {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-state-{label}.1")),
            source,
        )
        .expect("parse inline execution-state fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn private_and_keep_scopes_preserve_incoming_word_end_execution_state() {
    for (label, source, expected) in [
        (
            "tight-include",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p Ns In stdio.h\n.No AFTER\n".as_slice(),
            "A<stdio.h>\nAFTER",
        ),
        (
            "include-realizes-incoming-break-at-its-interior-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p Ns In \"B C\"\n.No AFTER\n".as_slice(),
            "A<B\nC> AFTER",
        ),
        (
            "keep-prephase-does-not-swallow-an-incoming-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\\p\n.Bk -words\n.No A No B\n.Ek\n.No C\n".as_slice(),
            "BEFORE\nA B C",
        ),
        (
            "macro-expanded-keep-observes-each-executed-line",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.de XX\n.No A\\p\n.No B\n..\n.Sh DESCRIPTION\n.Bk -words\n.XX\n.Ek\n.No C\n".as_slice(),
            "A\nB C",
        ),
        (
            "alternating-man-scope-receives-an-incoming-word-end-break",
            b".TH PROBE 1\n.SH DESCRIPTION\nA\\p\n.BR \"B C\" D\nAFTER\n".as_slice(),
            "A\nB CD AFTER",
        ),
        (
            "kept-include",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p In stdio.h\n.Ek\n.No AFTER\n".as_slice(),
            "A <stdio.h>\nAFTER",
        ),
        (
            "same-line-keep-without-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A No B\n.Ek\n.No C\n".as_slice(),
            "A B C",
        ),
        (
            "cross-line-keep-without-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\n.No B\n.Ek\n.No C\n".as_slice(),
            "A B C",
        ),
        (
            "keep-boundary-settles-a-zero-advance-glyph",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\zX\n.No B\n.Ek\n.No C\n".as_slice(),
            "AXB C",
        ),
        (
            "empty-keep-word-settles-zero-advance-before-the-next-word",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\zX\n.No \"\"\n.No B\n.Ek\n.No C\n".as_slice(),
            "AX B C",
        ),
        (
            "empty-keep-word-realizes-word-end-break-with-one-following-boundary",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p\n.No \"\"\n.No B\n.Ek\n.No C\n".as_slice(),
            "A\n B C",
        ),
        (
            "keep-boundary-defers-word-end-break-after-settling-zero-advance",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\zX\\p\n.No B\n.Ek\n.No C\n".as_slice(),
            "AXB\nC",
        ),
        (
            "inner-keep-close-clears-the-global-outer-state",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\n.Bk -words\n.No B\n.Ek\n.No E\\p No \"F G\"\n.Ek\n".as_slice(),
            "A B E\nF G",
        ),
        (
            "target-does-not-realize-a-pending-word-end-break",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\p\n.Tg word-break-mark\n.No B C\n".as_slice(),
            // `.Tg` emits no formatter word, so the pending break survives
            // until the next `.No` begins its first word.  This is the fixed
            // CVS `term_word()` order, not an internal-space heuristic.
            "A\nB C",
        ),
        (
            "target-does-not-cover-a-pending-zero-advance-glyph",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\n.Tg zero-mark\n.No B\n".as_slice(),
            "AXB",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-private-state-{label}.1")),
            source,
        )
        .expect("parse private inline execution fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn keep_words_survives_a_paragraph_output_flush() {
    let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bk -words\n.No A\\p\n.Pp\n.No B\n.Ek\n.No C\n";
    let document = parse_manual_bytes(
        std::path::Path::new("inline-private-state-keep-paragraph.1"),
        source,
    )
    .expect("parse keep paragraph fixture");

    let [
        Block::Paragraph {
            children: first, ..
        },
        Block::VerticalSpace { lines: 1, .. },
        Block::Paragraph {
            children: second, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!(
            "expected two paragraphs separated by one row: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(first), "A");
    assert_eq!(inline_text(second), "B C");
}

#[test]
fn keep_words_tracks_repeated_macro_execution_and_transparent_targets() {
    for (label, macro_body, expected) in [
        ("repeated-lines", ".No A\\p\n.No B", "A\nB A\nB C"),
        (
            "target-between-lines",
            ".No A\\p\n.Tg inside-macro\n.No B",
            "A\nB A\nB C",
        ),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.de XX\n{macro_body}\n..\n.Sh DESCRIPTION\n.Bk -words\n.XX\n.XX\n.Ek\n.No C\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-keep-macro-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse repeated keep macro fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        if label == "target-between-lines" {
            assert!(
                children
                    .iter()
                    .filter(
                        |inline| matches!(inline, Inline::Anchor { id, .. } if id == "inside-macro")
                    )
                    .count()
                    >= 1,
                "{children:?}"
            );
        }
    }
}

#[test]
fn transparent_target_preserves_continuation_and_its_exact_anchor() {
    let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No BEFORE\\c\n.Ec\n.Tg mark\n AFTER\n";
    let document = parse_manual_bytes(
        std::path::Path::new("inline-state-transparent-target.1"),
        source,
    )
    .expect("parse transparent target fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one paragraph: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "[BEFORE  AFTER");
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Anchor { id, .. } if id == "mark")),
        "{children:?}"
    );
}

#[test]
fn overstrike_projects_one_terminal_cell_through_the_shared_zero_advance_state() {
    for (label, source, expected) in [
        ("ordinary", r"A\o'BC'D", "ACD"),
        ("trailing-blank", r"A\o'BC 'D", "ACD"),
        ("repeated-trailing-blanks", r"A\o'BC  'D", "ACD"),
        ("trailing-tab", "A\\o'BC\t'D", "ACD"),
        ("all-blanks", r"A\o'   'D", "AD"),
        ("zero-advance", r"A\z\o'BC'D", "AD"),
        ("zero-advance-trailing-blank-at-end", r"A\z\o'BC '", "AC"),
        ("empty", r"A\o''D", "AD"),
        ("nested-escape-spelling", r"A\o'BC\fI'D", "AID"),
        ("same-delimiter-numbered-escape", r"A\o'BC\N'8''D", "A'D"),
        ("same-delimiter-motion-escape", r"A\o'BC\h'1n''D", "A'D"),
        ("same-delimiter-character-escape", r"A\o'BC\C'x''D", "A'D"),
    ] {
        let manual =
            format!(".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {source}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-overstrike-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse overstrike projection fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn styled_overstrike_uses_the_complete_nested_escape_extent() {
    for (label, source, expected) in [
        ("quoted-number", r"A\o'BC\N'8''D", "A'D"),
        ("standard-nested-argument", r"A\o'1\f\N'39'2'B", "A2B"),
    ] {
        let manual =
            format!(".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Sy {source}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-overstrike-nested-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse styled nested overstrike fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
        assert!(
            children.iter().any(
                |inline| matches!(inline, Inline::Strong { children } if inline_text(children) == expected)
            ),
            "{label}: styled projection escaped its semantic scope: {children:?}"
        );
    }
}

#[test]
fn deeply_nested_escape_arguments_remain_bounded_at_the_native_boundary() {
    let mut source =
        String::from(".Dd September 12, 2026\n.Dt ESCAPE-DEPTH 1\n.Os\n.Sh DESCRIPTION\n.No ");
    source.push_str(&"\\o'".repeat(512));
    source.push('X');
    source.push_str(&"'".repeat(512));
    source.push_str("\n.No AFTER\n");

    let document = parse_manual_bytes(
        std::path::Path::new("deep-native-escape.1"),
        source.as_bytes(),
    )
    .expect("native escape depth exhaustion must remain a finite lowering");
    let text = visible_document_text(&document);
    assert!(text.contains("AFTER"), "document tail was lost: {text:?}");
    assert!(
        text.len() < source.len(),
        "rejected nesting must not be expanded into unbounded output"
    );
}

#[test]
fn numbered_and_named_nonbreaking_glyphs_defer_word_end_breaks() {
    for (label, spelling, expected_glyph) in [
        ("numbered-tab", r"\N'9'", "\t"),
        ("numbered-nbsp", r"\N'160'", "\u{a0}"),
        ("unicode-nbsp", "\u{a0}", "\u{a0}"),
        ("named-unicode-nbsp", r"\[u00A0]", "\u{a0}"),
        ("roff-nbsp", r"\~", " "),
        ("roff-digit-width-space", r"\0", " "),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\pB{spelling}C\n.No D\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-numbered-glyph-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse numbered glyph break fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(
            inline_text(children),
            format!("AB{expected_glyph}C\nD"),
            "{label}: {children:?}"
        );
    }

    for (number, expected) in [("0", "�"), ("10", "�"), ("27", "�"), ("255", "ÿ")] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\\pB\\N'{number}'C\n.No D\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-numbered-control-{number}.1")),
            manual.as_bytes(),
        )
        .expect("parse numbered terminal glyph fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{number}: expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), format!("AB{expected}C\nD"));
    }
}

#[test]
fn literal_flow_uses_the_same_numbered_and_overstrike_glyph_projection() {
    for (label, source, expected) in [
        ("numbered-nbsp", r"A\pB\N'160'C", "AB\u{a0}C"),
        ("overstrike-trailing-blank", r"A\o'BC 'D", "ACD"),
        ("zero-advance-before-word-break", r"A\zX\p", "AX"),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No {source}\n.No D\n.Ed\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-literal-glyph-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse literal formatter glyph fixture");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!(
                "{label}: expected one preformatted block: {:#?}",
                document.sections
            );
        };
        let expected = format!("{expected}\nD");
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn man_no_fill_settles_an_unrealized_word_end_break_at_the_physical_line() {
    for (source, expected_first_row) in [
        (r"PLAIN:A\pB", "PLAIN:AB"),
        (r"PLAIN:A\pB\N'160'C", "PLAIN:AB\u{a0}C"),
        (r"PLAIN:A\pB\N'9'C", "PLAIN:AB\tC"),
        (r"PLAIN:A\zX\p", "PLAIN:AX"),
    ] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\n{source}\nNEXT\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new("inline-man-no-fill-word-end-break.1"),
            manual.as_bytes(),
        )
        .expect("parse man no-fill word-end break fixture");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected one preformatted block: {:#?}", document.sections);
        };
        assert_eq!(
            inline_text(children).matches('\n').count(),
            1,
            "{source}: {children:?}"
        );
        assert_eq!(
            inline_text(children),
            format!("{expected_first_row}\nNEXT"),
            "{source}: {children:?}"
        );
    }
}

#[test]
fn zero_advance_state_crosses_only_continued_no_fill_rows() {
    for (label, manual, expected) in [
        (
            "man",
            ".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\nB\n.fi\n",
            "AB",
        ),
        (
            "mdoc",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No A\\zX\\c\n.No B\n.Ed\n",
            "AB",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-{label}-continued-zero-advance.1")),
            manual.as_bytes(),
        )
        .expect("parse continued no-fill zero-advance fixture");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected one preformatted block: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-continued-word-end-break.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\p\\c\nB\n.fi\n",
    )
    .expect("parse continued no-fill word-end-break fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "AB", "{children:?}");
}

#[test]
fn no_fill_execution_state_crosses_transparent_requests_only() {
    for (label, request) in [("font", ".ft B"), ("paragraph-distance", ".PD 1")] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n{request}\nB\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-no-fill-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse transparent no-fill request fixture");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected one preformatted block: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), "AB", "{label}: {children:?}");
        if label == "font" {
            assert!(children.iter().any(|inline| matches!(
                inline,
                Inline::Strong { children } if inline_text(children) == "B"
            )));
        }
    }

    for (label, request) in [
        ("indent", ".in 4"),
        ("temporary-indent", ".ti 4"),
        ("break", ".br"),
        ("space", ".sp"),
        ("fill", ".fi\n.nf"),
        ("repeated-no-fill", ".nf"),
    ] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n{request}\nB\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-no-fill-{label}-boundary.1")),
            manual.as_bytes(),
        )
        .expect("parse no-fill boundary fixture");
        let text = document.sections[0]
            .blocks
            .iter()
            .map(|block| match block {
                Block::Preformatted { children, .. } | Block::Paragraph { children, .. } => {
                    inline_text(children)
                }
                Block::VerticalSpace { .. } => "\n".to_owned(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("AX"), "{label}: {:#?}", document.sections);
        assert!(text.contains('B'), "{label}: {:#?}", document.sections);
        assert!(!text.contains("AB"), "{label}: {:#?}", document.sections);
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-no-fill-margin-character.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n.mc |\nB\n.fi\n",
    )
    .expect("parse no-break margin-character fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "AX B", "{children:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-no-fill-margin-pending-cell.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\n\\zX\\c\n.mc |\nB\n.fi\n",
    )
    .expect("parse no-break margin pending-cell fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "X B", "{children:?}");

    for (label, first, expected) in [
        ("plain", "A", "A B"),
        ("continued-zero-advance", r"A\zX\c", "AX B"),
    ] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n{first}\n.mc |\nB\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-filled-margin-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse filled no-break margin-character fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn no_fill_exit_and_document_end_settle_continued_zero_advance_state() {
    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-zero-advance-before-fi.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n.fi\nB\n",
    )
    .expect("parse no-fill exit fixture");
    let [
        Block::Preformatted {
            children: literal, ..
        },
        Block::Paragraph {
            children: filled, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!(
            "expected literal and filled blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(literal), "AX", "{literal:?}");
    assert_eq!(inline_text(filled), "B", "{filled:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-zero-advance-at-eof.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c",
    )
    .expect("parse no-fill EOF fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "AX", "{children:?}");
}

#[test]
fn semantic_link_identity_executes_zero_advance_controls_without_guessing_display_text() {
    for (label, macro_name, source, expected) in [
        (
            "mail-no-space",
            "Mt",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt user@\\z\\cexample.org\n".as_slice(),
            "user@example.org",
        ),
        (
            "mail-zero-advance-glyph",
            "Mt",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt user@\\zXexample.org\n".as_slice(),
            "user@example.org",
        ),
        (
            "mail-zero-advance-font-control",
            "Mt",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt user@\\z\\fBexample.org\n".as_slice(),
            "user@xample.org",
        ),
        (
            "link-zero-width",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\z\\&suffix label\n".as_slice(),
            "https://example.org/suffix",
        ),
        (
            "link-word-break",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\z\\psuffix label\n".as_slice(),
            "https://example.org/suffix",
        ),
        (
            "link-no-space",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\z\\csuffix label\n".as_slice(),
            "https://example.org/suffix",
        ),
        (
            "link-unknown-glyph",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\[nosuch]suffix label\n".as_slice(),
            "https://example.org/suffix",
        ),
        (
            "mail-out-of-range-numbered-glyph",
            "Mt",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Mt user@\\N'999'example.org\n".as_slice(),
            "user@example.org",
        ),
        (
            "link-html-device-name",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\*[.T] label\n".as_slice(),
            "https://example.org/html",
        ),
        (
            "link-overstrike-final-glyph",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/\\o'ab' label\n".as_slice(),
            "https://example.org/b",
        ),
        (
            "link-overstrike-standard-nested-argument",
            "Lk",
            b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.org/A\\o'1\\f\\N'39'2'B label\n".as_slice(),
            "https://example.org/A2B",
        ),
    ] {
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("link-identity-{label}.1")),
            source,
        )
        .expect("parse link identity fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: expected one paragraph: {:#?}", document.sections);
        };
        let target = children.iter().find_map(|inline| match inline {
            Inline::Link { target, .. } => Some(target),
            _ => None,
        });
        match macro_name {
            "Mt" => assert!(
                matches!(target, Some(mant_ir::LinkTarget::Email { address }) if address == expected),
                "{label}: wrong typed email target: {target:?}"
            ),
            "Lk" => assert!(
                matches!(target, Some(mant_ir::LinkTarget::External { uri }) if uri == expected),
                "{label}: wrong typed external target: {target:?}"
            ),
            _ => unreachable!(),
        }
    }
}

#[test]
fn zero_advance_treats_every_empty_enclosure_delimiter_as_a_formatter_word() {
    for (macro_name, delimiters) in [
        ("Dq", "“”"),
        ("Op", "[]"),
        ("Pq", "()"),
        ("Brq", "{}"),
        ("Sq", "‘’"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt ZERO-ADVANCE 1\n.Os\n.Sh DESCRIPTION\n.No A\\zX\n.{macro_name}\n.No B\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-empty-{macro_name}.1")),
            source.as_bytes(),
        )
        .expect("parse empty mdoc enclosure");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{macro_name}: expected one paragraph");
        };
        assert_eq!(
            inline_text(children),
            format!("AX{delimiters} B"),
            "{macro_name}: {children:?}"
        );
    }
}

#[test]
fn visible_glyphs_before_a_definition_break_do_not_detach_the_head() {
    let document = parse_manual_bytes(
        std::path::Path::new("definition-glyph-before-break.1"),
        b".TH DEFINITION 1\n.SH DESCRIPTION\n.TP\n.B x\n\\[u03B1]\n.br\nBODY\n",
    )
    .expect("parse visible glyph before a definition body break");
    let text = visible_document_text(&document);

    // A glyph decoded from a named escape is visible content, not a formatter
    // transition. It therefore remains with x before `.br` starts BODY.
    assert!(text.contains("x α \nBODY"), "{text:?}");
}
