use crate::table_matrix::*;
use mant_ir::{Block, ResolvedContent, visit::Visit};
use mant_ui::DocumentView;
use serde_json::Value;

const CASES: &str = include_str!("../../../../tests/fixtures/roff/table_layout/real_cases.json");

struct Tables<'a>(Vec<&'a Block>);
impl<'a> Visit<'a> for Tables<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        if matches!(block, Block::Table { .. }) {
            self.0.push(block);
        }
        mant_ir::visit::walk_block(self, block);
    }
}

#[test]
fn real_manual_tables_keep_source_topology_and_viewport_layout() {
    // All seven complete immutable pages ran pinned pristine ASCII/UTF-8,
    // HTML/tree/lint before these assertions. Nonzero lint is recorded in
    // real_cases.json; these are not advertised as diagnostic-free pages.
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let mut table_count = 0;
    let mut sample_count = 0;
    for case in cases["cases"].as_array().unwrap() {
        let name = case["file"].as_str().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/roff/real")
            .join(name);
        let document = mant_loader::parse_manual_source(&path).unwrap();
        let loaded = ResolvedContent {
            label: name.to_owned(),
            address: None,
            document: Some(document),
            tldr: None,
        };
        let restored = restored(&loaded);
        let mut tables = Tables(vec![]);
        tables.visit_document(restored.document.as_ref().unwrap());
        assert_eq!(
            tables.0.len(),
            usize::try_from(case["tableCount"].as_u64().unwrap()).unwrap(),
            "{name}"
        );
        table_count += tables.0.len();
        for sample in case["samples"].as_array().unwrap() {
            sample_count += 1;
            assert_sample(name, sample, &tables.0);
        }
    }
    assert_eq!(table_count, 104);
    assert_eq!(sample_count, 12);
}

fn assert_sample(name: &str, sample: &Value, tables: &[&Block]) {
    let markers = sample["markers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|marker| marker.as_str().unwrap())
        .collect::<Vec<_>>();
    let index = usize::try_from(sample["index"].as_u64().unwrap()).unwrap();
    let table = tables[index];
    let Block::Table {
        column_count,
        borders,
        ..
    } = table
    else {
        unreachable!()
    };
    assert_eq!(
        column_count.map(u64::from),
        sample["columns"].as_u64(),
        "{name}, table {index}"
    );
    assert_eq!(
        serde_json::to_value(borders.outer).unwrap(),
        sample["outer"],
        "{name}, table {index}"
    );
    // Each selected table is read from the complete real source, not
    // reparsed as a context-free fragment. The isolated consumer root
    // lets marker assertions refer to original cells unambiguously.
    let value = query(serde_json::to_value(table).unwrap());
    let view = DocumentView::new(&value);
    for width in [40_u16, 80, 120, 40] {
        let rendered = view.render(width);
        for marker in sample["markers"].as_array().unwrap() {
            let marker = marker.as_str().unwrap();
            assert_eq!(
                rendered.search(marker).len(),
                1,
                "{name}, table {index}, {width}, {marker}: {:?}",
                rendered.text
            );
        }
        assert_buffer(&rendered, width, &markers);
        assert_no_decoration_search(&rendered);
        if borders.outer.is_some() && width >= 80 {
            let rows = rows(&rendered);
            let (_, right, _, _) = frame(&rows, sample["outer"] == "double");
            assert!(right < usize::from(width), "{name}, table {index}");
        }
        // Both user-reported awk tables must remain parallel at a
        // useful viewport. Exact native columns are not a screenshot
        // contract; body/rules/frame share the allocated reader area.
        if name == "archlinux/awk.1p.gz" && width >= 80 {
            assert_awk_headers(&rendered, index);
        }
    }
    assert_markers(&value, 120, &markers);
    let markdown = mant_codec::encode::render_markdown(&value);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let text = mant_render::render_query_text(&readback);
    for marker in sample["markers"].as_array().unwrap() {
        assert_eq!(
            text.matches(marker.as_str().unwrap()).count(),
            1,
            "{name}, table {index}: {markdown}"
        );
    }
}

fn assert_awk_headers(rendered: &mant_ui::RenderedDocument, index: usize) {
    if index == 1 {
        let escape = &rendered.search("Escape")[0];
        let sequence = &rendered.search("Sequence")[0];
        let description = &rendered.search("Description")[0];
        let meaning = &rendered.search("Meaning")[0];
        assert_eq!(description.row, sequence.row);
        assert_eq!(meaning.row, sequence.row);
        assert!(sequence.row > escape.row);
        assert!(sequence.end_column < description.start_column);
        assert!(description.end_column < meaning.start_column);
    } else if index == 0 {
        let headers = ["Syntax", "Name", "Type of Result", "Associativity"]
            .map(|label| rendered.search(label).into_iter().next().unwrap());
        assert!(
            headers.windows(2).all(|pair| pair[0].row == pair[1].row
                && pair[0].end_column < pair[1].start_column),
            "four real precedence columns must remain parallel: {:?}",
            rendered.text
        );
    }
}
