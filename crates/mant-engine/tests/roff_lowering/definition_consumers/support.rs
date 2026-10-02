use mant_codec::encode::{MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options};
use mant_ir::{Block, DefinitionItem, EntryOwner, Inline, ResolvedContent};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case {
    pub id: String,
    pub source: String,
    pub category: String,
    pub head: String,
    pub head_carrier: String,
    pub body_carrier: String,
    pub native_rows: Vec<String>,
}

pub(super) fn cases() -> Vec<Case> {
    let value: serde_json::Value = serde_json::from_str(include_str!("cases.json")).unwrap();
    let cases: Vec<Case> = serde_json::from_value(value["cases"].clone()).unwrap();
    assert_eq!(cases.len(), 199);
    cases
}

fn description(content: &ResolvedContent) -> &[Block] {
    &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
        .blocks
}

pub(super) fn definition(content: &ResolvedContent) -> &DefinitionItem {
    description(content)
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(&items[0]),
            _ => None,
        })
        .unwrap()
}

pub(super) fn imported_item_blocks(content: &ResolvedContent) -> &[Block] {
    let Block::List { items, .. } = &description(content)[0] else {
        panic!("one public ordinary-list item: {:?}", description(content));
    };
    assert_eq!(items.len(), 1);
    &items[0].blocks
}

pub(super) fn imported_rows(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => mant_ir::inline_plain_text(children),
            other => panic!("unexpected reader block {other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
        .split('\n')
        .map(str::to_owned)
        .collect()
}

fn word_row(case: &Case, row: &str) -> String {
    // These operands author no leading spaces. Remove only generated left
    // origins and the generated gap between the two known operands; do not
    // fold interior author text, fixed cells, or distinct empty physical rows.
    let row = row
        .trim_start_matches(' ')
        .trim_end_matches([' ', '\u{a0}']);
    if let Some(gap) = row
        .strip_prefix(&case.head)
        .and_then(|rest| rest.strip_suffix("BODY"))
        && !gap.is_empty()
        && gap.chars().all(|c| matches!(c, ' ' | '\u{a0}'))
    {
        format!("{} BODY", case.head)
    } else {
        row.to_owned()
    }
}

pub(super) fn expected_rows(case: &Case, simplify_distance: bool) -> Vec<String> {
    assert_eq!(case.native_rows.last().unwrap(), "", "known NEXT spacing");
    let rows = case.native_rows[..case.native_rows.len() - 1]
        .iter()
        .map(|row| word_row(case, row))
        .collect::<Vec<_>>();
    if !simplify_distance {
        return rows;
    }
    let mut output = Vec::new();
    for row in rows {
        if row.is_empty() && output.last().is_some_and(String::is_empty) {
            continue;
        }
        output.push(row);
    }
    output
}

pub(super) fn assert_native_rows(case: &Case, content: &ResolvedContent) {
    let text = mant_render::render_query_man(content);
    let body = text
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .split_once("\n\nNEXT\n")
        .unwrap()
        .0;
    let rows = body
        .split('\n')
        .map(|row| word_row(case, row))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        expected_rows(case, false),
        "{}: text/native rows",
        case.id
    );
}

pub(super) fn assert_ast_owners(case: &Case) {
    use libmandoc_rs::{Node, NodeKind, Parser};
    fn find<'a>(node: &'a Node, predicate: &impl Fn(&Node) -> bool) -> Option<&'a Node> {
        if predicate(node) {
            Some(node)
        } else {
            node.children
                .iter()
                .find_map(|child| find(child, predicate))
        }
    }
    let report = Parser::default()
        .parse_bytes("consumer.1", case.source.as_bytes())
        .unwrap();
    let it = find(&report.document.root, &|node| {
        node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("It")
    })
    .unwrap();
    let head = it
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Head)
        .unwrap();
    let body = it
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Body)
        .unwrap();
    assert!(
        find(head, &|node| node.text.as_deref()
            == Some(case.head.as_str()))
        .is_some(),
        "{}: true HEAD marker",
        case.id
    );
    assert!(find(head, &|node| node.text.as_deref() == Some("BODY")).is_none());
    assert!(
        find(body, &|node| node.text.as_deref() == Some("BODY")).is_some(),
        "{}: true BODY marker",
        case.id
    );
}

