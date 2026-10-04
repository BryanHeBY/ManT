//! Executed HP/headless-IP ownership preserves the original reading blocks.

use mant_codec::encode::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options,
    render_markdown_with_options,
};
use mant_ir::{
    Block, Document, EntryInlineRoot, EntryOwner, Inline, ListItem, ListKind, ResolvedContent,
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

fn executed_owners<'a>(document: &'a Document, root: &libmandoc_rs::Node) -> Vec<&'a ListItem> {
    let mut pairs = Vec::new();
    ast_pairs(root, &mut pairs);
    owners(document)
        .into_iter()
        .filter(|item| matches_pair(item, &pairs))
        .collect()
}

fn matches_pair(item: &ListItem, pairs: &[(u32, u32, &libmandoc_rs::Node)]) -> bool {
    let [head, Block::DefinitionList { items, .. }] = item.blocks.as_slice() else {
        return false;
    };
    let [body] = items.as_slice() else {
        return false;
    };
    if !body.terms.is_empty()
        || !matches!(head, Block::Paragraph { .. } | Block::Preformatted { .. })
    {
        return false;
    }
    let (Some(head_source), Some(body_source)) =
        (mant_ir::geometry::block_source(head), body.source)
    else {
        return false;
    };
    pairs.iter().any(|(line, column, head)| {
        (*line, *column) == (body_source.line, body_source.column)
            && descendant_has_source(head, head_source)
    })
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
    assert_eq!(items[0].terms, [] as [mant_ir::DefinitionTerm; 0]);
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
    assert_native_rows(case, content);
}

fn assert_native_rows(case: &Case, content: &ResolvedContent) {
    let text = mant_render::render_query_man(content);
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
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("owner.1", case.source.as_bytes())
            .unwrap();
        let content = roundtrip(&case);
        // A generic hanging definition may retain a Paragraph in the same
        // public Plain List shape. Only real HP/headless-IP siblings prove
        // this executed recovery; an intervening RS supplies another parent.
        let items = executed_owners(content.document.as_ref().unwrap(), &native.document.root);
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

#[test]
fn deeper_rs_description_keeps_a_generic_h_owner_without_claiming_an_executed_pair() {
    // The exact container-between input was rerun in pristine ASCII, UTF-8,
    // HTML and tree before this assertion. man_term.c::post_HP closes the
    // original paragraph; pre/post_RS adds four cells around the nested IP.
    // The tree has HP -> RS siblings, with IP inside RS BODY, so native pair
    // recovery has no handoff. Existing generic deeper-layout admission still
    // owns this declaration; preserving H keeps its original Paragraph root.
    let case = cases()
        .into_iter()
        .find(|case| case.id == "container-between")
        .unwrap();
    let native = libmandoc_rs::Parser::default()
        .parse_bytes("owner.1", case.source.as_bytes())
        .unwrap();
    let mut pairs = Vec::new();
    ast_pairs(&native.document.root, &mut pairs);
    assert_eq!(pairs.len(), 0);
    let content = roundtrip(&case);
    let document = content.document.as_ref().unwrap();
    assert_eq!(executed_owners(document, &native.document.root).len(), 0);
    let generic = owners(document);
    assert_eq!(generic.len(), 1);
    let item = generic[0];
    assert_generic_rs_owner(item);
    assert_geometry(&case, &content);
    assert_native_rows(&case, &content);
    assert_queries(&content, item);
}

fn assert_generic_rs_owner(item: &ListItem) {
    let names = ["-n", "--quiet", "--silent"].map(str::to_owned);
    assert_bindings(item, &names);
    assert_eq!(item.entry.as_ref().unwrap().id.as_str(), "option-n");
    let Block::Paragraph {
        children,
        layout,
        inline_layout,
        source,
    } = &item.blocks[0]
    else {
        panic!("the generic H owner retains the original paragraph")
    };
    assert_eq!(layout.indent_columns, 0);
    assert_eq!(layout.continuation_indent_columns, 7);
    assert_eq!(inline_layout.row_hints, []);
    assert_eq!(
        source.map(|source| (source.line, source.column)),
        Some((4, 1))
    );
    assert_eq!(item.source, *source);
    let strong = |value: &str| Inline::Strong {
        children: vec![Inline::Text {
            value: value.into(),
        }],
    };
    assert_eq!(
        children.as_slice(),
        &[
            strong("-n"),
            Inline::Text { value: ", ".into() },
            strong("--quiet"),
            Inline::Text { value: ", ".into() },
            strong("--silent"),
        ]
    );
    let Block::DefinitionList {
        items,
        layout,
        source,
        ..
    } = &item.blocks[1]
    else {
        unreachable!()
    };
    assert_eq!(layout.indent_columns, 4);
    assert_eq!(items[0].layout.body_indent_columns, 7);
    assert_eq!(
        source.map(|source| (source.line, source.column)),
        Some((6, 2))
    );
    assert_eq!(items[0].source, *source);
    let [
        Block::Paragraph {
            layout,
            source,
            children,
            ..
        },
    ] = items[0].description.as_slice()
    else {
        panic!("the RS/IP body keeps its original paragraph")
    };
    assert_eq!(layout.indent_columns, 0);
    assert_eq!(
        source.map(|source| (source.line, source.column)),
        Some((7, 1))
    );
    assert_eq!(
        mant_ir::inline_plain_text(children),
        "BodyWord is in another container."
    );
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
            && head.macro_token.as_deref() == Some("HP")
            && body.kind == libmandoc_rs::NodeKind::Block
            && body.macro_token.as_deref() == Some("IP")
            && body
                .children
                .iter()
                .find(|part| part.kind == libmandoc_rs::NodeKind::Head)
                .is_some_and(|part| part.children.is_empty())
            && exit_epoch(head) == body.flow_epoch
        {
            pairs.push((body.line, body.column, head));
        }
    }
    for child in &node.children {
        ast_pairs(child, pairs);
    }
}

fn exit_epoch(mut node: &libmandoc_rs::Node) -> usize {
    // Empty PP/P/LP validation can remove a node without erasing its executed
    // boundary. The parse's native epoch keeps that non-pair distinguishable.
    while let Some(last) = node.children.last() {
        node = last;
    }
    node.flow_epoch
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
