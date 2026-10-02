//! Controlled native omission remains readable and reaches every query summary.
//! Exact fixture sources were run through the registered pristine five profiles
//! before these assertions; `roff_escape.c::roff_escape_impl` keeps the native
//! 256-level guard. Its private omission receipt, not MANDOCERR text/severity,
//! supplies the document coverage reason.

use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};
use mant_ir::{DiagnosticImpact, ResolvedContent, content_complete, semantics_complete};
use mant_protocol::{
    ContentSelector, ExplanationOptions, ExplanationQuery, QueryBundle, SearchCase, SearchQuery,
    SearchScope, SearchSyntax,
};
use mant_query::{build_outline, explain_query, search_query, select_excerpt};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("escape_coverage/cases.json")).unwrap()
}

fn round_trip<T>(value: &T) -> T
where
    T: Serialize + DeserializeOwned,
{
    serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
}

fn content(case: &Value) -> ResolvedContent {
    mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap()
}

fn description_text(content: &ResolvedContent) -> String {
    let text = mant_render::render_query_man(content);
    text.split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("\nNEXT")
        .unwrap()
        .0
        .to_owned()
}

#[test]
fn native_escape_omission_is_structured_and_keeps_later_independent_text() {
    for case in cases() {
        let content = content(&case);
        let document = content.document.as_ref().unwrap();
        let limited = case["metadata"]["native_guard_expected"].as_bool().unwrap();
        let omission = document
            .diagnostics
            .iter()
            .filter(|finding| finding.code.as_deref() == Some("manual.escape-depth-truncated"))
            .collect::<Vec<_>>();
        assert_eq!(omission.len(), usize::from(limited), "{}", case["id"]);
        assert!(
            omission
                .iter()
                .all(|finding| finding.impact == DiagnosticImpact::ContentCoverage)
        );
        assert_eq!(
            content_complete(&document.diagnostics),
            !limited,
            "{}",
            case["id"]
        );
        assert_eq!(
            semantics_complete(&document.diagnostics),
            !limited,
            "{}",
            case["id"]
        );
        let encoded = serde_json::to_string(&QueryBundle::from(&content)).unwrap();
        let bundle: QueryBundle = serde_json::from_str(&encoded).unwrap();
        let restored = ResolvedContent::from(bundle);
        assert_eq!(restored.document, content.document, "{}", case["id"]);
        assert!(encoded.contains("AFTER"), "{}", case["id"]);
        let readable = description_text(&content);
        let after = readable.rfind("AFTER").unwrap();
        if let Some(prefix) = case["metadata"]["prefix"].as_str() {
            assert!(
                readable[..after].contains(prefix),
                "accepted body prefix: {}",
                case["id"]
            );
        }
    }
}

