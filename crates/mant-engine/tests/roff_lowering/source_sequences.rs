//! Complete source parents are independent of recovered-cell/output owners.

use std::collections::BTreeMap;

use mant_ir::{Block, Inline, LinkTarget, ResolvedContent, inline_plain_text};
use serde::Deserialize;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    core_count: usize,
    risk_count: usize,
    bibliography_clean_count: usize,
    bibliography_warning_count: usize,
    bibliography_root_count: usize,
    cases: Vec<Case>,
    source_entries: Vec<SourceEntry>,
}

#[derive(Deserialize)]
struct SourceEntry {
    id: String,
    section: String,
    source: String,
    native: Oracle,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    cohort: String,
    section: String,
    source: String,
    independent_source: String,
    contract: String,
    code_text: Option<String>,
    whole: Oracle,
    independent: Oracle,
}

#[derive(Deserialize)]
struct Oracle {
    source_sha256: String,
    rows: Vec<String>,
    profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
struct Profile {
    status: i32,
    stdout_sha256: String,
    stderr_sha256: String,
    diagnostics: String,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("source_sequences/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(
        (
            fixture.core_count,
            fixture.risk_count,
            fixture.bibliography_clean_count,
            fixture.bibliography_warning_count
        ),
        (60, 16, 6, 8)
    );
    assert_eq!(fixture.bibliography_root_count, 6);
    assert_eq!(fixture.cases.len(), 96);
    for case in &fixture.cases {
        for (role, oracle) in [("whole", &case.whole), ("fragment", &case.independent)] {
            assert_eq!(oracle.source_sha256.len(), 64, "{}: {role}", case.id);
            assert_eq!(oracle.profiles.len(), 5, "{}: {role}", case.id);
            for (profile, receipt) in &oracle.profiles {
                assert_eq!(receipt.stdout_sha256.len(), 64);
                assert_eq!(receipt.stderr_sha256.len(), 64);
                if profile != "lint" {
                    assert_eq!(receipt.status, 0, "{}: {role}/{profile}", case.id);
                }
            }
        }
        let (whole_lint, fragment_lint) = (
            case.whole.profiles["lint"].status,
            case.independent.profiles["lint"].status,
        );
        let expected = match case.cohort.as_str() {
            "risk" if case.id == "cell-other-line-prefix" => (4, 2),
            "core" | "risk" => (4, 0),
            "bibliography-clean" | "bibliography-clean-root" => (0, 0),
            "bibliography-warning" => (2, 2),
            other => panic!("unknown recorded cohort {other}"),
        };
        assert_eq!(
            (whole_lint, fragment_lint),
            expected,
            "{}: dual lint classes",
            case.id
        );
        if case.cohort == "bibliography-warning" {
            assert!(
                case.whole.profiles["lint"]
                    .diagnostics
                    .contains("invalid content in Rs block: Ft")
            );
        }
    }
    fixture.cases
}

fn load(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&content);
    assert_eq!(
        mant_ir::validate_document(content.document.as_ref().unwrap()),
        []
    );
    content
}

fn blocks<'a>(content: &'a ResolvedContent, section: &str) -> &'a [Block] {
    &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|candidate| candidate.heading.plain_text() == section)
        .unwrap()
        .blocks
}

fn rendered_rows(content: &ResolvedContent, section: &str) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    let selected = text
        .split_once(&format!("{section}\n"))
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0;
    selected
        .strip_suffix("\n\n")
        .unwrap()
        .split('\n')
        .map(|row| row.trim_matches(' ').to_owned())
        .collect()
}

fn first_cell<'a>(content: &'a ResolvedContent, section: &str) -> &'a [Inline] {
    let [Block::Table { rows, .. }] = blocks(content, section) else {
        panic!("one recovered table owns the complete parent sequence")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 1);
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("one complete recovered inline owner")
    };
    children
}

fn collect_codes<'a>(nodes: &'a [Inline], output: &mut Vec<&'a str>) {
    for node in nodes {
        match node {
            Inline::Code { value } => output.push(value),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                collect_codes(children, output);
            }
            _ => {}
        }
    }
}

