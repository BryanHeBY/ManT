//! LK family (review §25.3): links, targets and positions (NF03/NF04).
//!
//! Oracle cases `lk02`–`lk16` pin the terminal row projection of the
//! upstream link display rules (`mdoc_term.c::termp_lk_pre`: description
//! operands → generated `:` → address → trailing punctuation; `%U/%R`
//! reference fields; `.Mt`; man `UR/UE`). `lk01` has no roff oracle —
//! it is a Markdown-input contract test (review §19.1: the generic link
//! projection must not append `: URI` to Markdown labels).
//!
//! The `TARGET_RED` layer pins the roff-identity decode of typed link
//! targets (review §20: `%U` operands reach `LinkTarget::External` with
//! the raw roff escapes still inside); it is dispatched with the LK
//! fixes to NF-LINK.

use super::{MatrixRun, actual_rows, case_names, load_case};
use mant_ir::{
    Inline, LinkTarget,
    visit::{self, Visit},
};

#[path = "link_contracts.rs"]
mod contracts;

#[path = "portable_display_contracts.rs"]
mod portable;

/// Cases whose layer-1 projection is still red against the oracle pin.
/// Dispatched to NF-LINK (review §26.1 step 3); entries are removed as
/// the fixes land.
const KNOWN_RED: &[&str] = &[];

// These two filled paragraphs have no authored row requests. Their native
// width=78 breaks are device wrapping, covered separately below rather than
// being mistaken for source hard breaks. All other fixtures keep exact rows.
const RESPONSIVE_CASES: &[&str] = &["lk16_long", "lk16_many"];

/// Cases whose typed link targets still carry raw roff escapes (NF04).
const TARGET_RED: &[&str] = &[];

#[derive(Default)]
struct LinkTargets(Vec<String>);

impl<'ir> Visit<'ir> for LinkTargets {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Link { target, .. } = inline {
            let uri = match target {
                LinkTarget::External { uri } => Some(uri.clone()),
                LinkTarget::Email { address } => Some(address.clone()),
                // Graph-edge targets are not roff-decoded URIs, but they
                // still must never leak a raw escape.
                LinkTarget::Document { name, fragment } => Some(
                    fragment
                        .as_ref()
                        .map_or_else(|| name.clone(), |fragment| format!("{name}#{fragment}")),
                ),
                LinkTarget::Manual {
                    name,
                    manual_section,
                } => Some(
                    manual_section
                        .as_ref()
                        .map_or_else(|| name.clone(), |section| format!("{name}({section})")),
                ),
                LinkTarget::Section { .. } => None,
            };
            if let Some(uri) = uri {
                self.0.push(uri);
            }
        }
        visit::walk_inline(self, inline);
    }
}

fn link_targets(query: &mant_ir::ResolvedContent) -> Vec<String> {
    let mut collector = LinkTargets::default();
    if let Some(document) = &query.document {
        collector.visit_document(document);
    }
    collector.0
}

#[test]
fn lk_matrix_matches_the_pinned_reference() {
    let mut matrix = MatrixRun::new();
    for name in case_names("lk", 28) {
        if RESPONSIVE_CASES.contains(&name.as_str()) {
            continue;
        }
        matrix.evaluate(&name, KNOWN_RED, None);
    }
    matrix.finish(KNOWN_RED, "LK");
}

fn unwrap_filled_description(mut rows: Vec<String>) -> Vec<String> {
    let start = rows.iter().position(|row| row == "DESCRIPTION").unwrap() + 1;
    let end = start
        + rows[start..]
            .iter()
            .position(String::is_empty)
            .expect("fixture's filled paragraph has a following blank row");
    // Only the proven filled paragraph is rejoined. Every paragraph boundary
    // and every surrounding row remains exact; whitespace is not collapsed.
    let paragraph = rows[start..end].join(" ");
    rows.splice(start..end, [paragraph]);
    rows
}

