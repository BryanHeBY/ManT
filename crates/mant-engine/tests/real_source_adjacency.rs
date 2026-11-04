//! Exact complete real sources selected from the November 2026 roff audit.
//! Provenance and licenses: tests/fixtures/roff/real/source-adjacency/README.md.

#[path = "support/fixtures.rs"]
#[allow(dead_code)]
mod common;

use std::path::PathBuf;

use common::{block_slice_text, inline_text, section};
use mant_ir::{Block, Document, Inline, LinkTarget, ResolvedContent};
use mant_loader::{load_markdown_text, parse_manual_source};
use mant_protocol::{ContentSelector, EvidenceBasis, ExplanationOptions, ExplanationQuery};

fn fixture(file: &str) -> ResolvedContent {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/source-adjacency")
        .join(file);
    let document = parse_manual_source(&path).expect("complete real source parses");
    common::query_for_document(file, &document)
}

fn transported(content: &ResolvedContent) -> ResolvedContent {
    let wire = serde_json::to_vec(&mant_protocol::QueryBundle::from(content))
        .expect("serialize complete real source");
    let decoded: mant_protocol::QueryBundle =
        serde_json::from_slice(&wire).expect("deserialize complete real source");
    let decoded: ResolvedContent = decoded.into();
    assert_eq!(content.document, decoded.document);
    decoded
}

fn paragraph_texts(document: &Document, title: &str) -> Vec<String> {
    section(document, title)
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect()
}

fn assert_two_markdown_readbacks(content: &ResolvedContent, title: &str, expected_paragraph: &str) {
    let mut content = content.clone();
    for round in 1..=2 {
        let markdown = mant_codec::encode::render_markdown(&content);
        content = load_markdown_text(&markdown, None).expect("actual Markdown readback");
        let paragraphs = paragraph_texts(content.document.as_ref().unwrap(), title);
        assert!(
            paragraphs.iter().any(|text| text == expected_paragraph),
            "round {round}, {title}: {paragraphs:?}"
        );
    }
}

