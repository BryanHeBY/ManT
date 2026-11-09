//! Source-bound annotation identities through every public read representation.
use crate::{cases, json_roundtrip, queries};
use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
use mant_ir::{LinkTarget, ReferenceTargetType, ResolvedContent};
use mant_protocol::{
    EntryProjection, ReferenceCount, ReferenceProjection, ReferenceProjectionMode, SearchCase,
    SearchQuery, SearchScope, SearchSyntax,
};
use serde_json::Value;

fn policy() -> ReferenceProjection {
    ReferenceProjection {
        mode: ReferenceProjectionMode::All,
        target_types: vec![
            ReferenceTargetType::Document,
            ReferenceTargetType::Manual,
            ReferenceTargetType::Local,
            ReferenceTargetType::External,
            ReferenceTargetType::Email,
        ],
        ..Default::default()
    }
}

fn check_inventory(content: &ResolvedContent, expected: &[Value], id: &str) {
    let outline =
        mant_query::build_outline_with_references(content, EntryProjection::All, None, &policy())
            .unwrap();
    assert_eq!(
        outline.references.coverage.status,
        mant_protocol::ReferenceCoverageStatus::Complete {},
        "{id}"
    );
    let count = u64::try_from(expected.len()).unwrap();
    let targets = expected
        .iter()
        .map(|link| serde_json::to_string(&link["target"]).unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        outline.references.occurrences,
        ReferenceCount::Exact { value: count },
        "{id}"
    );
    assert_eq!(
        outline.references.targets,
        ReferenceCount::Exact {
            value: u64::try_from(targets.len()).unwrap()
        },
        "{id}"
    );
    assert_eq!(outline.references.records.len(), expected.len(), "{id}");
    assert!(outline.references.page.next_offset.is_none(), "{id}");
    for (offset, wanted) in expected.iter().enumerate() {
        let mut page = policy();
        page.offset = u32::try_from(offset).unwrap();
        page.limit = 1;
        let paged =
            mant_query::build_outline_with_references(content, EntryProjection::All, None, &page)
                .unwrap();
        assert_eq!(
            paged.references.occurrences,
            ReferenceCount::Exact { value: count }
        );
        assert_eq!(paged.references.records.len(), 1);
        assert_eq!(
            paged.references.coverage.status,
            mant_protocol::ReferenceCoverageStatus::Complete {}
        );
        assert_eq!(
            paged.references.page.next_offset,
            (offset + 1 < expected.len()).then(|| u32::try_from(offset + 1).unwrap())
        );
        let record = &paged.references.records[0];
        let target: LinkTarget = serde_json::from_value(wanted["target"].clone()).unwrap();
        assert_eq!(record.target, target, "{id}");
        assert_eq!(record.label, wanted["label"].as_str().unwrap(), "{id}");
        let Some(mant_ir::Inline::Link {
            target: actual,
            children,
            ..
        }) = record
            .origin
            .resolve_link(content.document.as_ref().unwrap())
        else {
            panic!("{id}: original link origin");
        };
        assert_eq!(actual, &target);
        assert_eq!(mant_ir::inline_plain_text(children), record.label);
        assert!(
            mant_query::select_excerpt(content, std::slice::from_ref(&record.source_read)).is_ok()
        );
    }
    let restored: mant_protocol::QueryOutline =
        serde_json::from_str(&serde_json::to_string(&outline).unwrap()).unwrap();
    assert_eq!(outline, restored);
}

fn query_case(case: &Value) -> cases::Case {
    cases::Case {
        id: case["id"].as_str().unwrap().into(),
        family: "annotation".into(),
        source: case["source"].as_str().unwrap().into(),
        links: Vec::new(),
        recoveries: Vec::new(),
        reading_rows: None,
        diagnostics: None,
        old_x: None,
        constraints: Vec::new(),
        after_style: cases::Style::default(),
        facts: None,
    }
}

fn check_queries(content: &ResolvedContent, case: &Value) {
    let snapshot = cases::snapshot(content);
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    let mut words = std::collections::BTreeSet::new();
    for link in case["expected"]["links"].as_array().unwrap() {
        words.extend(
            link["label"]
                .as_str()
                .unwrap()
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|s| s.chars().count() >= 2)
                .map(str::to_owned),
        );
    }
    for word in case["expected"]["outside_link_literals"]
        .as_array()
        .unwrap()
    {
        words.extend(
            word.as_str()
                .unwrap()
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|word| word.chars().count() >= 2)
                .map(str::to_owned),
        );
    }
    for word in words {
        assert!(
            snapshot.text().contains(&word),
            "{} missing source member {word}",
            case["id"]
        );
        for scope in [SearchScope::Visible, SearchScope::Markdown] {
            let response = mant_query::search_query(
                content,
                &SearchQuery {
                    pattern: word.clone(),
                    scope,
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Sensitive,
                    word: false,
                    context_lines: 0,
                    limit: 100,
                    offset: 0,
                },
            )
            .unwrap();
            let hits = response
                .matches
                .iter()
                .flat_map(|hit| &hit.occurrences)
                .collect::<Vec<_>>();
            assert!(!hits.is_empty(), "{} {scope:?} {word}", case["id"]);
            for hit in hits {
                queries::check_occurrence(
                    hit,
                    artifact.text(),
                    &word,
                    scope,
                    case["id"].as_str().unwrap(),
                );
            }
            assert_eq!(
                serde_json::from_str::<mant_protocol::QuerySearch>(
                    &serde_json::to_string(&response).unwrap()
                )
                .unwrap(),
                response
            );
        }
        queries::check_explanation(content, &query_case(case), &word);
    }
}

