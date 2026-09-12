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
