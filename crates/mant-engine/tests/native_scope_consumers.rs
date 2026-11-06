//! Source-scope matrices reach final text, Markdown and public query coordinates.
#![cfg(feature = "roff")]

use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};
use mant_ir::{Block, ResolvedContent, visit::Visit};
use mant_protocol::{
    ExplanationContent, ExplanationOptions, ExplanationQuery, SearchCase, SearchQuery, SearchScope,
    SearchSyntax,
};
use serde_json::Value;

#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/table_scope_witness.rs"]
mod table_scope_witness;

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("roff_lowering/execution_scopes/cases.json"))
        .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}

fn word_positions<'a>(rows: impl Iterator<Item = &'a str>, word: &str) -> Vec<(usize, usize)> {
    rows.enumerate()
        .flat_map(|(row, text)| {
            text.match_indices(word).filter_map(move |(column, _)| {
                let end = column + word.len();
                let part = |character: char| character.is_alphanumeric() || character == '_';
                (!text[..column].chars().next_back().is_some_and(part)
                    && !text[end..].chars().next().is_some_and(part))
                .then_some((row, column))
            })
        })
        .collect()
}

fn assert_table_rows(text: &str, case: &Value) {
    for row in table_scope_witness::data_rows(case) {
        let positions = row
            .labels
            .iter()
            .map(|label| {
                table_scope_witness::cell(case, row.physical_row, label, |word| {
                    word_positions(text.lines(), word)
                })
            })
            .collect::<Vec<_>>();
        assert!(
            positions
                .windows(2)
                .all(|pair| pair[0].0 == pair[1].0 && pair[0].1 < pair[1].1)
        );
    }
}

fn assert_nested_table_markdown(text: &str, case: &Value) {
    // The original column list owns an outer semantic table. Its nested tbl
    // retains the established portable `; ` row spelling, not native table
    // device lines (table_projection::plain_block and nested_rows contracts).
    // This permission does not apply to authored literal/inline hard breaks.
    let owners = case["expected"]["table_owners"].as_array().unwrap();
    let expected = owners[0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| match row["kind"].as_str().unwrap() {
            "data" => row["cells"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" | "),
            "horizontal-rule" => "---".into(),
            other => panic!("outside original three-shape M2 domain: {other}"),
        })
        .collect::<Vec<_>>()
        .join("; ");
    assert_eq!(
        text.matches(&expected).count(),
        1,
        "{}: exact nested portable table row/cell separators: {text:?}",
        case["id"]
    );
    for row in table_scope_witness::data_rows(case) {
        for label in row.labels {
            assert_eq!(
                word_positions(text.lines(), label).len(),
                table_scope_witness::original_occurrences(case, label),
                "{}: all original nested and outer {label} owners",
                case["id"]
            );
        }
    }
}

#[derive(Default)]
struct Fences(Vec<String>);
impl<'a> Visit<'a> for Fences {
    fn visit_block(&mut self, block: &'a Block) {
        if let Block::Preformatted { children, .. } = block {
            self.0.push(mant_ir::inline_plain_text(children));
        } else {
            mant_ir::visit::walk_block(self, block);
        }
    }
}