pub(super) fn assert_carrier(blocks: &[Block], word: &str, carrier: &str, target: &str) {
    fn collect(
        nodes: &[Inline],
        mask: u8,
        link: Option<&str>,
        out: &mut Vec<(char, u8, Option<String>)>,
    ) {
        for node in nodes {
            match node {
                Inline::Text { value } => {
                    out.extend(value.chars().map(|c| (c, mask, link.map(str::to_owned))));
                }
                Inline::Code { value } => out.extend(
                    value
                        .chars()
                        .map(|c| (c, mask | 4, link.map(str::to_owned))),
                ),
                Inline::Strong { children } => collect(children, mask | 1, link, out),
                Inline::Emphasis { children } => collect(children, mask | 2, link, out),
                Inline::Link {
                    target: mant_ir::LinkTarget::External { uri },
                    children,
                    ..
                } => collect(children, mask, Some(uri), out),
                Inline::LineBreak { .. } => out.push(('\n', 0, None)),
                Inline::Anchor { .. } => {}
                other => panic!("unexpected inline {other:?}"),
            }
        }
    }
    let mut out = Vec::new();
    for block in blocks {
        let Block::Paragraph { children, .. } = block else {
            panic!("prose");
        };
        collect(children, 0, None, &mut out);
    }
    let value = out.iter().map(|(c, _, _)| *c).collect::<String>();
    let start = value[..value.find(word).unwrap()].chars().count();
    let mask = match carrier {
        "No" | "Lk" => 0,
        "Sy" => 1,
        "Em" => 2,
        "Li" => 4,
        _ => panic!("carrier"),
    };
    let uri = (carrier == "Lk").then_some(target);
    for (_, actual, actual_uri) in &out[start..start + word.chars().count()] {
        assert_eq!(*actual, mask, "{carrier}: {value}");
        assert_eq!(actual_uri.as_deref(), uri, "link label scope: {value}");
    }
    let links = out
        .iter()
        .filter_map(|(_, _, uri)| uri.as_deref())
        .collect::<std::collections::HashSet<_>>();
    if carrier == "Lk" {
        assert!(links.contains(target));
    }
}

pub(super) fn assert_search(content: &ResolvedContent, word: &str, expected_bytes: &str) {
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    let owner_path = artifact
        .nodes()
        .iter()
        .find_map(|mapped| match mapped.node() {
            MarkdownNode::DocumentEntry {
                owner: EntryOwner::Definition(owner),
                path,
                ..
            } if std::ptr::eq(*owner, definition(content)) => Some(path.to_string()),
            _ => None,
        })
        .expect("the actual definition owner has an artifact path");
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let response = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: word.into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope,
                word: false,
                context_lines: 0,
                limit: 100,
                offset: 0,
            },
        )
        .unwrap();
        // Short labels such as X legitimately match the NEXT heading too.
        // Match by original owner, retaining every actual byte occurrence.
        let hits = response
            .matches
            .iter()
            .filter(|hit| hit.outline.path() == owner_path)
            .collect::<Vec<_>>();
        assert_eq!(hits.len(), 1, "{scope:?}: {word}, {}", artifact.text());
        let hit = hits[0];
        assert_ne!(hit.occurrences, [], "search hit has actual byte evidence");
        for occurrence in &hit.occurrences {
            let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                ..usize::try_from(occurrence.markdown.end_byte).unwrap();
            assert_eq!(
                &artifact.text()[range],
                expected_bytes,
                "actual artifact UTF-8 bytes"
            );
        }
    }
}

pub(super) fn assert_owner_ranges(case: &Case, content: &ResolvedContent) {
    let item = definition(content);
    assert_eq!(item.source.unwrap().line, 9);
    let owner = EntryOwner::Definition(item);
    let facts = owner.facts().expect("native declared label owner");
    assert_eq!(facts.names, [case.head.as_str()]);
    assert_ne!(facts.name_bindings, []);
    for binding in &facts.name_bindings {
        assert_eq!(binding.name, 0);
        assert_ne!(binding.occurrences, []);
        for occurrence in &binding.occurrences {
            assert_eq!(
                mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
                case.head
            );
            assert!(
                occurrence
                    .parts
                    .iter()
                    .all(|part| matches!(part.root, mant_ir::EntryInlineRoot::Term { .. }))
            );
        }
    }
    let explanation = mant_query::explain_query(
        content,
        &mant_protocol::ExplanationQuery {
            entry: case.head.clone(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .unwrap();
    let evidence = explanation
        .evidence
        .iter()
        .find(|e| e.class == mant_protocol::EvidenceClass::DirectEntry)
        .unwrap();
    let excerpt = mant_query::select_excerpt(
        content,
        &[mant_protocol::ContentSelector::path(
            evidence.outline.path(),
        )],
    )
    .unwrap();
    let [mant_protocol::ExcerptSelection::DocumentEntry { entry, .. }] =
        excerpt.selections.as_slice()
    else {
        panic!("one original definition owner");
    };
    let Some(EntryOwner::Definition(selected)) = entry.entry_owner() else {
        panic!("original definition shape");
    };
    assert_eq!(selected, item);
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    let entries = artifact
        .nodes()
        .iter()
        .filter(|mapped| {
            let MarkdownNode::DocumentEntry {
                owner: EntryOwner::Definition(mapped_owner),
                names,
                ..
            } = mapped.node()
            else {
                return false;
            };
            assert!(std::ptr::eq(*mapped_owner, item));
            assert_eq!(*names, facts.names.as_slice());
            let bytes = &artifact.text()[mapped.range()];
            assert!(bytes.contains(&case.head) && bytes.contains("BODY"));
            assert!(!bytes.contains("END"), "range must not include next owner");
            true
        })
        .count();
    assert_eq!(entries, 1);
}