fn expected_portable(
    original: &cases::Snapshot,
    case: &Value,
    options: MarkdownOptions,
) -> Vec<Value> {
    case["expected"]["links"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&original.links)
        .filter(|(gold, actual)| {
            // Frozen export policy selects which original identities remain;
            // labels and targets always come from independently frozen source gold.
            let target: LinkTarget = serde_json::from_value(gold["target"].clone()).unwrap();
            target.to_uri().is_some()
                && !matches!(target, LinkTarget::Manual { .. })
                && !(actual.owner.contains("/preformatted")
                    || actual.owner.contains("/row/")
                    || options == MarkdownOptions::default()
                        && matches!(
                            target,
                            LinkTarget::Section { .. } | LinkTarget::Manual { .. }
                        ))
        })
        .map(|(gold, _)| gold.clone())
        .collect()
}

#[test]
fn source_annotation_matrix_survives_json_inventory_queries_and_two_markdown_readers() {
    // All 162 complete inputs have pre-assertion pristine five-profile receipts
    // and independent exact link/member gold in the shared codec fixture.
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../mant-codec/src/mandoc/tests/annotation_ownership/boundaries.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let original = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        let snapshot = cases::snapshot(&original);
        let expected = case["expected"]["links"].as_array().unwrap();
        assert_eq!(snapshot.links.len(), expected.len(), "{id}");
        check_content(
            &original,
            case,
            expected,
            false,
            &std::collections::BTreeMap::new(),
        );
        check_inventory(&original, expected, id);
        check_queries(&original, case);
        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
            let expected = expected_portable(&snapshot, case, options);
            for direct in [false, true] {
                let mut current = original.clone();
                let mut navigation = std::collections::BTreeMap::new();
                for round in 1..=2 {
                    if options.preserve_anchors {
                        for literal in navigation_literals(&current) {
                            *navigation.entry(literal).or_default() += 1;
                        }
                    }
                    let markdown = render_markdown_with_options(&current, options);
                    let artifact = render_addressable_markdown_with_options(&current, options);
                    assert_eq!(artifact.text(), markdown, "{id} round{round}");
                    assert!(!markdown.contains("mant:annotation"));
                    for node in artifact.nodes() {
                        assert!(
                            markdown.get(node.range()).is_some(),
                            "{id}: artifact boundary"
                        );
                    }
                    current = if direct {
                        let parsed = mant_codec::parse_markdown(&markdown, None).unwrap();
                        ResolvedContent {
                            address: None,
                            label: original.label.clone(),
                            document: Some(parsed.document),
                            tldr: parsed.tldr,
                        }
                    } else {
                        mant_loader::load_markdown_text(&markdown, None).unwrap()
                    };
                    current = json_roundtrip::json_string_roundtrip(&current);
                    check_content(&current, case, &expected, true, &navigation);
                    check_inventory(
                        &current,
                        &expected,
                        &format!("{id} {direct} {options:?} round{round}"),
                    );
                    check_queries(&current, case);
                }
            }
        }
    }
}

fn check_content(
    content: &ResolvedContent,
    case: &Value,
    links: &[Value],
    portable: bool,
    navigation: &std::collections::BTreeMap<String, usize>,
) {
    let snapshot = cases::snapshot(content);
    for (index, expected) in links.iter().enumerate() {
        let Some(spans) = expected["scalar_styles"].as_array() else {
            continue;
        };
        let actual = &snapshot.links[index];
        check_styles(
            &snapshot.cells[actual.start..actual.end],
            spans,
            case,
            portable,
        );
    }
    if let Some(text) = case["expected"]["body_text"].as_str() {
        let cells = snapshot
            .cells
            .iter()
            .filter(|cell| {
                cell.owner.ends_with("/paragraph") || cell.owner.ends_with("/preformatted")
            })
            .collect::<Vec<_>>();
        for literal in navigation.keys() {
            assert!(
                !text.contains(literal),
                "fixture payload must not alias a generated raw anchor"
            );
        }
        let cells = payload_cells(cells, navigation, case);
        assert_eq!(
            cells.iter().map(|cell| cell.value).collect::<String>(),
            text,
            "{} exact source body",
            case["id"]
        );
        let cloned = cells
            .into_iter()
            .map(|cell| cases::Cell {
                value: cell.value,
                style: cell.style,
                link: cell.link,
                owner: cell.owner.clone(),
            })
            .collect::<Vec<_>>();
        let spans = case["expected"]["body_scalar_styles"]
            .as_array()
            .unwrap()
            .clone();

        check_styles(&cloned, &spans, case, portable);
    }
}

