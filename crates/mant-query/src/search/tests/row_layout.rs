//! Layout metadata does not replace author content or actual artifact coordinates.
use super::{query, request};
use crate::{search_query, select_explanation};
use mant_ir::{Block, EntryOwner, ResolvedContent, inline_plain_text};
use mant_protocol::{QueryBundle, QueryExplanation, QuerySearch, SearchScope};

fn content() -> ResolvedContent {
    let block: Block = serde_json::from_str(r#"{
        "type":"list","kind":{"kind":"plain"},"compact":true,"items":[{
            "blocks":[{
                "type":"paragraph","children":[{"type":"text","value":" --alpha 中 e\u0301  \n NEXT "}],
                "inlineLayout":{"rowHints":[{"row":0,"indentColumns":4},{"row":1,"indentColumns":-2}]}
            },{
                "type":"preformatted","children":[{"type":"text","value":"  中e\u0301  \n END \n"}],
                "inlineLayout":{"rowHints":[{"row":0,"indentColumns":3},{"row":1,"indentColumns":-2},{"row":2,"indentColumns":9}]}
            }],
            "entry":{"id":"option-alpha","kind":{"kind":"parameter","parameterKind":"option"},
                "case":"sensitive","names":["--alpha"],
                "forms":[{"parts":[{"root":{"kind":"block","index":0},"path":[]}]}],
                "nameBindings":[{"name":0,"evidence":"declared","occurrences":[{"parts":[
                    {"root":{"kind":"block","index":0},"path":[0],"bytes":{"start":1,"end":8}}
                ]}]}]
            }
        }]
    }"#).unwrap();
    let mut content = query();
    content.document.as_mut().unwrap().sections[0].blocks = vec![block];
    content
}

#[test]
fn ordinary_and_fenced_layout_policies_keep_owner_names_and_exact_search_bytes() {
    let original = content();
    let decoded: ResolvedContent = serde_json::from_str::<QueryBundle>(
        &serde_json::to_string(&QueryBundle::from(&original)).unwrap(),
    )
    .unwrap()
    .into();
    let block = &decoded.document.as_ref().unwrap().sections[0].blocks[0];
    let Block::List { items, .. } = block else {
        unreachable!()
    };
    let owner = EntryOwner::List(&items[0]);
    assert_eq!(owner.validated_names().unwrap(), ["--alpha"]);
    let artifact = mant_codec::encode::render_addressable_markdown(&decoded);
    assert!(!artifact.text().contains("&#160;"));
    assert!(
        artifact
            .text()
            .contains("&#32;--alpha 中\u{a0}e\u{301}&#32;&#32;  \n  &#32;NEXT&#32;"),
        "ordinary artifact:\n{}",
        artifact.text()
    );
    assert!(
        artifact
            .text()
            .contains("  ```\n      \u{a0}中e\u{301}  \n   END \n\n  ```"),
        "literal artifact:\n{}",
        artifact.text()
    );
    let mapped = artifact
        .nodes()
        .iter()
        .find(|node| {
            matches!(
                node.node(),
                mant_codec::encode::MarkdownNode::DocumentEntry { .. }
            )
        })
        .unwrap();
    assert!(artifact.text()[mapped.range()].contains("--alpha"));
    assert_search_bytes(&decoded, &artifact, mapped.range());
    let explanation = select_explanation(&decoded, "--alpha").unwrap();
    let restored: QueryExplanation =
        serde_json::from_str(&serde_json::to_string(&explanation).unwrap()).unwrap();
    let evidence = restored
        .evidence
        .iter()
        .find(|e| e.outline.node.id() == "option-alpha")
        .unwrap();
    let entry = evidence.entry.as_ref().unwrap();
    assert_eq!(
        inline_plain_text(&entry.forms[0]),
        " --alpha 中\u{a0}e\u{301}  \n NEXT "
    );
    let form_range = &entry.name_bindings[0].occurrences[0].forms[0];
    assert_eq!((form_range.start_char, form_range.end_char), (1, 8));
    assert_readback_rows(artifact.text(), "<a id=\"option-alpha\"></a>");
    assert_readback_rows(&mant_codec::encode::render_markdown(&decoded), "");
    assert_eq!(decoded.document, original.document);
}

fn assert_readback_rows(markdown: &str, navigation_source: &str) {
    let readback = crate::query_fixture::markdown(markdown, None).unwrap();
    let Block::List { items, .. } = &readback.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("same list owner: {markdown}")
    };
    assert_eq!(items.len(), 1);
    let blocks = &items[0].blocks;
    assert_eq!(blocks.len(), 2);
    let Block::Paragraph { children, .. } = &blocks[0] else {
        panic!("ordinary body")
    };
    assert_eq!(
        inline_plain_text(children),
        format!("{navigation_source} --alpha 中\u{a0}e\u{301}  \n NEXT ")
    );
    let Block::Preformatted { children, .. } = &blocks[1] else {
        panic!("literal body")
    };
    assert_eq!(
        inline_plain_text(children),
        "    \u{a0}中e\u{301}  \n END \n"
    );
}

fn assert_search_bytes(
    content: &ResolvedContent,
    artifact: &mant_codec::encode::MarkdownArtifact<'_>,
    owner_range: std::ops::Range<usize>,
) {
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        for pattern in ["--alpha", "中\u{a0}e\u{301}", "中e\u{301}"] {
            let result = search_query(
                content,
                &mant_protocol::SearchQuery {
                    scope,
                    ..request(pattern)
                },
            )
            .unwrap();
            let restored: QuerySearch =
                serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
            assert_eq!(restored.total, 1);
            let hit = &restored.matches[0];
            assert_eq!(hit.outline.node.id(), "option-alpha");
            let range = &hit.occurrences[0].markdown;
            let start = usize::try_from(range.start_byte).unwrap();
            let end = usize::try_from(range.end_byte).unwrap();
            assert_eq!(&artifact.text()[start..end], pattern);
            assert_eq!(
                artifact.text()[..start]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count()
                    + 1,
                usize::try_from(range.start_line).unwrap()
            );
            assert_eq!(
                artifact.text()[..start]
                    .rsplit('\n')
                    .next()
                    .unwrap()
                    .chars()
                    .count()
                    + 1,
                usize::try_from(range.start_column).unwrap()
            );
            assert!(owner_range.start <= start && end <= owner_range.end);
        }
    }
}
