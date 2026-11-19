use super::{markers, word_hits};
use mant_ir::{Inline, LinkTarget, inline_plain_text, visit::Visit};

const MDOC: &str =
    ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";

mod operand_edits;
mod unproved_prefixes;
mod word_projection;

#[path = "field_replay/related_slots.rs"]
mod related_slots;

#[derive(serde::Deserialize)]
struct Case {
    family: String,
    source: String,
    want: Vec<Vec<String>>,
    #[serde(default)]
    link: bool,
}

const SEAM_WORDS: &[&str] = &["PASSYTAIL", "PASSXYTAIL", "PASSX", "YTAIL", "PASS"];

#[test]
fn native_slot_edits_cross_source_words_and_keep_link_ownership() {
    // Fixed CVS term.c::term_word writes both the entry blank and payload
    // before term_fill accepts a range. All exact sources ran lint, ASCII
    // and UTF-8 first; the two terminal profiles gave the same marker rows.
    let cases: Vec<Case> = serde_json::from_str(include_str!("field_replay_oracle.json")).unwrap();
    for case in cases {
        let source = &case.source;
        let raw = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = crate::json_roundtrip::json_string_roundtrip(&raw);
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for body in [&content, &readback] {
            let text = mant_render::render_query_text(body);
            let actual: Vec<_> = text
                .lines()
                .map(|line| marker_row(line, &case.family))
                .filter(|line| !line.is_empty())
                .collect();
            assert_eq!(actual, case.want, "{source}\n{text}");
        }
        let queries = if case.family == "continuation" {
            SEAM_WORDS
        } else {
            &["NEEDLE"]
        };
        for query in queries {
            let want = case
                .want
                .iter()
                .flatten()
                .filter(|word| *word == query)
                .count();
            assert_eq!(word_hits(&content, query), want, "{source}");
        }
        if case.link {
            let doc = content.document.as_ref().unwrap();
            let mut labels = LinkedNeedle::default();
            for block in &doc.sections[1].blocks {
                labels.visit_block(block);
            }
            let want = case
                .want
                .iter()
                .flatten()
                .filter(|word| *word == "NEEDLE")
                .count();
            assert_eq!(labels.0, want, "{source}");
        }
    }
}

fn marker_row<'a>(line: &'a str, family: &str) -> Vec<&'a str> {
    if family == "continuation" {
        line.split(|character: char| !character.is_ascii_uppercase())
            .filter(|word| {
                word.starts_with("PASS") || word.ends_with("YTAIL") || *word == "ENDTEXT"
            })
            .collect()
    } else {
        markers(line)
    }
}

#[derive(Default)]
struct LinkedNeedle(usize);

impl<'ir> Visit<'ir> for LinkedNeedle {
    fn visit_inline(&mut self, node: &'ir Inline) {
        if let Inline::Link {
            children,
            target: LinkTarget::External { uri },
            ..
        } = node
            && uri == "https://example.test/r"
        {
            self.0 += inline_plain_text(children).matches("NEEDLE").count();
        }
        mant_ir::visit::walk_inline(self, node);
    }
}

#[test]
fn nonadvancing_marks_survive_until_an_accepted_graph_covers_their_base() {
    #[derive(serde::Deserialize)]
    struct CombiningCase {
        source: String,
        want: String,
    }
    // The raw UTF-8 suffix, including backspaces, matches the source with
    // no opening device edits for every case. The semantic scalar sequence
    // follows the same contract as direct zero-advance words.
    let cases: Vec<CombiningCase> =
        serde_json::from_str(include_str!("combining_oracle.json")).unwrap();
    for case in cases {
        let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for projection in [&content, &readback] {
            let text = mant_render::render_query_text(projection);
            let after = text
                .split_once("ALPHA")
                .unwrap()
                .1
                .lines()
                .next()
                .unwrap_or_default();
            assert_eq!(after, case.want, "{}\n{text}", case.source);
        }
    }
}