#[test]
fn recovered_cell_matrix_uses_the_independent_fragment_not_unsupported_table_geometry() {
    // Every source and its independent fragment ran pristine ASCII/UTF-8,
    // HTML/tree/lint before these assertions. Whole tbl lint=4 is UNSUPP;
    // clean independent fragments define the selected bounded enhancement.
    // mdoc_term.c::print_mdoc_nodelist retains original siblings;
    // synopsis_pre(Ft/Cd/In/Fn/Nm) consumes their logical predecessor, and
    // term_newln/term_flushln settle BACKBEFORE before the next operand.
    verify_cases(
        cases()
            .into_iter()
            .filter(|case| case.contract == "recovered"),
        |case| &case.id,
        |case| {
            let content = load(&case.source);
            let expected = case.independent.rows.join("\n");
            if let Some(expected_code) = &case.code_text {
                let mut codes = Vec::new();
                collect_codes(first_cell(&content, &case.section), &mut codes);
                assert_eq!(
                    codes,
                    [expected_code.as_str()],
                    "{}: semantic include Code",
                    case.id
                );
            }
            assert_eq!(
                inline_plain_text(first_cell(&content, &case.section)),
                expected,
                "{}: recovered cell",
                case.id
            );
            let independent = load(&case.independent_source);
            assert_eq!(
                rendered_rows(&independent, &case.section),
                case.independent
                    .rows
                    .iter()
                    .map(|row| row.trim_matches(' ').to_owned())
                    .collect::<Vec<_>>(),
                "{}: ordinary independent fragment",
                case.id
            );
            assert_eq!(
                rendered_rows(&content, &case.section),
                case.independent
                    .rows
                    .iter()
                    .map(|row| row.trim_matches(' ').to_owned())
                    .collect::<Vec<_>>(),
                "{}: CLI exact hard rows",
                case.id
            );
            let markdown = mant_codec::encode::render_markdown(&content);
            let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
            assert_eq!(
                rendered_rows(&read, &case.section),
                case.independent
                    .rows
                    .iter()
                    .map(|row| row.trim_matches(' ').to_owned())
                    .collect::<Vec<_>>(),
                "{}: actual Markdown readback\n{markdown}",
                case.id
            );
        },
    );
}

fn unwrap_exported_definition(blocks: &mut Vec<Block>) {
    if let [
        Block::List {
            kind: mant_ir::ListKind::Bullet,
            items,
            ..
        },
    ] = blocks.as_mut_slice()
    {
        assert_eq!(items.len(), 1);
        *blocks = std::mem::take(&mut items[0].blocks);
    }
}

fn portable_reference_rows(case: &Case) -> Vec<String> {
    let mut rows = case
        .whole
        .rows
        .iter()
        .map(|row| row.trim_matches(' ').to_owned())
        .collect::<Vec<_>>();
    // The fixed native row proves a shared prose HEAD/BODY separator. Only
    // that witnessed field gap becomes the published one-SP Markdown gap;
    // internal HEAD spaces and every independent hard/empty row remain exact.
    let last = rows.last_mut().unwrap();
    if let Some(before) = last.strip_suffix("BODY") {
        let head = before.trim_end_matches(' ');
        if !head.is_empty() && head.len() < before.len() {
            *last = format!("{head} BODY");
        }
    }
    rows
}

#[test]
fn bibliography_warning_recovery_and_clean_field_posts_keep_original_siblings() {
    // post_rs() retains invalid Ft with a warning. It does not license a
    // clean-source claim, nor erase that child's synopsis_pre. Normal %A/%T/
    // %U still own conjunctions, generated punctuation and final SENTENCE.
    verify_cases(
        cases()
            .into_iter()
            .filter(|case| case.contract == "reference-head"),
        |case| &case.id,
        |case| {
            let content = load(&case.source);
            let expected: Vec<_> = case
                .whole
                .rows
                .iter()
                .map(|row| row.trim_matches(' ').to_owned())
                .collect();
            assert_eq!(
                rendered_rows(&content, &case.section),
                expected,
                "{}: original bibliography owner",
                case.id
            );
            let [Block::DefinitionList { items, .. }] = blocks(&content, &case.section) else {
                panic!("Rs remains in its original definition HEAD")
            };
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].terms.len(), 1);
            assert!(inline_plain_text(&items[0].terms[0].content).contains(
                if case.id.ends_with("cached-glyph") {
                    "X"
                } else if case.cohort == "bibliography-clean" {
                    "Author0"
                } else {
                    "arg"
                }
            ));
            let [Block::Paragraph { children, .. }] = items[0].description.as_slice() else {
                panic!("one complete bibliography description")
            };
            assert_eq!(
                inline_plain_text(children).trim(),
                "BODY",
                "{}: no duplicated or missing BODY",
                case.id
            );
            let mut read = content.clone();
            for round in 1..=2 {
                let markdown = mant_codec::encode::render_markdown(&read);
                read = mant_loader::load_markdown_text(&markdown, None).unwrap();
                let section = read
                    .document
                    .as_mut()
                    .unwrap()
                    .sections
                    .iter_mut()
                    .find(|section| section.heading.plain_text() == case.section)
                    .unwrap();
                unwrap_exported_definition(&mut section.blocks);
                assert_eq!(
                    rendered_rows(&read, &case.section),
                    portable_reference_rows(case),
                    "{}: actual bibliography Markdown read {round}\n{markdown}",
                    case.id
                );
            }
        },
    );
}

