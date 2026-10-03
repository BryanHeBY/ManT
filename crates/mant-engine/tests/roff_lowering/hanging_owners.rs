//! Executed HP/headless-IP ownership preserves the original reading blocks.

use mant_codec::encode::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options,
    render_markdown_with_options,
};
use mant_ir::{
    Block, Document, EntryInlineRoot, EntryOwner, ListItem, ListKind, ResolvedContent,
    visit::{self, Visit},
};
use mant_protocol::{EvidenceBasis, ExplanationOptions, ExplanationQuery, SearchQuery};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    source: String,
    owner_names: Vec<Vec<String>>,
    native_rows: Vec<String>,
}

fn cases() -> Vec<Case> {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("hanging_owners/cases.json")).unwrap();
    assert_eq!(matrix["header"]["count"], 24);
    serde_json::from_value(matrix["cases"].clone()).unwrap()
}

fn owners(document: &Document) -> Vec<&ListItem> {
    struct Collect<'a>(Vec<&'a ListItem>);
    impl<'a> Visit<'a> for Collect<'a> {
        fn visit_block(&mut self, block: &'a Block) {
            if let Block::List {
                kind: ListKind::Plain,
                items,
                ..
            } = block
            {
                self.0
                    .extend(items.iter().filter(|item| item.entry.is_some()));
            }
            visit::walk_block(self, block);
        }
    }
    let mut collect = Collect(Vec::new());
    collect.visit_document(document);
    collect.0
}

fn roundtrip(case: &Case) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let encoded = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
    assert!(!encoded.contains("mant-native-definition-owner"));
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&encoded).unwrap();
    let decoded: ResolvedContent = decoded.into();
    assert_eq!(decoded, original, "{}: actual JSON", case.id);
    assert_eq!(
        mant_ir::validate_document(decoded.document.as_ref().unwrap()),
        []
    );
    decoded
}

fn assert_bindings(item: &ListItem, names: &[String]) {
    let facts = item.entry.as_ref().unwrap();
    assert_eq!(facts.names, names);
    assert_eq!(
        facts.kind,
        mant_ir::EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        }
    );
    assert_eq!(facts.forms.len(), 1);
    assert_eq!(
        facts.forms[0].parts[0].root,
        EntryInlineRoot::Block { index: 0 }
    );
    assert_eq!(facts.name_bindings.len(), names.len());
    let owner = EntryOwner::List(item);
    for binding in &facts.name_bindings {
        assert_ne!(binding.occurrences, []);
        for occurrence in &binding.occurrences {
            assert!(
                occurrence
                    .parts
                    .iter()
                    .all(|part| part.root == EntryInlineRoot::Block { index: 0 })
            );
            assert_eq!(
                mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
                names[binding.name]
            );
        }
    }
    assert_eq!(item.layout.spacing_before_lines, Some(0));
    assert!(matches!(
        &item.blocks[0],
        Block::Paragraph { .. } | Block::Preformatted { .. }
    ));
    let Block::DefinitionList { items, .. } = &item.blocks[1] else {
        panic!("original empty-head IP carrier");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].terms, [] as [Vec<mant_ir::Inline>; 0]);
    assert_ne!(items[0].description, []);
}

fn unwrap_owners(blocks: &mut Vec<Block>) {
    let mut original = Vec::new();
    for mut block in std::mem::take(blocks) {
        match &mut block {
            Block::List {
                kind: ListKind::Plain,
                items,
                ..
            } if items.len() == 1 && items[0].entry.is_some() => {
                original.append(&mut items[0].blocks);
                continue;
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    unwrap_owners(&mut item.description);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    unwrap_owners(&mut item.blocks);
                }
            }
            _ => {}
        }
        original.push(block);
    }
    *blocks = original;
}

fn assert_geometry(case: &Case, content: &ResolvedContent) {
    let text = mant_render::render_query_man(content);
    let mut unwrapped = content.clone();
    let document = unwrapped.document.as_mut().unwrap();
    unwrap_owners(&mut document.blocks);
    for section in &mut document.sections {
        unwrap_owners(&mut section.blocks);
    }
    assert_eq!(
        text,
        mant_render::render_query_man(&unwrapped),
        "{}: ownership cannot change reading",
        case.id
    );
    if case.owner_names.is_empty() {
        return;
    }
    let body = text
        .split_once("OPTIONS\n")
        .unwrap()
        .1
        .split_once("\nNEXT\n")
        .unwrap()
        .0;
    let next_spacing = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "NEXT")
        .unwrap()
        .spacing_before_lines;
    // Only NEXT's independent section gap is outside the HP/IP fixture.
    // In particular PD 0 supplies no such gap, and internal completed rows
    // remain strict rather than being trimmed from the body.
    let boundary_gap = "\n".repeat(usize::from(next_spacing));
    let body = body.strip_suffix(&boundary_gap).unwrap();
    // The section's preceding boundary is outside this HP/IP ownership unit.
    // Begin at the exact first authored HEAD row, retaining every subsequent
    // space, NBSP and empty physical row from pristine (common page margin 5).
    let rows = body.split('\n').collect::<Vec<_>>();
    let first = rows
        .iter()
        .position(|row| *row == case.native_rows[0])
        .unwrap();
    assert_eq!(&rows[first..], case.native_rows, "{}: native rows", case.id);
}