#[test]
fn real_outline_excerpt_explain_and_search_preserve_coverage_and_text_ranges() {
    for case in cases() {
        let content = content(&case);
        let document = content.document.as_ref().unwrap();
        let complete = !case["metadata"]["native_guard_expected"].as_bool().unwrap();
        let outline = round_trip(&build_outline(&content).unwrap());
        assert_eq!(outline.content_complete, complete, "{}", case["id"]);
        assert_eq!(outline.semantics_complete, complete, "{}", case["id"]);
        let description = document
            .sections
            .iter()
            .find(|s| mant_ir::inline_plain_text(&s.heading.content) == "DESCRIPTION")
            .unwrap();
        let selector = ContentSelector::id(description.id.clone());
        let excerpt = round_trip(&select_excerpt(&content, &[selector]).unwrap());
        assert_eq!(excerpt.content_complete, complete, "{}", case["id"]);
        assert_eq!(excerpt.semantics_complete, complete, "{}", case["id"]);
        let explanation = round_trip(
            &explain_query(
                &content,
                &ExplanationQuery {
                    entry: "AFTER".to_owned(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap(),
        );
        assert_eq!(explanation.content_complete, complete, "{}", case["id"]);
        assert_eq!(explanation.semantics_complete, complete, "{}", case["id"]);
        assert_search_ranges_and_pagination(&content, complete, &case);
    }
}

fn assert_search_ranges_and_pagination(content: &ResolvedContent, complete: bool, case: &Value) {
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let artifact = render_addressable_markdown_with_options(
            content,
            MarkdownOptions {
                native_text: scope == SearchScope::Visible,
                ..MarkdownOptions::ADDRESSABLE
            },
        );
        for offset in [0, 1] {
            let query = SearchQuery {
                pattern: "AFTER|END".to_owned(),
                syntax: SearchSyntax::Regex,
                case: SearchCase::Sensitive,
                scope,
                word: true,
                context_lines: 0,
                limit: 1,
                offset,
            };
            let search = round_trip(&search_query(content, &query).unwrap());
            assert_eq!(search.content_complete, complete, "{}", case["id"]);
            assert_eq!(search.total, 2, "{}", case["id"]);
            assert_eq!(search.returned, 1, "{}", case["id"]);
            assert_eq!(search.truncated, offset == 0, "{}", case["id"]);
            assert_eq!(search.next_offset, (offset == 0).then_some(1));
            let expected = if offset == 0 { "AFTER" } else { "END" };
            for occurrence in &search.matches[0].occurrences {
                let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                    ..usize::try_from(occurrence.markdown.end_byte).unwrap();
                assert_eq!(
                    artifact.text().get(range.clone()),
                    Some(expected),
                    "{}",
                    case["id"]
                );
                let prefix = &artifact.text()[..range.start];
                assert_eq!(
                    usize::try_from(occurrence.markdown.start_line).unwrap(),
                    prefix.bytes().filter(|byte| *byte == b'\n').count() + 1
                );
                assert_eq!(
                    usize::try_from(occurrence.markdown.start_column).unwrap(),
                    prefix.rsplit('\n').next().unwrap().chars().count() + 1
                );
            }
        }
    }
}

#[test]
fn changed_human_messages_do_not_change_the_actual_omission_summary() {
    let case = cases()
        .into_iter()
        .find(|case| case["metadata"]["native_guard_expected"] == true)
        .unwrap();
    let mut content = content(&case);
    for diagnostic in &mut content.document.as_mut().unwrap().diagnostics {
        diagnostic.message = "localized human detail".to_owned();
    }
    assert!(!build_outline(&content).unwrap().content_complete);
    let mut compact = build_outline(&content).unwrap();
    compact.diagnostics.clear();
    let restored = round_trip(&compact);
    assert!(!restored.content_complete);
    assert!(!restored.semantics_complete);
}

#[test]
fn owned_replay_keeps_the_rust_scanner_omission_fact_across_output_owners() {
    use libmandoc_rs::{Node, NodeKind, Parser};

    fn replace_text(node: &mut Node, source: &str) -> bool {
        if node.kind == NodeKind::Text && node.line == 8 {
            node.text = Some(source.to_owned());
            node.native_text = None;
            return true;
        }
        node.children
            .iter_mut()
            .any(|child| replace_text(child, source))
    }

    // This exact D01 source was frozen in all pristine profiles. Owned AST
    // replay can bypass native parsing: the bounded Rust scanner must retain
    // its own omission fact, rather than relying on the original native guard.
    let case = cases()
        .into_iter()
        .find(|case| {
            case["metadata"]["depth"] == 300
                && case["metadata"]["closed"] == true
                && case["metadata"]["carrier"] == "No"
                && case["metadata"]["prefix"] == "A"
        })
        .unwrap();
    let source = case["source"].as_str().unwrap();
    let word = source
        .lines()
        .find_map(|line| {
            line.strip_prefix(".No \"")
                .and_then(|word| word.strip_suffix('"'))
        })
        .unwrap();
    let mut report = Parser::default()
        .parse_bytes("owned.1", source.as_bytes())
        .unwrap();
    report.diagnostics.clear();
    assert!(replace_text(&mut report.document.root, word));
    let document = mant_codec::lower_mandoc_document(std::path::Path::new("owned.1"), &report);
    assert!(document.diagnostics.iter().any(|finding| {
        finding.code.as_deref() == Some("manual.escape-scan-truncated")
            && finding.impact == DiagnosticImpact::ContentCoverage
    }));
    assert!(!content_complete(&document.diagnostics));
    assert!(!semantics_complete(&document.diagnostics));
    assert!(serde_json::to_string(&document).unwrap().contains("AFTER"));
}