#[test]
fn long_lk_paragraphs_keep_words_without_fixed_device_wrapping() {
    // Exact fixtures were rerun with the pristine CVS -Tutf8/-Thtml before
    // these assertions. term.c::term_fill() wraps these filled words at the
    // device margin; no source break exists. ManT retains responsive rows.
    for name in RESPONSIVE_CASES {
        let (query, expected) = load_case(name);
        let expected = unwrap_filled_description(expected);
        let actual = unwrap_filled_description(actual_rows(&mant_render::render_query_man(&query)));
        assert_eq!(
            actual, expected,
            "{name}: word or paragraph boundary changed"
        );
        let targets = link_targets(&query);
        let expected_targets = if *name == "lk16_long" {
            vec![format!("https://e.example/{}", "x".repeat(120))]
        } else {
            (1..=3)
                .map(|index| format!("https://e.example/{index}"))
                .collect()
        };
        assert_eq!(
            targets, expected_targets,
            "{name}: target identities changed"
        );
        let label = if *name == "lk16_long" {
            "L".repeat(120)
        } else {
            "three".to_owned()
        };
        let search = mant_query::search_query(
            &query,
            &mant_protocol::SearchQuery {
                pattern: label.clone(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope: mant_protocol::SearchScope::default(),
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        let [hit] = search.matches.as_slice() else {
            panic!("{name}: label search changed");
        };
        let [occurrence] = hit.occurrences.as_slice() else {
            panic!("{name}: duplicated label");
        };
        assert_eq!(occurrence.matched_text, label);
        assert_eq!(
            occurrence.markdown.end_byte - occurrence.markdown.start_byte,
            u64::try_from(label.len()).unwrap()
        );
        let json = mant_render::render_query_json(&query, false).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(
            unwrap_filled_description(actual_rows(&mant_render::render_query_man(&decoded.into()))),
            expected,
            "{name}: JSON changed the responsive text"
        );
    }
}

/// LK01 (review §19.1, §25.3): a Markdown `[label](uri)` link keeps the
/// compact label display. The IR visible label is `label`; the address
/// appendix invented by the generic terminal link visitor
/// (`link_target_display_gap`) was the regression. Promoted green by the
/// NF-LINK visitor fix; the flag stays so drift is still detected.
const LK01_RED: bool = false;

#[test]
fn lk01_markdown_external_links_keep_the_compact_label() {
    let query = mant_loader::load_markdown_text(
        "# Title\n\n[label](https://example.com/x)\n",
        Some("lk01.md".to_owned()),
    )
    .expect("parse markdown link fixture");
    let rendered = mant_render::render_query_text(&query);
    let rows = actual_rows(&rendered);
    let failed = rows != ["label".to_owned()] || rendered.contains("https://example.com/x");
    assert_eq!(
        failed, LK01_RED,
        "LK01 red tracking drifted\nrows: {rows:?}\n{rendered}"
    );
}

/// NF04 layer (review §20): typed link targets are roff-identity decoded
/// — no `\&`, font or zero-width escape survives into the URI/mailbox
/// strings. Red cases must be exactly `TARGET_RED`; green cases are
/// asserted directly.
#[test]
fn lk_targets_are_roff_identity_decoded() {
    let mut red = Vec::new();
    let mut red_names: Vec<String> = Vec::new();
    for name in case_names("lk", 28) {
        let (query, _) = load_case(&name);
        let targets = link_targets(&query);
        let dirty: Vec<&str> = targets
            .iter()
            .filter(|uri| uri.contains('\\'))
            .map(String::as_str)
            .collect();
        if !dirty.is_empty() {
            red.push(format!("{name}: {dirty:?}"));
            red_names.push(name);
        }
    }
    let unexpected = red_names
        .iter()
        .filter(|name| !TARGET_RED.contains(&name.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let still_listed = TARGET_RED
        .iter()
        .copied()
        .filter(|listed| !red_names.iter().any(|name| name == listed))
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty() && still_listed.is_empty(),
        "TARGET_RED tracking drifted; new reds {unexpected:?}, stale {still_listed:?}\n{}",
        red.join("\n")
    );
}