#[test]
fn all_120_table_scope_sources_keep_native_cell_rows_through_two_markdown_readers() {
    // Five pristine profiles for each exact complete source precede its sealed
    // manifest. Both print_*_node dispatchers consume parent input once at the
    // first tbl span; tbl_term.c consumes subsequent data/rule spans itself.
    let mut checked = 0;
    for case in cases().into_iter().filter(|case| case["family"] == "M2") {
        checked += 1;
        let mut current = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        for round in 0..=2 {
            assert_eq!(
                mant_ir::validate_document(current.document.as_ref().unwrap()),
                []
            );
            for width in [40, 78] {
                let text = mant_render::render_query_man_at_width(&current, width);
                if round > 0 && case["axes"]["context"] == "column" {
                    assert_nested_table_markdown(&text, &case);
                } else {
                    assert_table_rows(&text, &case);
                }
            }
            if round > 0 {
                let mut fences = Fences::default();
                fences.visit_document(current.document.as_ref().unwrap());
                let selected = fences
                    .0
                    .iter()
                    .filter(|text| !word_positions(text.lines(), "A").is_empty())
                    .collect::<Vec<_>>();
                assert_eq!(
                    selected.len(),
                    1,
                    "{}: one accepted table fence",
                    case["id"]
                );
                if case["axes"]["context"] == "column" {
                    assert_nested_table_markdown(selected[0], &case);
                } else {
                    assert_table_rows(selected[0], &case);
                }
            }
            if round < 2 {
                let options = MarkdownOptions::default();
                let markdown = mant_codec::encode::render_markdown_with_options(&current, options);
                assert_eq!(
                    render_addressable_markdown_with_options(&current, options).text(),
                    markdown
                );
                current = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
            }
        }
    }
    assert_eq!(checked, 120);
}

fn assert_queries(content: &ResolvedContent, label: &str) {
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let result = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: label.into(),
                scope,
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                word: false,
                context_lines: 0,
                limit: 20,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(result.total, 1, "{label}/{scope:?}");
        let occurrences = result
            .matches
            .iter()
            .flat_map(|hit| &hit.occurrences)
            .collect::<Vec<_>>();
        assert_eq!(occurrences.len(), 1);
        let occurrence = occurrences[0];
        assert_eq!(occurrence.matched_text, label);
        let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
        let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(artifact.text().get(start..end), Some(label));
        for (at, line, column) in [
            (
                start,
                occurrence.markdown.start_line,
                occurrence.markdown.start_column,
            ),
            (
                end,
                occurrence.markdown.end_line,
                occurrence.markdown.end_column,
            ),
        ] {
            let prefix = &artifact.text()[..at];
            assert_eq!(
                usize::try_from(line).unwrap(),
                prefix.bytes().filter(|byte| *byte == b'\n').count() + 1
            );
            assert_eq!(
                usize::try_from(column).unwrap(),
                prefix.rsplit('\n').next().unwrap().chars().count() + 1
            );
        }
    }
    let explanation = mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: label.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let restored: mant_protocol::QueryExplanation =
        serde_json::from_str(&serde_json::to_string(&explanation).unwrap()).unwrap();
    assert_eq!(restored, explanation);
    let mut selected = 0;
    for evidence in &explanation.evidence {
        let Some(ExplanationContent::Entry { block } | ExplanationContent::Block { block }) =
            &evidence.content
        else {
            continue;
        };
        for range in evidence
            .previews
            .iter()
            .flat_map(|preview| &preview.content_ranges)
        {
            let text = range.resolve(block).unwrap().safe_text();
            let actual = text
                .chars()
                .skip(range.char_range().start)
                .take(range.char_range().len())
                .collect::<String>();
            assert_eq!(actual, label);
            selected += 1;
        }
    }
    assert_eq!(selected, 1, "one exact response-local preview for {label}");
}

#[test]
fn equal_uri_title_collision_cannot_rebind_search_explanation_or_export_metadata() {
    let source = "# Probe\n\n## OPTIONS\n\n- `--safe`: [DECLINED][r\x1bf] [ACCEPTED][r f]\n\n[r f]: https://same.test \"same\"\n[r\x1bf]: https://same.test \"same\"\n";
    let mut current = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_markdown_text(source, None).unwrap(),
    );
    for round in 0..=2 {
        assert_queries(&current, "DECLINED");
        assert_queries(&current, "ACCEPTED");
        let inventory = mant_query::project_references(
            current.document.as_ref().unwrap(),
            None,
            mant_ir::ReferenceScope::Document,
            &mant_protocol::ReferenceProjection {
                mode: mant_protocol::ReferenceProjectionMode::All,
                target_types: vec![mant_ir::ReferenceTargetType::External],
                ..Default::default()
            },
        );
        assert_eq!(inventory.records.len(), 1);
        assert_eq!(inventory.records[0].label, "ACCEPTED");
        assert_eq!(
            inventory.records[0].target,
            mant_ir::LinkTarget::External {
                uri: "https://same.test".into()
            }
        );
        if round < 2 {
            let markdown = mant_codec::encode::render_markdown(&current);
            current = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
        }
    }
}

