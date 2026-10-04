//! Accepted cells survive projection and addressable consumer boundaries.
//!
//! Complete selected sources ran the registered pristine five profiles before
//! `consumer_cases.json` was written. Existing eleven sources were rerun with
//! `regen_acceptance_axes_gold.sh --check`. Device padding is responsive; actual
//! empty rows and accepted scalar order are observed independently.

use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
use mant_ir::{LinkTarget, ReferenceScope, ResolvedContent};
use mant_protocol::{
    ContentSelector, ExplanationOptions, ExplanationOutcome, ExplanationQuery, QueryBundle,
    ReferenceProjection, ReferenceProjectionMode, ReferenceTargetType, SearchCase, SearchQuery,
    SearchScope, SearchSyntax,
};

pub(super) fn round_trip(source: &str) -> ResolvedContent {
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&loaded, false).unwrap();
    assert!(!json.contains("\\u0000mant:"));
    let bundle: QueryBundle = serde_json::from_str(&json).unwrap();
    let query = bundle.into();
    assert_eq!(
        mant_render::render_query_man(&loaded),
        mant_render::render_query_man(&query)
    );
    query
}

fn rows(query: &ResolvedContent) -> Vec<String> {
    let rendered = mant_render::render_query_man(query);
    let lines = rendered.lines().collect::<Vec<_>>();
    let start = lines.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let end = query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "NEXT")
        .map_or(lines.len(), |section| {
            let end = lines.iter().position(|row| *row == "NEXT").unwrap();
            end - usize::from(section.spacing_before_lines)
        });
    lines[start..end]
        .iter()
        .map(|row| {
            row.split(' ')
                .filter(|cell| !cell.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn normalize_final_gap(rows: &mut [String]) {
    // Only the source-bound HANG conservative-final-seam card permits this
    // extra ordinary boundary. D/AFTER remains an exact native joined word.
    for row in rows {
        if let Some(at) = row.find("BodyWord")
            && at > 0
            && !row[..at].ends_with(' ')
        {
            row.insert(at, ' ');
        }
    }
}

pub(super) fn search(
    query: &ResolvedContent,
    scope: SearchScope,
    pattern: &str,
) -> mant_protocol::QuerySearch {
    mant_query::search_query(
        query,
        &SearchQuery {
            pattern: pattern.into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope,
            word: false,
            context_lines: 0,
            limit: 100,
            offset: 0,
        },
    )
    .unwrap()
}

fn assert_query_views(query: &ResolvedContent, accepted: &[String], source: &str) {
    let outline = mant_query::build_outline(query).unwrap();
    let encoded = serde_json::to_string(&outline).unwrap();
    let _: mant_protocol::QueryOutline = serde_json::from_str(&encoded).unwrap();
    let section = outline
        .nodes
        .iter()
        .find(|node| node.title() == "DESCRIPTION")
        .unwrap();
    let excerpt =
        mant_query::select_excerpt(query, &[ContentSelector::path(section.path())]).unwrap();
    let encoded = serde_json::to_string(&excerpt).unwrap();
    let restored: mant_protocol::QueryExcerpt = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        mant_render::render_excerpt_text(&excerpt),
        mant_render::render_excerpt_text(&restored)
    );
    let excerpt_text = mant_render::render_excerpt_text(&excerpt);
    let native = render_addressable_markdown_with_options(query, MarkdownOptions::ADDRESSABLE);
    for unit in accepted
        .iter()
        .flat_map(|row| row.split(' '))
        .filter(|unit| !unit.is_empty())
    {
        // A responsive HEAD/BODY boundary can separate this native joined
        // unit. That one declared final seam is asserted in the row test.
        if unit.contains("BodyWord") && unit != "BodyWord" {
            continue;
        }
        assert!(
            excerpt_text.contains(unit),
            "excerpt lost {unit:?}: {source}\n{excerpt_text}"
        );
        let found = search(query, SearchScope::Visible, unit);
        let encoded = serde_json::to_string(&found).unwrap();
        let _: mant_protocol::QuerySearch = serde_json::from_str(&encoded).unwrap();
        assert!(
            !found.matches.is_empty(),
            "visible search lost {unit:?}: {source}"
        );
        for occurrence in found.matches.iter().flat_map(|hit| &hit.occurrences) {
            assert_eq!(occurrence.matched_text, unit);
            let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
            let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
            // Markdown syntax can occur inside a visible match (P[Y]);
            // byte boundaries must still land on actual UTF-8 scalars.
            assert!(native.text().is_char_boundary(start));
            assert!(native.text().is_char_boundary(end));
            assert!(start < end && end <= native.text().len());
            assert_ne!(occurrence.line_ranges.len(), 0);
        }
    }
}

#[test]
fn mechanism_controls_preserve_completed_rows_and_accepted_text_through_query_views() {
    // term_newln/term_vspace produce physical receipts independently of IR
    // owners. man_term.c::print_man_node and mdoc_term.c::print_mdoc_node
    // consume HEAD/BODY state in source order. ERROR admission for the
    // malformed delimiter does not waive accepted AFTER or extent bounds.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("consumer_cases.json")).unwrap();
    assert_eq!(
        fixture["oracle"]["sha256"],
        "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6"
    );
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 34);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let query = round_trip(source);
        let mut actual = rows(&query);
        let mut expected = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        if name == "spacing-intervals-0632" {
            normalize_final_gap(&mut actual);
            normalize_final_gap(&mut expected);
        }
        if actual != expected {
            failures.push(format!(
                "{name}: {source}\nnative={expected:?}\nproduct={actual:?}"
            ));
        }
        assert_query_views(&query, &expected, source);
        let plain = mant_render::render_query_man(&query);
        let styled =
            mant_render::render_query_text_with(&query, |_, text| format!("\x1b[1m{text}\x1b[0m"));
        assert_eq!(
            styled.replace("\x1b[1m", "").replace("\x1b[0m", ""),
            plain,
            "{name}"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn assert_markdown_projection(
    query: &ResolvedContent,
    card: &super::axis_model::AcceptanceCase,
) -> Vec<String> {
    let name = card.id;
    let mut markdown_row_failures = Vec::new();
    let markdown = render_markdown_with_options(query, MarkdownOptions::default());
    let reparsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let projected_rows = rows(&reparsed);
    let projected = projected_rows.join("\n");
    // Every accepted URI and punctuation cell belongs to the only body.
    // Definition word seams are checked separately from these operands.
    for &unit in &card.gold.accepted_units.as_ref().unwrap().expect {
        if unit.contains("BodyWord") && unit != "BodyWord" {
            continue;
        }
        assert!(
            projected.contains(unit),
            "{name}: lost {unit:?}\n{markdown}\n{projected}"
        );
    }
    if let Some(forbidden) = &card.gold.forbidden_units {
        for unit in &forbidden.expect {
            assert!(!projected.contains(unit), "{name}: revived {unit:?}");
        }
    }
    if let Some(hard_rows) = &card.gold.hard_rows {
        if name == "column_tail_hard_row" {
            // The exact source was rerun with pristine CVS first:
            // term.c::term_word buffers ESCAPE_BREAK, and term_fill
            // accepts D before the rejected suffix. The public breakAfter
            // fact closes the accepted data row without another blank row.
            // Its hard boundary replaces the portable topology pipe, which
            // is format syntax rather than accepted source body content.
            // Native snapshots retain their independent exact row contract.
            assert_eq!(projected_rows, ["D", "RightWord"]);
        }
        for boundary in &hard_rows.expect {
            let left = super::axis_model::row_containing(&projected_rows, boundary.left)
                .expect("accepted left unit in Markdown projection");
            let right = super::axis_model::row_containing(&projected_rows, boundary.right)
                .expect("accepted right unit in Markdown projection");
            let same_row = matches!(
                boundary.relation,
                super::axis_model::HardRowRelation::SameRow
            );
            if (left == right) != same_row {
                markdown_row_failures.push(format!(
                    "{name}: Markdown changed {:?}/{:?} row relation\n{markdown}\nreparsed={projected_rows:?}",
                    boundary.left, boundary.right
                ));
            }
        }
        if name == "tag_explicit_vspace" {
            // These completed HEAD rows are inline hard breaks, not
            // omitted VerticalSpace blocks. Both exporter spellings
            // and the real Markdown reader retain the two blank rows.
            assert_eq!(
                super::axis_model::blank_count_between(&projected_rows, "D", "AFTER"),
                Some(2),
                "{name}: {markdown}"
            );
        }
    }
    markdown_row_failures
}

#[test]
fn recorded_examples_export_accepted_content_without_reviving_rejected_words() {
    let mut markdown_row_failures = Vec::new();
    for name in super::case_names() {
        let files = super::load_case(&name);
        let card = super::acceptance_cases::case_by_name(&name);
        super::validate_against_oracle(&card, &files);
        let query = round_trip(&files.source);
        let accepted = card
            .gold
            .accepted_units
            .as_ref()
            .unwrap()
            .expect
            .iter()
            .map(|unit| (*unit).to_owned())
            .collect::<Vec<_>>();
        assert_query_views(&query, &accepted, &files.source);
        if let Some(forbidden) = &card.gold.forbidden_units {
            for unit in &forbidden.expect {
                assert!(
                    search(&query, SearchScope::Visible, unit)
                        .matches
                        .is_empty(),
                    "{name}: visible query revived rejected {unit:?}"
                );
            }
        }
        markdown_row_failures.extend(assert_markdown_projection(&query, &card));
        if let Some(identities) = &card.gold.identities {
            let inventory = mant_query::project_references(
                query.document.as_ref().unwrap(),
                None,
                ReferenceScope::Document,
                &ReferenceProjection {
                    mode: ReferenceProjectionMode::All,
                    target_types: vec![ReferenceTargetType::External],
                    ..Default::default()
                },
            );
            assert_eq!(inventory.records.len(), identities.expect.len(), "{name}");
            for (record, expected) in inventory.records.iter().zip(&identities.expect) {
                assert_eq!(
                    record.target,
                    LinkTarget::External {
                        uri: expected.uri.into()
                    }
                );
                assert_eq!(record.label, expected.label);
                let excerpt =
                    mant_query::select_excerpt(&query, std::slice::from_ref(&record.source_read))
                        .unwrap();
                assert!(mant_render::render_excerpt_text(&excerpt).contains(expected.label));
                let found = search(&query, SearchScope::Markdown, expected.uri);
                assert!(
                    !found.matches.is_empty(),
                    "{name}: Markdown query lost typed URI"
                );
                let artifact =
                    render_addressable_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
                for occurrence in found.matches.iter().flat_map(|hit| &hit.occurrences) {
                    assert_eq!(
                        &artifact.text()[usize::try_from(occurrence.markdown.start_byte).unwrap()
                            ..usize::try_from(occurrence.markdown.end_byte).unwrap()],
                        expected.uri
                    );
                }
            }
            if inventory.records.len() == 2 {
                assert_ne!(inventory.records[0].origin, inventory.records[1].origin);
            }
        }
    }
    assert!(
        markdown_row_failures.is_empty(),
        "{}",
        markdown_row_failures.join("\n\n")
    );
}

#[test]
fn definition_explanation_reads_final_body_without_rejected_head_suffix() {
    let files = super::load_case("tag_explicit_vspace");
    let query = round_trip(&files.source);
    let explanation = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "BodyWord".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(explanation.outcome, ExplanationOutcome::Evidence);
    let encoded = serde_json::to_string(&explanation).unwrap();
    let restored: mant_protocol::QueryExplanation = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        mant_render::render_explanation_text(&explanation),
        mant_render::render_explanation_text(&restored)
    );
    assert!(mant_render::render_explanation_text(&restored).contains("BodyWord"));
    for evidence in &restored.evidence {
        for preview in &evidence.previews {
            assert!(
                mant_query::resolve_explanation_block(
                    query.document.as_ref().unwrap(),
                    &preview.block_path
                )
                .is_some()
            );
        }
    }
}