#[test]
fn executed_hanging_pairs_keep_block_forms_and_native_geometry() {
    // Every exact source ran pristine ASCII/UTF-8/HTML/tree/lint/width24
    // before these assertions. man_term.c::pre_HP/post_HP retains hanging
    // paragraph geometry; pre_IP permits an empty HEAD with its own BODY
    // origin. Neither macro supplies a native semantic TP declaration.
    for case in cases() {
        let content = roundtrip(&case);
        let items = owners(content.document.as_ref().unwrap());
        assert_eq!(
            items.len(),
            case.owner_names.len(),
            "{}: recovered owners",
            case.id
        );
        for (item, names) in items.iter().zip(&case.owner_names) {
            assert_bindings(item, names);
        }
        assert_geometry(&case, &content);
    }
}

fn assert_queries(content: &ResolvedContent, item: &ListItem) {
    let facts = item.entry.as_ref().unwrap();
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    let mapped = artifact.nodes().iter().filter(|mapped| matches!(mapped.node(),
        MarkdownNode::DocumentEntry { owner: EntryOwner::List(value), .. } if std::ptr::eq(*value, item)))
        .collect::<Vec<_>>();
    assert_eq!(mapped.len(), 1);
    let owned_bytes = &artifact.text()[mapped[0].range()];
    for name in &facts.names {
        assert!(owned_bytes.contains(name), "{name}: owner artifact");
        let response = mant_query::explain_query(
            content,
            &ExplanationQuery {
                entry: name.clone(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        let expected_owners = owners(content.document.as_ref().unwrap())
            .into_iter()
            .filter_map(|owner| owner.entry.as_ref())
            .filter(|entry| entry.names.contains(name))
            .map(|entry| entry.id.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        let direct_names = response
            .evidence
            .iter()
            .filter(|evidence| {
                evidence
                    .bases
                    .iter()
                    .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
            })
            .map(|evidence| evidence.outline.node.id().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(direct_names, expected_owners, "{name}: exact name owners");
        let search = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: name.clone(),
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 100,
                offset: 0,
            },
        )
        .unwrap();
        assert!(
            search
                .matches
                .iter()
                .any(|hit| hit.outline.node.id() == facts.id.as_str())
        );
        for occurrence in search.matches.iter().flat_map(|hit| &hit.occurrences) {
            let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                ..usize::try_from(occurrence.markdown.end_byte).unwrap();
            assert_eq!(&artifact.text()[range], name);
            assert_eq!(&occurrence.matched_text, name);
        }
    }
}

pub(super) fn descendant_has_source(
    node: &libmandoc_rs::Node,
    source: mant_ir::SourceSpan,
) -> bool {
    (node.line, node.column) == (source.line, source.column)
        || node
            .children
            .iter()
            .any(|child| descendant_has_source(child, source))
}

pub(super) fn ast_pairs<'a>(
    node: &'a libmandoc_rs::Node,
    pairs: &mut Vec<(u32, u32, &'a libmandoc_rs::Node)>,
) {
    for siblings in node.children.windows(2) {
        let [head, body] = siblings else {
            unreachable!()
        };
        if head.kind == libmandoc_rs::NodeKind::Block
            && head.macro_name.as_deref() == Some("HP")
            && body.kind == libmandoc_rs::NodeKind::Block
            && body.macro_name.as_deref() == Some("IP")
            && body
                .children
                .iter()
                .find(|part| part.kind == libmandoc_rs::NodeKind::Head)
                .is_some_and(|part| part.children.is_empty())
        {
            pairs.push((body.line, body.column, head));
        }
    }
    for child in &node.children {
        ast_pairs(child, pairs);
    }
}

#[test]
fn block_backed_owner_sources_belong_to_the_actual_hp_ip_sibling_nodes() {
    for case in cases()
        .into_iter()
        .filter(|case| !case.owner_names.is_empty())
    {
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("owner.1", case.source.as_bytes())
            .unwrap();
        let mut pairs = Vec::new();
        ast_pairs(&native.document.root, &mut pairs);
        let content = roundtrip(&case);
        let items = owners(content.document.as_ref().unwrap());
        assert!(
            pairs.len() >= items.len(),
            "{}: real sibling ownership",
            case.id
        );
        let mut seen = std::collections::HashSet::new();
        for item in items {
            assert!(
                seen.insert(item.entry.as_ref().unwrap().id.clone()),
                "{}: unique expanded owner",
                case.id
            );
            let head_source = mant_ir::geometry::block_source(&item.blocks[0]).unwrap();
            let Block::DefinitionList { items: bodies, .. } = &item.blocks[1] else {
                unreachable!()
            };
            let body_source = bodies[0].source.unwrap();
            assert!(
                pairs.iter().any(|(line, column, head)| {
                    (*line, *column) == (body_source.line, body_source.column)
                        && descendant_has_source(head, head_source)
                }),
                "{}: original HEAD/BODY source",
                case.id
            );
        }
    }
}

#[test]
fn hanging_owner_queries_and_portable_export_borrow_the_retained_content() {
    for case in cases()
        .into_iter()
        .filter(|case| !case.owner_names.is_empty())
    {
        let content = roundtrip(&case);
        for item in owners(content.document.as_ref().unwrap()) {
            assert_queries(&content, item);
        }
        for preserve_semantics in [false, true] {
            let markdown = render_markdown_with_options(
                &content,
                MarkdownOptions {
                    preserve_anchors: false,
                    preserve_semantics,
                },
            );
            let reader = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let visible = mant_render::render_query_text(&reader);
            for word in case.owner_names.iter().flatten() {
                assert!(visible.contains(word), "{}: {markdown}", case.id);
            }
            for word in ["BodyWord", "MACRO_BODY", "SECOND_BODY", "MORE_BODY"] {
                if case.source.contains(word) {
                    assert!(visible.contains(word), "{}: {markdown}", case.id);
                }
            }
            assert!(!visible.contains("mant-native-definition-owner"));
        }
    }
}