#[test]
fn an_absolute_or_tight_relative_edit_retires_only_overwritten_link_members() {
    // The exact fixed ASCII and UTF-8 profiles agree. The X escapes write
    // NBRZW, and term.c::term_word() can rewrite those and the entry blank
    // before term_fill() chooses the accepted rows.
    for (boundary, motion, retained) in [(" Ns No ", "-2n", "PREFI"), ("\n.No ", "|5n", "PREF")] {
        let source = format!(
            "{MDOC}.No \"\\X'tty: link https://example.test/old'PREFIX\\X'tty: link'\"{boundary}\"\\h'{motion}'\\X'tty: link https://example.test/new'REPLACEMENT\\X'tty: link'\\p BREAKWORD\"\n.Pp\nOwner.\nENDMARK\n"
        );
        let raw = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = crate::json_roundtrip::json_string_roundtrip(&raw);
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for projection in [&content, &readback] {
            let rendered = mant_render::render_query_text(projection);
            assert!(
                rendered.contains(&format!("{retained}REPLACEMENT\nBREAKWORD")),
                "{rendered}"
            );
            let doc = projection.document.as_ref().unwrap();
            let mut labels = OwnedLabels::default();
            labels.visit_section(&doc.sections[1]);
            assert_eq!(
                labels.0,
                [
                    ("https://example.test/old".into(), retained.into()),
                    ("https://example.test/new".into(), "REPLACEMENT".into()),
                ],
                "{source}"
            );
        }
    }
}

#[derive(Default)]
struct OwnedLabels(Vec<(String, String)>);

impl<'ir> Visit<'ir> for OwnedLabels {
    fn visit_inline(&mut self, node: &'ir Inline) {
        if let Inline::Link {
            children,
            target: LinkTarget::External { uri },
            ..
        } = node
        {
            self.0.push((uri.clone(), inline_plain_text(children)));
        }
        mant_ir::visit::walk_inline(self, node);
    }
}

#[test]
fn a_relocated_glyph_keeps_its_link_through_json_and_markdown() {
    // Fixed CVS ASCII and UTF-8 agree on PT, PTM and TM. The overwrite
    // only retires the link's label when it covers the buffered M itself.
    let uri = "https://example.test/r";
    for (distance, visible, label) in [(2, "PT", ""), (3, "PTM", "M"), (4, "TM", "M")] {
        let source = format!(
            "{MDOC}.No \"P\\X'tty: link {uri}'\\zM\\X'tty: link'\" No \"\\h'-{distance}n'T\"\n.Pp\nDONE.\n"
        );
        let content = crate::json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
        );
        let markdown = mant_codec::encode::render_markdown(&content);
        let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for projection in [&content, &restored] {
            let text = mant_render::render_query_text(projection);
            assert!(
                text.lines().any(|row| row.trim() == visible),
                "{source}\n{text}"
            );
            assert_eq!(word_hits(projection, visible), 1, "{source}");
            let mut labels = OwnedLabels::default();
            labels.visit_section(&projection.document.as_ref().unwrap().sections[1]);
            assert_eq!(labels.0, [(uri.to_owned(), label.to_owned())], "{source}");
        }
    }
}

#[test]
fn earlier_input_registers_participate_in_definition_head_acceptance() {
    // The earlier .No's NODE_LINE produces ASCII_TABREF. term_word then
    // writes the automatic blank. HORIZ, overstrike trim and BACKBEFORE
    // act on the complete prefix before the private field is flushed.
    for style in ["tag -width 4n", "hang -width 4n", "inset"] {
        for (prefix, operation, admits) in [
            ("PRE", r"\l'1n.'\h'-1n'", false),
            (r"PRE\zX", r"\l'1n.'\h'-1n'", true),
            (r"PRE\zX", r"\l'2n '\h'|2n'", true),
            (r"PRE\zX", r"\l'2n '\h'|3n'", true),
            (r"PRE\z", r"\o' '", true),
            (r"PRE\z", r"\o' '\h'|3n'", true),
            (r"PRE\z", r"Q\p \o' '", true),
        ] {
            let source = format!(
                "{MDOC}.Bl -{style}\n.It Xo\n.No {prefix}\n.No \"{operation}\\p XYZ\"\n.Xc\nENDING\n.El\n"
            );
            let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let markdown = mant_codec::encode::render_markdown(&content);
            let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
            for projection in [&content, &readback] {
                let text = mant_render::render_query_text(projection);
                assert_eq!(text.contains("XYZ"), admits, "{source}\n{text}");
            }
        }
    }
}