#[test]
fn accepted_code_rows_keep_dual_owner_ranges_search_and_explanation_after_utf8_changes() {
    // Hand IR: original row1 is Z; bare CR introduces a second fragment of
    // row0. FIRST's source Link/Anchor remain authoritative even though a
    // portable table fence intentionally has no active Markdown link syntax.
    let mut current = mant_loader::load_markdown_text("# Owners\n", None).unwrap();
    current.document.as_mut().unwrap().blocks = vec![serde_json::from_value(serde_json::json!({
        "type":"table", "rows":[{"cells":[{"blocks":[
            {"type":"list","kind":{"kind":"plain"},"compact":true,"items":[{"entry":{"id":"first","kind":{"kind":"term"},"case":"sensitive","names":[],"forms":[]},"blocks":[{"type":"preformatted","children":[{"type":"anchor","id":"row-owner"},{"type":"link","target":{"kind":"external","uri":"https://row.test"},"children":[{"type":"text","value":"FIRST"}]},{"type":"text","value":"\r字\nZ"}],"inlineLayout":{"rowHints":[{"row":1,"indentColumns":3}]}}]}]},
            {"type":"list","kind":{"kind":"plain"},"compact":true,"items":[{"entry":{"id":"second","kind":{"kind":"term"},"case":"sensitive","names":[],"forms":[]},"blocks":[{"type":"paragraph","children":[{"type":"text","value":"SECOND"}]}]}]}
        ]}]}]
    })).unwrap()];
    current = json_roundtrip::json_string_roundtrip(&current);
    assert_eq!(
        mant_ir::validate_document(current.document.as_ref().unwrap()),
        []
    );
    let original = current.clone();
    for preserve_anchors in [false, true] {
        let options = MarkdownOptions {
            preserve_anchors,
            ..MarkdownOptions::default()
        };
        let artifact = render_addressable_markdown_with_options(&current, options);
        assert_eq!(
            artifact.text(),
            mant_codec::encode::render_markdown_with_options(&current, options)
        );
        let mut count = 0;
        for node in artifact.nodes() {
            if let mant_codec::encode::MarkdownNode::DocumentEntry { owner, .. } = node.node() {
                let id = owner.facts().unwrap().id.as_str();
                let expected = match id {
                    "first" => "FIRST\n字\n   Z",
                    "second" => "SECOND",
                    other => panic!("unexpected owner {other}"),
                };
                assert_eq!(artifact.text().get(node.range()), Some(expected));
                count += 1;
            }
        }
        assert_eq!(count, 2);
    }
    for round in 0..=2 {
        assert_queries(&current, "SECOND");
        if round < 2 {
            let markdown = mant_codec::encode::render_markdown(&current);
            current = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
            let mut fences = Fences::default();
            fences.visit_document(current.document.as_ref().unwrap());
            assert_eq!(fences.0, ["FIRST\n字\n   Z\nSECOND"]);
        }
    }
    assert_eq!(
        mant_ir::inline_plain_text(match &original.document.as_ref().unwrap().blocks[0] {
            Block::Table { rows, .. } => match &rows[0].cells[0].blocks[0] {
                Block::List { items, .. } => match &items[0].blocks[0] {
                    Block::Preformatted { children, .. } => children,
                    _ => panic!(),
                },
                _ => panic!(),
            },
            _ => panic!(),
        }),
        "FIRST\r字\nZ"
    );
}