fn check_styles(cells: &[cases::Cell], spans: &[Value], case: &Value, portable: bool) {
    let mut expected = Vec::new();
    for span in spans {
        let style = span["style"].as_str().unwrap();
        expected.extend(span["text"].as_str().unwrap().chars().map(|ch| {
            (
                ch,
                if ch == '\n' {
                    cases::Style::default()
                } else {
                    cases::Style {
                        strong: style == "strong",
                        emphasis: style == "emphasis",
                        code: style == "code",
                    }
                },
            )
        }));
    }
    if portable {
        // mant-markdown(7): Unicode edge whitespace remains literal but
        // lies outside Strong/Emphasis delimiters. Inner blanks and Code
        // membership remain strict; LF itself has no painted font.
        let before = expected.clone();
        for (at, (ch, style)) in expected.iter_mut().enumerate() {
            if ch.is_whitespace() && (style.strong || style.emphasis) {
                let same_run = |(ch, other): &(char, cases::Style)| *ch == '\n' || other == style;
                let left = before[..at]
                    .iter()
                    .rev()
                    .take_while(|item| same_run(item))
                    .any(|(ch, _)| !ch.is_whitespace());
                let right = before[at + 1..]
                    .iter()
                    .take_while(|item| same_run(item))
                    .any(|(ch, _)| !ch.is_whitespace());
                if !left || !right {
                    *style = cases::Style::default();
                }
            }
        }
    }
    let actual: Vec<_> = cells
        .iter()
        .map(|cell| {
            (
                cell.value,
                if cell.value == '\n' {
                    cases::Style::default()
                } else {
                    cell.style
                },
            )
        })
        .collect();
    assert_eq!(actual, expected, "{} original scalar style", case["id"]);
}

fn navigation_literals(content: &ResolvedContent) -> Vec<String> {
    use mant_ir::visit::{self, Visit};
    #[derive(Default)]
    struct Anchors(Vec<String>);
    impl<'a> Visit<'a> for Anchors {
        fn visit_inline(&mut self, node: &'a mant_ir::Inline) {
            if let mant_ir::Inline::Anchor { id, .. } = node {
                self.0.push(mant_codec::encode::html_anchor(id.as_str()));
            }
            visit::walk_inline(self, node);
        }
    }
    let mut anchors = Anchors::default();
    anchors.visit_section(cases::description(content));
    // Addressable HTML for the following heading precedes its Markdown
    // heading and is read as a literal fallback in the preceding section.
    let sections = &content.document.as_ref().unwrap().sections;
    let at = sections
        .iter()
        .position(|section| section.id == cases::description(content).id)
        .unwrap();
    if let Some(next) = sections.get(at + 1) {
        // Consecutive anchor lines for a heading and its aliases become one
        // literal fallback paragraph. Its soft separators belong to that
        // exact generated block. A readback can derive a new heading ID, so
        // record the contribution at each export rather than assuming it
        // will repeat the original native ID.
        let block = std::iter::once(next.id.as_str())
            .chain(
                next.fragment_aliases
                    .iter()
                    .map(mant_ir::FragmentAlias::as_str)
                    .filter(|alias| *alias != next.id.as_str()),
            )
            .map(mant_codec::encode::html_anchor)
            .collect::<Vec<_>>()
            .join(" ");
        anchors.0.push(block);
    }
    anchors.0
}

fn payload_cells<'a>(
    cells: Vec<&'a cases::Cell>,
    navigation: &std::collections::BTreeMap<String, usize>,
    case: &Value,
) -> Vec<&'a cases::Cell> {
    let text: String = cells.iter().map(|cell| cell.value).collect();
    let mut excluded = std::collections::BTreeSet::new();
    for (literal, count) in navigation {
        let positions = cases::positions(&text, literal);
        assert_eq!(
            positions.len(),
            *count,
            "{} explicit generated raw-anchor contribution {literal}",
            case["id"]
        );
        for start in positions {
            for (at, cell) in cells
                .iter()
                .enumerate()
                .skip(start)
                .take(literal.chars().count())
            {
                assert_eq!(cell.style, cases::Style::default());
                assert!(cell.link.is_none());
                assert!(excluded.insert(at), "navigation receipts cannot overlap");
            }
        }
    }
    cells
        .into_iter()
        .enumerate()
        .filter_map(|(at, cell)| (!excluded.contains(&at)).then_some(cell))
        .collect()
}