fn collect(nodes: &[Inline], output: &mut Vec<(String, LinkTarget)>) {
    for node in nodes {
        match node {
            Inline::Link {
                target, children, ..
            } => {
                output.push((inline_plain_text(children), target.clone()));
                collect(children, output);
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                collect(children, output);
            }
            _ => {}
        }
    }
}
#[test]
fn recovered_links_and_font_scopes_keep_their_original_content_owner() {
    let case = cases()
        .into_iter()
        .find(|case| case.id == "cell-manual-identity")
        .unwrap();
    let content = load(&case.source);
    let mut links = Vec::new();
    collect(first_cell(&content, &case.section), &mut links);
    assert_eq!(
        links,
        [(
            "printf(3)".into(),
            LinkTarget::Manual {
                name: "printf".into(),
                manual_section: Some("3".into())
            }
        )]
    );
}

#[test]
fn native_table_requests_decline_optional_recovery_without_replaying_macros() {
    // Native ft requests execute in the original roff session, so optional
    // semantic replay is deliberately disabled. The complete whole-table
    // base cell is its own contract, distinct from independent fragment rows.
    verify_cases(
        cases()
            .into_iter()
            .filter(|case| case.contract == "native-fallback"),
        |case| &case.id,
        |case| {
            let content = load(&case.source);
            assert_eq!(
                inline_plain_text(first_cell(&content, &case.section)),
                case.whole.rows.join("\n"),
                "{}: original native cell",
                case.id
            );
        },
    );
}

#[test]
fn root_and_detached_bibliography_posts_share_spacing_and_sentence_word_execution() {
    // Exact root and HEAD sources each ran five pristine profiles first.
    // termp____post calls term_word after setting NOSPACE; Sm off determines
    // that word's outgoing flag. Only the later real Sm-on request clears it,
    // while SENTENCE still supplies two cells before the following word.
    verify_cases(
        cases()
            .into_iter()
            .filter(|case| case.contract == "reference-root"),
        |case| &case.id,
        |case| {
            let content = load(&case.source);
            assert_eq!(
                rendered_rows(&content, &case.section),
                case.whole.rows,
                "{}: root native post",
                case.id
            );
            let mut read = content;
            for round in 1..=2 {
                let markdown = mant_codec::encode::render_markdown(&read);
                read = mant_loader::load_markdown_text(&markdown, None).unwrap();
                assert_eq!(
                    rendered_rows(&read, &case.section),
                    case.whole.rows,
                    "{}: root bibliography Markdown read {round}\n{markdown}",
                    case.id
                );
            }
        },
    );
}

#[test]
fn empty_native_words_do_not_skip_the_next_source_line_or_break_continuations() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("source_sequences/cases.json")).unwrap();
    assert_eq!(fixture.source_entries.len(), 8);
    // Both native node walkers test NODE_NOFILL/NODE_LINE and NONEWLINE
    // before handlers (mdoc_term.c:314-317, man_term.c:922-925). term_newln
    // selects NOSPACE even with no cells (term.c:475-480); graphless words
    // still clear it, unlike IR output-owner drains (term.c:573-589).
    // tbl_term.c:175,608 additionally leaves NOSPACE selected and removes
    // NONOSPACE, so an outer Sm-off cannot glue later ordinary words.
    // Every exact source ran all five pristine profiles before these checks.
    verify_cases(
        fixture.source_entries,
        |case| &case.id,
        |case| {
            for profile in case.native.profiles.values() {
                assert_eq!(profile.status, 0, "{}: native profile", case.id);
            }
            let mut content = load(&case.source);
            for round in 0..=2 {
                let rendered = mant_render::render_query_man(&content);
                let actual = rendered
                    .split_once(&format!("{}\n", case.section))
                    .unwrap()
                    .1
                    .split_once("NEXT\n")
                    .unwrap()
                    .0
                    .strip_suffix("\n\n")
                    .unwrap()
                    .split('\n')
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual, case.native.rows,
                    "{}: exact source rows, round {round}",
                    case.id
                );
                if round < 2 {
                    let markdown = mant_codec::encode::render_markdown(&content);
                    content = mant_loader::load_markdown_text(&markdown, None).unwrap();
                }
            }
        },
    );
}
