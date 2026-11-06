//! Physical line preparation changes reading spelling, never original origins.
use super::parse_markdown;
use mant_ir::{
    Document, Inline,
    visit::{Visit, walk_inline},
};
use pulldown_cmark::Event;

fn without_sources(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("source");
            for value in map.values_mut() {
                without_sources(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                without_sources(value);
            }
        }
        _ => {}
    }
}
fn verify_sources(value: &serde_json::Value, source: &str) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(range) = map.get("byteRange") {
                let start = usize::try_from(range["start"].as_u64().unwrap()).unwrap();
                let end = usize::try_from(range["end"].as_u64().unwrap()).unwrap();
                assert!(source.get(start..end).is_some(), "{source:?}: {map:?}");
                let prefix = &source[..start];
                let prefix = prefix.replace("\r\n", "\n").replace('\r', "\n");
                assert_eq!(
                    map["line"],
                    1 + prefix.bytes().filter(|b| *b == b'\n').count()
                );
                let column = source[..start]
                    .rfind(['\r', '\n'])
                    .map_or(start + 1, |i| start - i);
                assert_eq!(map["column"], column);
            }
            for value in map.values() {
                verify_sources(value, source);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                verify_sources(value, source);
            }
        }
        _ => {}
    }
}

#[test]
fn source_line_endings_have_equal_semantics_and_original_utf8_spans() {
    let cases = [
        "# 中\n\n`A\nB`\n",
        "# 中\n\n<div>\nAlpha\n</div>\n",
        "# 中\n\n- **A**\n  B\n\n- `C\n  D`\n",
        "# 中\n\n<br />\n[a][ref]\n\n[ref]: https://a.test\n  \"before&#12;after\"\n",
        "# 中\n\n| A&#13;B |\n| --- |\n| C |\n",
        "# 中\n\n```sh\nA\nB\n```\n",
        "<!-- mant:entries term -->\n- `Alpha`\n\n  Body\n",
    ];
    for source in cases {
        let baseline = parse_markdown(source, None).unwrap().document;
        let mut expected = serde_json::to_value(baseline).unwrap();
        without_sources(&mut expected);
        for ending in ["\n", "\r\n", "\r"] {
            let source = source.replace('\n', ending);
            let document = parse_markdown(&source, None).unwrap().document;
            let mut actual = serde_json::to_value(&document).unwrap();
            verify_sources(&actual, &source);
            without_sources(&mut actual);
            assert_eq!(actual, expected, "{source:?}");
            assert_eq!(
                serde_json::from_str::<Document>(&serde_json::to_string(&document).unwrap())
                    .unwrap(),
                document
            );
        }
    }
}

#[test]
fn code_and_unsupported_reading_text_follow_one_physical_line_rule() {
    #[derive(Default)]
    struct Codes(Vec<String>);
    impl<'a> Visit<'a> for Codes {
        fn visit_inline(&mut self, node: &'a Inline) {
            if let Inline::Code { value } = node {
                self.0.push(value.clone());
            }
            walk_inline(self, node);
        }
    }
    for ending in ["\n", "\r\n", "\r"] {
        let source = format!("`A{ending}B`{ending}{ending}<div>{ending}中{ending}</div>{ending}");
        let document = parse_markdown(&source, None).unwrap().document;
        let mut codes = Codes::default();
        codes.visit_document(&document);
        assert_eq!(codes.0, ["A B"]);
        assert!(
            matches!(&document.blocks[1], mant_ir::Block::Unsupported {text, ..} if text == "<div>\n中\n</div>\n")
        );
        for (event, range) in crate::markdown_mapping::markdown_source_events(&source) {
            if let Event::Code(value) = event {
                assert_eq!(value.as_ref(), "A B");
                assert_eq!(&source[range.clone()], format!("`A{ending}B`"));
                let mapped = crate::markdown_mapping::map_inline_characters(
                    &source,
                    &value,
                    range,
                    crate::markdown_mapping::InlineMappingKind::Code,
                );
                assert_eq!(&source[mapped[1].source.clone()], ending);
            }
        }
    }
    let document = parse_markdown("A&#13;B", None).unwrap().document;
    assert!(
        matches!(&document.blocks[0], mant_ir::Block::Paragraph {children, ..} if mant_ir::inline_plain_text(children) == "A\rB")
    );
}

#[test]
fn mixed_endings_and_embedded_preface_keep_exact_original_owner_positions() {
    let source = "<!-- mant:tldr:start -->\n# tool\n\n> Quick reference.\n\n- Run:\n\n`tool`\n<!-- mant:tldr:end -->\n\n# 中\n\nBody\n";
    for ending in ["\n", "\r\n", "\r"] {
        let source = source.replace('\n', ending);
        let parsed = parse_markdown(&source, None).unwrap();
        assert_eq!(parsed.tldr.unwrap().title, "tool");
        assert_eq!(parsed.document.heading.as_ref().unwrap().plain_text(), "中");
        verify_sources(&serde_json::to_value(parsed.document).unwrap(), &source);
    }
    let source = "# 中\r\n\rA\n\r\n`B\rC`\r\n\n<div>\rD\r\n</div>";
    let document = parse_markdown(source, None).unwrap().document;
    verify_sources(&serde_json::to_value(document).unwrap(), source);
    let endings = super::super::source::LineEndings::new("中\r\nA\r\n");
    assert_eq!(endings.text, "中\nA\n");
    assert_eq!(endings.range(3..4), 3..5);
    assert_eq!(endings.range(4..5), 5..6);
    assert_eq!(endings.range(6..6), 8..8);
}
