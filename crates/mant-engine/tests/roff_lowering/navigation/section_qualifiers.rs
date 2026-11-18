//! Authored section names, literal whitespace and uniquely qualified targets.

use mant_ir::{
    Block, Inline, LinkTarget, ResolvedContent, Section,
    visit::{self, Visit},
};
use mant_protocol::{ContentSelector, ExcerptSelection};
use std::fmt::Write as _;

const HEADER: &str = concat!(
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n",
    ".Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n",
);

type Expected = Option<(&'static str, &'static str)>;

#[test]
fn section_qualifiers_require_a_subject_and_count_all_spacing_variants() {
    // CVS mdoc_sx_pre derives its href from the authored phrase. For the
    // zero-width arguments the fixed tree/terminal retain the source label,
    // and the fixed HTML build has no usable destination (its empty-id
    // assertion can abort). Qualified lookup is ManT's own extension.
    let cases: &[(&str, &[&str], Expected)] = &[
        (r"\0", &["(valid)"], None),
        (r"\~", &["(valid)"], None),
        ("\"\"", &["(valid)"], None),
        ("Topic", &["Topic(valid)"], Some(("topic-valid", "BODY-A."))),
        (
            "Topic",
            &["Topic (valid)"],
            Some(("topic-valid", "BODY-A.")),
        ),
        (
            "Topic",
            &["Topic  (valid)"],
            Some(("topic-valid", "BODY-A.")),
        ),
        ("Topic", &["Topic  (valid)", "Topic (other)"], None),
        (
            "Topic",
            &["Topic  (valid)", "Topic(unfinished"],
            Some(("topic-valid", "BODY-A.")),
        ),
        (
            "Topic",
            &["Topic  (valid (nested))"],
            Some(("topic-valid-nested", "BODY-A.")),
        ),
        ("Topic", &["Topic  (valid) extra"], None),
        (
            "Topic",
            &["Topic  (valid)", "Topic (other)", "Topic"],
            Some(("topic", "BODY-C.")),
        ),
        (
            "\"Topic  (valid)\"",
            &["Topic  (valid)", "Topic (other)"],
            Some(("topic-valid", "BODY-A.")),
        ),
        (
            "\"Multi  Word\"",
            &["Multi  Word  (valid)", "Multi Word (other)"],
            Some(("multi-word-valid", "BODY-A.")),
        ),
    ];
    for &(reference, headings, expected) in cases {
        for reverse in [false, true] {
            let mut source = format!("{HEADER}.Sx {reference}\n");
            for slot in 0..headings.len() {
                let index = if reverse {
                    headings.len() - 1 - slot
                } else {
                    slot
                };
                let marker = ['A', 'B', 'C'][index];
                writeln!(source, ".Sh \"{}\"\nBODY-{marker}.", headings[index]).unwrap();
            }
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            assert_section_result(&query, reference, expected, &source);
        }
    }
}

fn assert_section_result(
    query: &ResolvedContent,
    reference: &str,
    expected: Expected,
    source: &str,
) {
    let document = query.document.as_ref().unwrap();
    let description = &document.sections[1];
    let unresolved = document
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("unresolved-section-reference"))
        .count();
    let markdown = mant_codec::encode::render_addressable_markdown(query);
    let ids = section_ids(description);
    if let Some((id, body)) = expected {
        assert_eq!(ids, [id], "{source}");
        assert_eq!(unresolved, 0, "{source}");
        assert!(markdown.text().contains(&format!("](#{id})")), "{source}");
        let readback = mant_loader::load_markdown_text(markdown.text(), None).unwrap();
        let readback_description = &readback.document.as_ref().unwrap().sections[1];
        assert_eq!(section_ids(readback_description), [id], "{source}");
        let excerpt = mant_query::select_excerpt(query, &[ContentSelector::id(id)]).unwrap();
        let [ExcerptSelection::DocumentSection { section, .. }] = excerpt.selections.as_slice()
        else {
            panic!("expected one authored target: {source}");
        };
        let [Block::Paragraph { children, .. }] = section.blocks.as_slice() else {
            panic!("expected the selected section to own its body: {source}");
        };
        assert_eq!(mant_ir::inline_plain_text(children), body, "{source}");
    } else {
        assert!(ids.is_empty(), "{source}");
        assert_eq!(unresolved, usize::from(reference != "\"\""), "{source}");
        assert!(!markdown.text().contains("](#"), "{source}");
        if matches!(reference, r"\0" | r"\~") {
            let [Block::Paragraph { children, .. }] = description.blocks.as_slice() else {
                panic!("the original spacing glyph remains visible: {source}");
            };
            assert_eq!(mant_ir::inline_plain_text(children), "\u{a0}", "{source}");
        }
    }
}

fn section_ids(section: &Section) -> Vec<&str> {
    #[derive(Default)]
    struct Links<'a>(Vec<&'a str>);

    impl<'a> Visit<'a> for Links<'a> {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Link {
                target: LinkTarget::Section { id },
                ..
            } = inline
            {
                self.0.push(id.as_str());
            }
            visit::walk_inline(self, inline);
        }
    }

    let mut links = Links::default();
    links.visit_section(section);
    links.0
}
