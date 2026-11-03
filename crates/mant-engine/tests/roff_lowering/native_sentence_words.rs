//! Native `NODE_EOS` travels through the shared source exit and word buffer.

use libmandoc_rs::{Node, Parser};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    rows: Vec<String>,
    sentence_sites: Vec<(u32, u32)>,
}

fn sentence_sites(node: &Node, sites: &mut Vec<(u32, u32)>) {
    if node.flags.sentence_end {
        sites.push((node.line, node.column));
    }
    for child in &node.children {
        sentence_sites(child, sites);
    }
}

#[test]
fn native_sentence_words_consume_real_separators_and_source_exit_flags() {
    // The complete 62-source mdoc cohort plus ten man and four tag/hang
    // sources, nine direct-operand contrasts and four ce/rj sources ran all five pristine
    // profiles before these expectations.
    // mdoc.c:264-277 and append_delims (mdoc_macro.c:532-551) supply EOS;
    // no rendered punctuation heuristic may replace those owned AST facts.
    // Both native node exits arm SENTENCE after font/post, and term_word()
    // writes two ordinary blanks, one KEEP blank, or none under NOSPACE,
    // then clears SENTENCE even for an empty/control-only word (573-589).
    let fixture: Fixture =
        serde_json::from_str(include_str!("native_sentence_words/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    // roff_term_pre_ce() also calls term_word directly, without child exit.
    // Three further exact sources pin BACKBEFORE's two-cell SENTENCE
    // separator: a cached dot consumes one, while bare BACKAFTER consumes
    // a graph and leaves the complete automatic word blank in place.
    assert_eq!(fixture.cases.len(), 92);
    for case in fixture.cases {
        let parsed = Parser::default()
            .parse_bytes(&case.id, case.source.as_bytes())
            .unwrap();
        let mut sites = Vec::new();
        sentence_sites(&parsed.document.root, &mut sites);
        assert_eq!(sites, case.sentence_sites, "{}: source EOS flags", case.id);
        let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let bundle = mant_protocol::QueryBundle::from(&content);
        let json = serde_json::to_string(&bundle).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(&bundle).unwrap(),
            "{}: actual JSON string roundtrip",
            case.id
        );
        let content: mant_ir::ResolvedContent = decoded.into();
        assert_eq!(
            mant_ir::validate_document(content.document.as_ref().unwrap()).len(),
            0,
            "{}",
            case.id
        );
        let rendered = mant_render::render_query_man(&content);
        let region = rendered
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("NEXT\n")
            .unwrap()
            .0;
        // Only the common native page origin and final section gap are
        // presentation. Interior blanks, all word boundaries and hard rows
        // stay exact, including 0/1/2/3 spaces and graphless word effects.
        let actual: Vec<_> = region
            .trim_end_matches('\n')
            .split('\n')
            .map(|row| row.trim_start_matches(' '))
            .collect();
        assert_eq!(
            actual, case.rows,
            "{}: native rows and exact word separators",
            case.id
        );
    }
}