#[test]
fn name_field_and_nested_display_markdown_readers_keep_original_row_witnesses() {
    let selected = cases()
        .into_iter()
        .filter(|case| {
            let id = case["id"].as_str().unwrap();
            (case["family"] == "M1-core"
                && ["sy_x_", "sy_long_command_with_ample_field_width_"]
                    .iter()
                    .any(|prefix| id.starts_with(prefix))
                && ["_br", "_rs"].iter().any(|suffix| id.ends_with(suffix)))
                || (case["family"] == "M3"
                    && ["root", "tag-enum", "tag-bullet"]
                        .iter()
                        .any(|context| case["axes"]["context"] == *context))
        })
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 20, "eight SY and twelve display controls");
    for case in selected {
        let mut current = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        for round in 0..=2 {
            let text = mant_render::render_query_man(&current);
            if case["family"] == "M1-core" {
                let labels = ["alpha", "beta", "gamma", "delta"];
                let locations = labels.map(|label| {
                    let found = word_positions(text.lines(), label);
                    assert_eq!(found.len(), 1, "{}/{round}: {label}", case["id"]);
                    found[0]
                });
                assert!(
                    locations
                        .iter()
                        .all(|location| location.0 == locations[0].0)
                );
                let row = text.lines().nth(locations[0].0).unwrap();
                assert_eq!(
                    &row[locations[0].1..locations[3].1 + "delta".len()],
                    "alpha beta gamma delta"
                );
            } else {
                let locations = ["BEFORE", "DISPLAY", "AFTER", "TAIL"].map(|label| {
                    let found = word_positions(text.lines(), label);
                    assert_eq!(found.len(), 1, "{}/{round}: {label}", case["id"]);
                    found[0]
                });
                assert!(
                    locations.windows(2).all(|pair| pair[0].0 < pair[1].0),
                    "{}/{round}: original display/body order",
                    case["id"]
                );
                if round > 0 && case["axes"]["display"] != "filled" {
                    let mut fences = Fences::default();
                    fences.visit_document(current.document.as_ref().unwrap());
                    assert_eq!(
                        fences
                            .0
                            .iter()
                            .filter(|payload| payload.as_str() == "DISPLAY")
                            .count(),
                        1,
                        "{}/{round}: original portable literal owner and exact payload",
                        case["id"]
                    );
                }
            }
            if round < 2 {
                let markdown = mant_codec::encode::render_markdown(&current);
                current = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
            }
        }
    }
}

#[test]
fn name_field_exit_keeps_source_font_roles_after_two_portable_readers() {
    // Exact identity-styles-br source ran the five pinned profiles. .B alpha
    // and .I beta are separate man font pre/post calls after post_SY BODY;
    // neither retired name geometry nor subsequent owner drains erase styles.
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "identity-styles-br")
        .unwrap();
    let mut current = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
    );
    for round in 0..=2 {
        #[derive(Default)]
        struct Styles(Vec<(String, bool, bool)>);
        impl<'a> Visit<'a> for Styles {
            fn visit_block(&mut self, block: &'a Block) {
                match block {
                    Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                        mant_render::visit_inline_text(children, &[], |role, _, text| {
                            for word in text
                                .split_whitespace()
                                .filter(|word| ["alpha", "beta", "gamma", "delta"].contains(word))
                            {
                                self.0.push((word.into(), role.strong, role.emphasis));
                            }
                        });
                    }
                    _ => mant_ir::visit::walk_block(self, block),
                }
            }
        }
        let mut styles = Styles::default();
        styles.visit_document(current.document.as_ref().unwrap());
        assert_eq!(
            styles.0,
            [
                ("alpha".into(), true, false),
                ("beta".into(), false, true),
                ("gamma".into(), false, false),
                ("delta".into(), false, false)
            ],
            "round{round}: source roles"
        );
        if round < 2 {
            let markdown = mant_codec::encode::render_markdown(&current);
            current = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
        }
    }
}