#[test]
fn tmux_xo_keeps_the_complete_option_owner_link_and_query_binding() {
    // Each exact complete fixture was first checked with pristine CVS ASCII,
    // UTF-8, HTML, tree and lint. mdoc_term.c::print_mdoc_nodelist visits the
    // literal '(' before Fl inside Xo; Sx stays within this same It BODY.
    let original = fixture("arch-tmux.1.gz");
    for content in [&original, &transported(&original)] {
        let document = content.document.as_ref().unwrap();
        let item = common::nested_definition_items(section(document, "DESCRIPTION"))
            .into_iter()
            .find(|item| {
                item.entry
                    .as_ref()
                    .is_some_and(|entry| entry.names == ["-C"])
            })
            .expect("the authored -C option owner");
        assert_eq!(item.terms.len(), 1);
        assert_eq!(inline_text(&item.terms[0]), "-C");
        assert_eq!(item.description.len(), 1, "Xo must not split the body");
        assert_eq!(
            block_slice_text(&item.description),
            "Start in control mode (see the CONTROL MODE section).  Given twice (-CC) disables echo."
        );
        let mut links = Vec::new();
        for block in &item.description {
            common::visit_block_inlines(block, &mut |inline| {
                if let Inline::Link {
                    target, children, ..
                } = inline
                {
                    links.push((target.clone(), inline_text(children)));
                }
            });
        }
        assert_eq!(
            links,
            [(
                LinkTarget::Section {
                    id: section(document, "CONTROL MODE").id.clone()
                },
                "CONTROL MODE".into()
            )]
        );

        let explained = mant_query::explain_query(
            content,
            &ExplanationQuery {
                entry: "-C".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        let named = explained
            .evidence
            .iter()
            .find(|evidence| {
                evidence.outline.node.id() == item.entry.as_ref().unwrap().id.as_str()
                    && evidence
                        .entry
                        .as_ref()
                        .is_some_and(|entry| entry.names == ["-C"])
                    && evidence
                        .bases
                        .iter()
                        .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
            })
            .expect("explain binds to the same -C owner");
        let path = named.outline.path();
        let outline = mant_query::build_outline_projection(
            content,
            mant_protocol::EntryProjection::All,
            None,
        )
        .unwrap();
        let node = common::find_outline_entry(&outline.nodes, "-C").expect("outline -C");
        assert_eq!(node.id(), named.outline.node.id());
        let excerpt = mant_query::select_excerpt(content, &[ContentSelector::path(path)]).unwrap();
        assert!(
            mant_render::render_excerpt_text(&excerpt).contains("Given twice (-CC) disables echo.")
        );
        let found = mant_query::search_query(
            content,
            &serde_json::from_value(
                serde_json::json!({"pattern":"Given twice (-CC) disables echo."}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].outline.node.id(), named.outline.node.id());
    }
}

#[test]
fn openbsd_date_keeps_sm_controlled_nested_synopsis_seams() {
    // mdoc_term.c::termp_sm_pre and termp_ns_pre govern joining across this
    // complete real synopsis; print_mdoc_node preserves each enclosure phase.
    let content = transported(&fixture("openbsd-date.1"));
    let expected = "date [-aju] [-f pformat] [-r seconds] [-z output_zone] [+format] [[[[[[cc]yy]mm]dd]HH]MM[.SS]]";
    assert_eq!(
        paragraph_texts(content.document.as_ref().unwrap(), "SYNOPSIS"),
        [expected]
    );
    assert_two_markdown_readbacks(&content, "SYNOPSIS", expected);
}

#[test]
fn dragonfly_regex_keeps_complete_fo_fa_declarations_and_enclosure_prefix() {
    // mdoc_term.c::synopsis_pre, termp_fo_pre/post and termp_fa_pre govern
    // return-type hard rows and generated punctuation between complete Fa
    // siblings. The exact source's four first native declarations were read
    // before these expectations were written; responsive soft wraps are omitted.
    let content = transported(&fixture("dragonfly-regex.3.gz"));
    let declarations = paragraph_texts(content.document.as_ref().unwrap(), "SYNOPSIS");
    for expected in [
        "int\nregcomp(regex_t *restrict preg, const char *restrict pattern, int cflags);",
        "size_t\nregerror(int errcode, const regex_t *restrict preg, char *restrict errbuf, size_t errbuf_size);",
        "int\nregexec(const regex_t *restrict preg, const char *restrict string, size_t nmatch, regmatch_t pmatch[restrict], int eflags);",
        "void\nregfree(regex_t *preg);",
    ] {
        assert_eq!(
            declarations
                .iter()
                .filter(|text| text.as_str() == expected)
                .count(),
            1,
            "{declarations:?}"
        );
    }
    let text = block_slice_text(
        &section(content.document.as_ref().unwrap(), "IMPLEMENTATION CHOICES").blocks,
    );
    assert!(text.contains("A repetition operator (‘?’,"), "{text}");
}

#[test]
fn illumos_ksh_arithmetic_styles_preserve_exact_text_through_two_readbacks() {
    // mdoc_term.c::termp_bold_pre/termp_under_pre retain the adjacent Sy/Ar
    // scopes while termp_ns_pre joins their text. The old real-source failure
    // added literal '**' before arithmetic_expression on Markdown readback.
    let content = transported(&fixture("illumos-ksh93.1"));
    let expected = "An arithmetic expression enclosed in double parentheses preceded by a dollar sign - $((arithmetic_expression)) - is replaced by the value of the arithmetic expression within the double parentheses.";
    assert_eq!(
        paragraph_texts(
            content.document.as_ref().unwrap(),
            "Arithmetic Substitution"
        ),
        [expected]
    );
    assert_two_markdown_readbacks(&content, "Arithmetic Substitution", expected);
}
