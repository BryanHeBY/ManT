//! Native column entry, row flush and following content are separate facts.
use mant_ir::{Block, DefinitionItem, Section, visit::Visit};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

#[derive(Deserialize)]
struct Fixture {
    sources: BTreeMap<String, NativeSource>,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    source_sha256: String,
    family: String,
}
#[derive(Deserialize)]
struct NativeSource {
    source: String,
    expected_label: String,
    owner: Owner,
    exit: Exit,
    #[serde(default)]
    expected_tables: Vec<ExpectedTable>,
}
#[derive(Deserialize)]
struct Owner {
    heading: String,
}
#[derive(Deserialize)]
struct Exit {
    word: String,
}

#[derive(Deserialize)]
struct ExpectedTable {
    source_line: u32,
    columns: u32,
    rows: Vec<ExpectedRow>,
}
#[derive(Deserialize)]
struct ExpectedRow {
    kind: String,
    spans: Vec<(u32, u32)>,
}
#[derive(Default)]
struct Tables<'a>(Vec<&'a Block>);
impl<'a> Visit<'a> for Tables<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        if matches!(block, Block::Table { .. }) {
            self.0.push(block);
        }
        mant_ir::visit::walk_block(self, block);
    }
}
fn assert_topology(document: &mant_ir::Document, expected: &[ExpectedTable]) {
    let mut tables = Tables::default();
    tables.visit_document(document);
    for expected in expected {
        let owners=tables.0.iter().filter(|block|matches!(block,Block::Table{source:Some(source),..} if source.line==expected.source_line)).collect::<Vec<_>>();
        assert_eq!(
            owners.len(),
            1,
            "original native table owner at {}",
            expected.source_line
        );
        let Block::Table {
            column_count, rows, ..
        } = owners[0]
        else {
            unreachable!()
        };
        assert_eq!(*column_count, Some(expected.columns));
        assert_eq!(rows.len(), expected.rows.len());
        for (row, expected) in rows.iter().zip(&expected.rows) {
            let kind = match row.kind {
                mant_ir::TableRowKind::Data => "data",
                mant_ir::TableRowKind::HorizontalRule => "horizontal-rule",
                mant_ir::TableRowKind::DoubleHorizontalRule => "double-horizontal-rule",
                mant_ir::TableRowKind::LayoutRule { .. } => "layout-rule",
            };
            assert_eq!(kind, expected.kind);
            assert_eq!(
                row.cells
                    .iter()
                    .map(|cell| (cell.column_span, cell.row_span))
                    .collect::<Vec<_>>(),
                expected.spans
            );
        }
    }
}

fn positions(text: &str, word: &str) -> Vec<usize> {
    let part = |character: char| character.is_alphanumeric() || matches!(character, '_' | '-');
    text.match_indices(word)
        .filter_map(|(start, _)| {
            let end = start + word.len();
            (!text[..start].chars().next_back().is_some_and(part)
                && !text[end..].chars().next().is_some_and(part))
            .then_some(start)
        })
        .collect()
}

#[derive(Default)]
struct Roots(Vec<String>);
impl<'a> Visit<'a> for Roots {
    fn visit_block(&mut self, block: &'a Block) {
        if let Block::Paragraph { children, .. } | Block::Preformatted { children, .. } = block {
            self.0.push(mant_ir::inline_plain_text(children));
        } else {
            mant_ir::visit::walk_block(self, block);
        }
    }
    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        self.0.extend(
            item.terms
                .iter()
                .map(|term| mant_ir::inline_plain_text(term)),
        );
        for block in &item.description {
            self.visit_block(block);
        }
    }
}

fn section<'a>(sections: &'a [Section], heading: &str) -> Option<&'a Section> {
    sections
        .iter()
        .find(|section| section.heading.plain_text() == heading)
        .or_else(|| {
            sections
                .iter()
                .find_map(|item| section(&item.children, heading))
        })
}

fn verify(source: &NativeSource) {
    let original = mant_loader::load_roff_bytes(source.source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&original);
    assert_eq!(original, content);
    let document = content.document.as_ref().unwrap();
    assert_eq!(mant_ir::validate_document(document), []);
    assert!(
        !document
            .diagnostics
            .iter()
            .any(|finding| finding.code.as_deref() == Some("manual.table-execution-incomplete")),
        "supported native table input must have complete execution facts"
    );
    assert_topology(document, &source.expected_tables);
    let owner = section(&document.sections, &source.owner.heading).expect("original source owner");
    let mut roots = Roots::default();
    for block in &owner.blocks {
        roots.visit_block(block);
    }
    assert_eq!(
        roots
            .0
            .iter()
            .map(|root| positions(root, &source.expected_label).len())
            .sum::<usize>(),
        1,
        "one complete native follower, in its original section/inline owner: {:?}",
        roots.0
    );
    for width in [40, 78] {
        let text = mant_render::render_query_man_at_width(&content, width);
        let label = positions(&text, &source.expected_label);
        let beta = positions(&text, "BETA");
        let tail = positions(&text, &source.exit.word);
        assert_eq!((label.len(), beta.len(), tail.len()), (1, 1, 1), "{text}");
        assert!(
            label[0] < beta[0] && beta[0] < tail[0],
            "native source order: {text}"
        );
    }
}

