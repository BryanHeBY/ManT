//! Pure IR consumers preserve the retained paragraph owner and its geometry.
use super::query;
use crate::{ResolvedContent, search_query, select_excerpt, select_explanation};
use mant_ir::{Block, Document, EntryInlineRoot, EntryOwner, inline_plain_text};
use mant_protocol::{
    ContentSelector, ExcerptSelection, ExplanationBlockStep, ExplanationContent,
    ExplanationContentRange, QueryBundle, QueryExcerpt, QueryExplanation, QuerySearch, SearchCase,
    SearchQuery, SearchScope, SearchSyntax,
};

fn hanging_owner() -> ResolvedContent {
    let block: Block = serde_json::from_str(r#"{
        "type":"list", "kind":{"kind":"plain"}, "compact":true,
        "items":[{
            "blocks":[{
                "type":"paragraph",
                "children":[
                    {"type":"strong","children":[{"type":"text","value":"-"}]},
                    {"type":"link","target":{"kind":"external","uri":"https://example.org/alpha"},
                     "children":[{"type":"emphasis","children":[{"type":"text","value":"alpha"}]}]},
                    {"type":"text","value":" ARG"}, {"type":"line-break"},
                    {"type":"text","value":"VALUE"}, {"type":"line-break"},
                    {"type":"text","value":"MORE"}
                ],
                "inlineLayout":{"rowHints":[{"row":0,"indentColumns":-2},{"row":2,"indentColumns":3}]},
                "layout":{"indentColumns":4,"continuationIndentColumns":6,"spacingBeforeLines":2}
            },{
                "type":"preformatted", "children":[{"type":"text","value":"BodyWord\nSecondRow"}],
                "layout":{"indentColumns":9,"continuationIndentColumns":3}
            }],
            "entry":{
                "id":"option-alpha", "kind":{"kind":"parameter","parameterKind":"option"},
                "case":"sensitive", "names":["-alpha"],
                "forms":[{"parts":[{"root":{"kind":"block","index":0},"path":[]}]}],
                "nameBindings":[{"name":0,"evidence":"declared","occurrences":[{"parts":[
                    {"root":{"kind":"block","index":0},"path":[0,0]},
                    {"root":{"kind":"block","index":0},"path":[1,0,0]}
                ]}]}]
            }
        }]
    }"#).unwrap();
    let mut content = query();
    content.document.as_mut().unwrap().sections[0].blocks = vec![block];
    content
}

fn original_owner(content: &ResolvedContent) -> &Block {
    &content.document.as_ref().unwrap().sections[0].blocks[0]
}

fn request(pattern: &str) -> SearchQuery {
    SearchQuery {
        pattern: pattern.into(),
        syntax: SearchSyntax::Literal,
        case: SearchCase::Sensitive,
        scope: SearchScope::Visible,
        word: false,
        context_lines: 1,
        limit: 100,
        offset: 0,
    }
}

fn assert_complete_owner(block: &Block) {
    let Block::List { items, .. } = block else {
        panic!("original list owner")
    };
    let owner = EntryOwner::List(&items[0]);
    assert_eq!(owner.validated_names().unwrap(), ["-alpha"]);
    let forms = owner.forms().unwrap();
    assert_eq!(
        inline_plain_text(forms.iter().next().unwrap()),
        "-alpha ARG\nVALUE\nMORE"
    );
    let facts = owner.facts().unwrap();
    assert_eq!(
        facts.forms[0].parts[0].root,
        EntryInlineRoot::Block { index: 0 }
    );
    assert_eq!(facts.forms[0].parts[0].path, Vec::<usize>::new());
    assert!(
        facts.name_bindings[0].occurrences[0]
            .parts
            .iter()
            .all(|part| { part.root == EntryInlineRoot::Block { index: 0 } })
    );
    let Block::Paragraph {
        children,
        inline_layout,
        layout,
        ..
    } = &items[0].blocks[0]
    else {
        panic!("original paragraph")
    };
    assert_eq!(inline_plain_text(children), "-alpha ARG\nVALUE\nMORE");
    assert_eq!(
        (layout.indent_columns, layout.continuation_indent_columns),
        (4, 6)
    );
    assert_eq!(
        (inline_layout.row_indent(0), inline_layout.row_indent(2)),
        (-2, 3)
    );
    assert_eq!(
        mant_ir::geometry::block_layout(&items[0].blocks[1])
            .unwrap()
            .indent_columns,
        9
    );
    assert_eq!(
        mant_ir::geometry::block_layout(&items[0].blocks[1])
            .unwrap()
            .continuation_indent_columns,
        3
    );
}

