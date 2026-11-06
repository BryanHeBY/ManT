//! Entire original manuals retain every tbl and selected following owners.
#![cfg(feature = "roff")]
use mant_ir::Block;
#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/field_boundary_real.rs"]
mod real;

#[test]
fn seven_complete_manuals_keep_104_tables_and_their_original_following_owners() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/roff/table_layout/real_followers.json"
    ))
    .unwrap();
    let mut tables = 0;
    let mut witnesses = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let file = case["file"].as_str().unwrap();
        let mut current = json_roundtrip::json_string_roundtrip(&real::load(file));
        let blocks = real::blocks(&current);
        let actual = blocks
            .iter()
            .filter_map(|block| {
                if let Block::Table { source, .. } = block {
                    Some(source.as_ref().expect("native table source"))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual.len(),
            usize::try_from(case["table_count"].as_u64().unwrap()).unwrap(),
            "{file}"
        );
        tables += actual.len();
        for boundary in case["owner_boundaries"].as_array().unwrap() {
            let start = boundary["ts_line"].as_u64().unwrap();
            let end = boundary["te_line"].as_u64().unwrap();
            assert_eq!(
                actual
                    .iter()
                    .filter(|source| u64::from(source.line) > start && u64::from(source.line) < end)
                    .count(),
                1,
                "{file}: {start}..{end}"
            );
        }
        for round in 0..=2 {
            for witness in case["witnesses"].as_array().unwrap() {
                let phrase = witness["expected_decoded_phrase"].as_str().unwrap();
                let blocks = real::blocks(&current);
                let owners = blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Paragraph {
                            children, source, ..
                        } if mant_ir::inline_plain_text(children).contains(phrase) => Some(source),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    owners.len(),
                    1,
                    "{file} round{round}: complete following phrase {phrase}"
                );
                if round == 0 {
                    witnesses += 1;
                    let source = owners[0].as_ref().expect("original following owner source");
                    assert!(
                        u64::from(source.line) > witness["te_line"].as_u64().unwrap(),
                        "{file}: following source must be outside the table"
                    );
                    assert!(
                        u64::from(source.line) <= witness["source_last_line"].as_u64().unwrap(),
                        "{file}: following owner belongs to the independent source window (which may include a section heading)"
                    );
                }
            }
            if round < 2 {
                let options = mant_codec::encode::MarkdownOptions::default();
                let markdown = mant_codec::encode::render_markdown_with_options(&current, options);
                assert_eq!(
                    mant_codec::encode::render_addressable_markdown_with_options(&current, options)
                        .text(),
                    markdown
                );
                current = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
            }
        }
    }
    assert_eq!((tables, witnesses), (104, 12));
}
