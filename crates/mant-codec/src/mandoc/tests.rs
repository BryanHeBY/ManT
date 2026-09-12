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
