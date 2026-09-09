//! Execute the authored example rather than validating only its JSON syntax.
use mant_engine::{build_outline_with_references, query_markdown_text};
use mant_ir::{DocumentAddress, MarkdownOrigin};
use mant_protocol::{EntryProjection, QueryOutline, ReferenceProjection, ReferenceProjectionMode};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag};

fn first_fence(source: &str, language: &str) -> String {
    let mut parser = Parser::new(source);
    while let Some(event) = parser.next() {
        if matches!(event, Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref name))) if name.as_ref() == language)
        {
            let mut text = String::new();
            for event in parser.by_ref() {
                match event {
                    Event::Text(value) => text.push_str(&value),
                    Event::End(_) => return text,
                    _ => {}
                }
            }
        }
    }
    panic!("missing {language} fence")
}

#[test]
fn reference_inventory_manual_example_matches_real_producer() {
    let manual = include_str!("../../../docs/manuals/mant-protocol.md");
    let source = first_fence(
        manual.split_once("Reference inventory input:").unwrap().1,
        "markdown",
    );
    let mut query = query_markdown_text(&source, Some("linked.md".into())).unwrap();
    query.address = Some(DocumentAddress::Markdown {
        path: "linked".into(),
        origin: MarkdownOrigin::Documents,
    });
    let actual = build_outline_with_references(
        &query,
        EntryProjection::All,
        None,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            ..Default::default()
        },
    )
    .unwrap();
    println!(
        "GENERATED_REFERENCE_EXAMPLE\n{}\nEND_REFERENCE_EXAMPLE",
        serde_json::to_string_pretty(&actual).unwrap()
    );
    let expected: QueryOutline = serde_json::from_str(&first_fence(
        manual
            .split_once("Complete reference inventory example (registered as `documents/linked`):")
            .unwrap()
            .1,
        "json",
    ))
    .unwrap();
    assert_eq!(actual, expected);
    let document = query.document.as_ref().unwrap();
    assert_eq!(actual.references.records.len(), 2);
    for record in &actual.references.records {
        assert!(record.origin.resolve_link(document).is_some());
    }
    assert_ne!(
        actual.references.records[0].origin,
        actual.references.records[1].origin
    );
    assert_eq!(
        actual.references.targets,
        mant_protocol::ReferenceCount::Exact { value: 1 }
    );
}