#[test]
fn all_declared_column_and_parent_scope_programs_keep_their_native_followers() {
    // Exact complete sources ran all five pristine profiles before this file
    // and its source-bound gold were written. tbl_term.c fills ALL columns
    // before any term_flushln; empty declared slots still clear BACK*.
    // Opposite controls intentionally expect LPHA/IRST, never an invented A/F.
    let fixture: Fixture =
        serde_json::from_str(include_str!("table_execution/cases.json")).unwrap();
    assert_eq!(fixture.cases.len(), 1188);
    assert_eq!(fixture.sources.len(), 1045);
    assert_eq!(
        fixture
            .cases
            .iter()
            .filter(|case| case.family == "T1")
            .count(),
        192
    );
    assert_eq!(
        fixture
            .cases
            .iter()
            .filter(|case| case.family == "T2")
            .count(),
        240
    );
    verify_cases(
        fixture.cases.iter(),
        |case| case.id.as_str(),
        |case| {
            verify(&fixture.sources[&case.source_sha256]);
        },
    );
}

#[test]
fn invisible_native_writes_and_effective_widths_keep_opposite_flush_controls() {
    // Exact pristine profiles preceded this source-bound gold. In particular,
    // term.c::ESCAPE_HORIZ cancels BACKAFTER before parsing its argument;
    // OVERSTRIKE calls encode1 even for blanks, and tblcalc_number measures
    // raw font-argument digits rather than the projected cell string.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("table_execution/adjacent.json")).unwrap();
    verify_cases(
        fixture["cases"].as_array().unwrap(),
        |case| case["id"].as_str().unwrap(),
        |case| {
            let content = json_string_roundtrip(
                &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
            );
            let label = case["expected_label"].as_str().unwrap();
            let document = content.document.as_ref().unwrap();
            assert_eq!(mant_ir::validate_document(document), []);
            assert!(
                !document
                    .diagnostics
                    .iter()
                    .any(|finding| finding.code.as_deref()
                        == Some("manual.table-execution-incomplete")),
                "complete safe measurement: {:?}",
                document.diagnostics
            );
            for width in [40, 78] {
                let text = mant_render::render_query_man_at_width(&content, width);
                assert_eq!(positions(&text, label).len(), 1, "{text}");
                assert_eq!(positions(&text, "BETA").len(), 1, "{text}");
            }
            if case["id"].as_str().unwrap().starts_with("authored-blank-")
                && case["id"].as_str().unwrap().ends_with("-wide")
            {
                let mut roots = Roots::default();
                roots.visit_document(document);
                assert!(
                    roots.0.iter().any(|root| root == "A B"),
                    "author blank precedes suppressed motion: {:?}",
                    roots.0
                );
            }
        },
    );
}

#[test]
fn motion_receipts_keep_word_spacing_and_empty_words_clear_spacing_debt() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("table_execution/motion_controls.json")).unwrap();
    verify_cases(
        fixture["cases"].as_array().unwrap(),
        |case| case["id"].as_str().unwrap(),
        |case| {
            let content = json_string_roundtrip(
                &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
            );
            if let Some(expected) = case["expected_inline_text"].as_str() {
                // term.c::ESCAPE_HORIZ first clears BACKBEFORE without
                // writing its first cell. These exact full sources ran
                // pristine ASCII/UTF-8/HTML/tree/lint before this assertion.
                let mut roots = Roots::default();
                roots.visit_document(content.document.as_ref().unwrap());
                assert!(roots.0.iter().any(|root| root == expected), "{:?}", roots.0);
            }
            if case["native_x_space_y"] == true {
                let text = mant_render::render_query_man(&content);
                assert_eq!(positions(&text, "X Y").len(), 1, "{text}");
            }
            if case["id"] == "empty-word-clears-debt" {
                let document = content.document.as_ref().unwrap();
                assert_eq!(document.sections.iter().flat_map(|section|&section.blocks).filter(|block|matches!(block,Block::VerticalSpace{lines:1,source:Some(source)} if source.line==10)).count(),1,"real empty tbl term_word clears skipvsp before source sp1");
            }
        },
    );
}
