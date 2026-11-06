//! MC consumes native input, not a synthetic next-word separator.

use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    case_count: usize,
    profile_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    source_sha256: String,
    profiles: BTreeMap<String, Profile>,
    utf8_rows: Vec<String>,
}

#[derive(Deserialize)]
struct Profile {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!("margin_flush/cases.json")).unwrap();
    assert_eq!(fixture.case_count, 19);
    assert_eq!(fixture.profile_count, 95);
    assert_eq!(fixture.cases.len(), fixture.case_count);
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    for case in &fixture.cases {
        let mut source_hash = String::with_capacity(64);
        for byte in &Sha256::digest(case.source.as_bytes()) {
            write!(source_hash, "{byte:02x}").unwrap();
        }
        assert_eq!(
            source_hash, case.source_sha256,
            "{}: exact source receipt",
            case.id
        );
        assert_eq!(
            case.profiles.keys().map(String::as_str).collect::<Vec<_>>(),
            ["ascii", "html", "lint", "tree", "utf8"]
        );
        for profile in case.profiles.values() {
            assert_eq!(profile.status, 0, "{}: pristine status", case.id);
            assert_eq!(profile.stdout_sha256.len(), 64);
            assert_eq!(
                profile.stderr_sha256,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            );
        }
    }
    fixture.cases
}

fn body_rows(document: &mant_ir::Document) -> Vec<String> {
    let section = document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    let mut rows = Vec::new();
    for block in &section.blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                rows.extend(inline_text(children).split('\n').map(str::to_owned));
            }
            Block::VerticalSpace { lines, .. } => {
                rows.extend(std::iter::repeat_n(String::new(), usize::from(*lines)));
            }
            other => panic!("plain source acquired another semantic owner: {other:#?}"),
        }
    }
    rows
}

#[test]
fn margin_flush_uses_actual_cells_and_preserves_exact_word_and_row_boundaries() {
    // All 19 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint before
    // these assertions. roff_term_pre_mc tests p->col, then temporarily
    // applies NOBREAK and clears NOSPACE only after a real flush. Native
    // term_flushln assigns minbl=trailspace, not an extra plain-field SP.
    for case in cases() {
        let document = parse_manual_bytes(
            std::path::Path::new("margin-flush-word-boundary.1"),
            case.source.as_bytes(),
        )
        .unwrap();
        assert_eq!(body_rows(&document), case.utf8_rows, "{}: IR", case.id);
        let json = serde_json::to_string(&document).unwrap();
        let mut restored: mant_ir::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(body_rows(&restored), case.utf8_rows, "{}: JSON", case.id);
        for round in 1..=2 {
            let content = mant_ir::ResolvedContent {
                address: None,
                label: "margin-flush".into(),
                document: Some(restored),
                tldr: None,
            };
            let markdown = crate::encode::render_markdown(&content);
            restored = crate::parse_markdown(&markdown, None).unwrap().document;
            assert_eq!(
                body_rows(&restored),
                case.utf8_rows,
                "{}: actual Markdown round{round}\n{markdown}",
                case.id
            );
        }
        if case.id == "styled-word-owners" {
            assert!(strong_document_text(&restored).contains("FIRST"));
            assert_eq!(emphasized_document_text(&restored), "SECOND");
        }
    }
}
