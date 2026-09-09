//! Executed no-fill words are not the same as arbitrary empty inline output.
//! The oracle is mandoc CVS HEAD `man_term`/`mdoc_term` and `term_word`. Where groff
//! preserves an empty macro parameter or a font-only row that mandoc does
//! not, these tests deliberately select mandoc rather than mix both policies.

use mant_ir::ResolvedContent;
use mant_ir::{
    Block, Inline,
    visit::{self, Visit},
};
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

#[test]
fn a_literal_display_switched_to_fill_keeps_source_word_and_indent_policy() {
    for (body, expected) in [
        ("ALPHA\n BETA", "ALPHA\n BETA"),
        ("ALPHA\\c\n BETA", "ALPHA BETA"),
        (".Sm off\nALPHA\nBETA", "ALPHA BETA"),
    ] {
        let text = render_query_text(&query("mdoc", &format!(".fi\n{body}\n.nf\nGAMMA")));
        assert!(text.contains(expected), "{body}: {text:?}");
    }
}

fn query(mode: &str, body: &str) -> ResolvedContent {
    let source = match mode {
        "mdoc" => format!(
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bd -literal\n{body}\n.Ed\nAFTER\n"
        ),
        "SY" => format!(".TH PROBE 1\n.SH TEST\n.SY probe\n.nf\n{body}\n.fi\n.YS\nAFTER\n"),
        _ => format!(".TH PROBE 1\n.SH TEST\n.nf\n{body}\n.fi\nAFTER\n"),
    };
    load_roff_bytes(source.as_bytes()).unwrap()
}

#[test]
fn man_empty_font_macro_operands_and_invisible_glyphs_occupy_a_row() {
    for mode in ["man", "SY"] {
        for request in [
            ".B \"\"", ".I \"\"", ".B \\&", ".I \\&", ".BR \\&", ".BI \\&",
        ] {
            let content = query(mode, &format!("ALPHA\n{request}\nBETA"));
            let text = render_query_text(&content);
            assert!(text.contains("ALPHA\n\nBETA"), "{mode}: {request}: {text}");
        }
    }
}

#[test]
fn empty_mdoc_or_alternating_font_parameters_are_not_literal_blank_rows() {
    for (mode, requests) in [
        ("mdoc", [".No \"\"", ".Em \"\"", ".Sy \"\""]),
        ("man", [".BI \"\"", ".BR \"\"", ".IR \"\""]),
    ] {
        for request in requests {
            let text = render_query_text(&query(mode, &format!("ALPHA\n{request}\nBETA")));
            assert!(text.contains("ALPHA\nBETA"), "{mode}: {request}: {text}");
        }
    }
    for request in [".No \\&", ".Em \\&", ".Sy \\&"] {
        let text = render_query_text(&query("mdoc", &format!("ALPHA\n{request}\nBETA")));
        assert!(text.contains("ALPHA\n\nBETA"), "{request}: {text}");
    }
}

#[test]
fn font_only_words_end_the_old_row_without_occupying_the_next_one() {
    for (mode, request) in [
        ("man", ".B \\fB"),
        ("man", "\\fB"),
        ("SY", ".B \\fB"),
        ("mdoc", ".No \\fB"),
        ("mdoc", "\\fB"),
    ] {
        for continued in [false, true] {
            let join = if continued { "\\c" } else { "" };
            let text = render_query_text(&query(mode, &format!("ALPHA{join}\n{request}\nBETA")));
            assert!(text.contains("ALPHA\nBETA"), "{mode}: {request}: {text}");
        }
        let text = render_query_text(&query(mode, &format!("ALPHA\n{request}")));
        assert!(text.contains("ALPHA\nAFTER"), "{mode}: {request}: {text}");
    }
}

#[test]
fn state_only_requests_and_continuation_words_do_not_invent_a_row() {
    for mode in ["man", "SY", "mdoc"] {
        for request in [".ft B", "\\c"] {
            let text = render_query_text(&query(mode, &format!("ALPHA\\c\n{request}\nBETA")));
            assert!(text.contains("ALPHABETA"), "{mode}: {request}: {text}");
        }
    }
}

#[test]
fn synopsis_body_executes_fill_mode_changes_instead_of_flattening_every_child() {
    let content = query("SY", "ALPHA\n.fi\nBETA\nSECOND\n.nf\nGAMMA\n.sp 2\nDELTA");
    let text = render_query_text(&content);
    assert!(
        text.contains("ALPHA\nBETA SECOND\nGAMMA\n\n\nDELTA"),
        "{text}"
    );
    let blocks = &content.document.as_ref().unwrap().sections[0].blocks;
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, Block::Paragraph { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, Block::VerticalSpace { lines: 2, .. }))
    );
    assert!(mant_ir::validate_document(content.document.as_ref().unwrap()).is_empty());
}

#[test]
fn literal_display_executes_explicit_fill_switches_and_restores_literal_rows() {
    for wrapped in [false, true] {
        let body = "ALPHA\n.fi\nBETA\nSECOND\n.br\nTHIRD\n.nf\nGAMMA\n.sp 2\nDELTA";
        let body = if wrapped {
            format!(".Bf -emphasis\n{body}\n.Ef")
        } else {
            body.into()
        };
        let content = query("mdoc", &body);
        let text = render_query_text(&content);
        assert!(
            text.contains("ALPHA\nBETA SECOND\nTHIRD\nGAMMA\n\n\nDELTA\nAFTER"),
            "{text}"
        );
        let blocks = &content.document.as_ref().unwrap().sections[0].blocks;
        assert!(matches!(
            blocks.as_slice(),
            [
                Block::Preformatted { .. },
                Block::Paragraph { .. },
                Block::Preformatted { .. },
                Block::VerticalSpace { lines: 2, .. },
                Block::Preformatted { .. },
                Block::Paragraph { .. }
            ]
        ));
        assert!(mant_ir::validate_document(content.document.as_ref().unwrap()).is_empty());
    }
}

#[test]
fn synopsis_font_changes_survive_spacing_until_the_synopsis_scope_ends() {
    struct Styles {
        strong: bool,
        words: Vec<(String, bool)>,
    }
    impl<'ir> Visit<'ir> for Styles {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            let saved = self.strong;
            match inline {
                Inline::Strong { .. } => self.strong = true,
                Inline::Text { value } => self.words.push((value.clone(), self.strong)),
                _ => {}
            }
            visit::walk_inline(self, inline);
            self.strong = saved;
        }
    }
    let content = query("SY", "ALPHA\n.ft B\n.sp 2\nBETA\nGAMMA");
    let mut styles = Styles {
        strong: false,
        words: Vec::new(),
    };
    styles.visit_document(content.document.as_ref().unwrap());
    for token in ["BETA", "GAMMA"] {
        assert!(
            styles
                .words
                .iter()
                .any(|(word, strong)| word == token && *strong),
            "{token}: {:?}",
            styles.words
        );
    }
    assert!(
        styles
            .words
            .iter()
            .any(|(word, strong)| word == "AFTER" && !strong),
        "{:?}",
        styles.words
    );
    let text = render_query_text(&content);
    assert!(text.contains("ALPHA\n\n\nBETA\nGAMMA\nAFTER"), "{text}");
}