fn assert_explanation(content: &ResolvedContent, expected: &Block) {
    let explanation = select_explanation(content, "-alpha").unwrap();
    let restored: QueryExplanation =
        serde_json::from_str(&serde_json::to_string(&explanation).unwrap()).unwrap();
    let evidence = restored
        .evidence
        .iter()
        .find(|e| e.outline.node.id() == "option-alpha")
        .unwrap();
    let Some(ExplanationContent::Entry { block }) = &evidence.content else {
        panic!("complete owner")
    };
    assert_eq!(block, expected);
    assert_complete_owner(block);
    let entry = evidence.entry.as_ref().unwrap();
    assert_eq!(
        inline_plain_text(&entry.forms[0]),
        "-alpha ARG\nVALUE\nMORE"
    );
    let occurrence = &entry.name_bindings[0].occurrences[0];
    let intervals = occurrence
        .forms
        .iter()
        .map(|range| (range.form_index, range.start_char, range.end_char))
        .collect::<Vec<_>>();
    assert_eq!(intervals, [(0, 0, 1), (0, 1, 6)]);
    let expected_ranges =
        [(0, 1), (1, 6)].map(
            |(start_char, end_char)| ExplanationContentRange::BlockText {
                path: vec![
                    ExplanationBlockStep::ListItem { index: 0 },
                    ExplanationBlockStep::Block { index: 0 },
                ],
                start_char,
                end_char,
            },
        );
    assert_eq!(occurrence.content, expected_ranges);
}

#[test]
fn retained_hanging_paragraph_survives_document_and_query_json_consumption() {
    let original = hanging_owner();
    let wire = serde_json::to_string(original.document.as_ref().unwrap()).unwrap();
    let document: Document = serde_json::from_str(&wire).unwrap();
    assert_eq!(&document, original.document.as_ref().unwrap());
    let mut decoded_document = original.clone();
    decoded_document.document = Some(document);
    let decoded_bundle: ResolvedContent = serde_json::from_str::<QueryBundle>(
        &serde_json::to_string(&QueryBundle::from(&original)).unwrap(),
    )
    .unwrap()
    .into();
    for content in [&original, &decoded_document, &decoded_bundle] {
        let expected = original_owner(&original);
        assert_eq!(original_owner(content), expected);
        assert_complete_owner(original_owner(content));
        let excerpt = select_excerpt(content, &[ContentSelector::id("option-alpha")]).unwrap();
        let restored: QueryExcerpt =
            serde_json::from_str(&serde_json::to_string(&excerpt).unwrap()).unwrap();
        let ExcerptSelection::DocumentEntry { entry, .. } = &restored.selections[0] else {
            panic!("entry excerpt")
        };
        assert_eq!(entry, expected);
        assert_explanation(content, expected);
        for pattern in ["-alpha", "VALUE", "MORE", "BodyWord"] {
            let search = search_query(content, &request(pattern)).unwrap();
            let restored: QuerySearch =
                serde_json::from_str(&serde_json::to_string(&search).unwrap()).unwrap();
            assert_eq!(restored.total, 1);
            assert_eq!(restored.matches[0].outline.node.id(), "option-alpha");
            assert_eq!(restored.matches[0].occurrences[0].matched_text, pattern);
        }
    }
}
