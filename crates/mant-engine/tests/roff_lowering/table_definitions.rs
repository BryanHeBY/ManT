//! Portable table export retains the native definition seam, not a colon.
use super::*;

const SOURCE: &str = ".Dd October 4, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd inspect layout\n.Sh DESCRIPTION\n.Bl -column abc -compact\n.It Xo\n.Bl -hang -width 0n -compact\n.It Xo\n.br\n.No XX\n.Xc\n.No BODY\n.El\n.Xc\n.El\n";

#[test]
fn nested_native_definitions_keep_joined_and_separated_words_in_table_readback() {
    // Both exact sources ran the authenticated pristine ASCII/UTF-8/HTML/
    // tree/lint profiles before these assertions (all statuses zero).
    // termp_it_pre/post plus roff_term_pre_br clear trailspace on .br;
    // HANG then retains the physical row, yielding XXBODY only with .br.
    for (source, expected) in [
        (SOURCE.to_owned(), "XXBODY"),
        (SOURCE.replace(".br\n", ""), "XX BODY"),
    ] {
        assert_nested_ast_owners(&source);
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = mant_render::render_query_json(&query, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let restored: ResolvedContent = restored.into();
        assert_eq!(query.document, restored.document);
        for content in [&query, &restored] {
            assert_entry_mapping(content, expected);
            let text = mant_render::render_query_man(content);
            let body = text
                .split_once("DESCRIPTION\n")
                .unwrap()
                .1
                .trim_end_matches('\n');
            // Only the common parent margin is outside this word/row axis.
            assert_eq!(
                body.lines().map(str::trim_start).collect::<Vec<_>>(),
                [expected]
            );
            let markdown = mant_codec::encode::render_markdown_with_options(
                content,
                mant_codec::encode::MarkdownOptions::default(),
            );
            let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let document = imported.document.unwrap();
            let description = document
                .sections
                .iter()
                .find(|section| {
                    mant_ir::inline_plain_text(&section.heading.content) == "DESCRIPTION"
                })
                .unwrap();
            let [Block::Preformatted { children, .. }] = description.blocks.as_slice() else {
                panic!("one literal table projection: {markdown}");
            };
            // The nested .El contributes one completed VerticalSpace row
            // inside the cell. A fence preserves its two row delimiters,
            // while the document text printer trims page-final spacing.
            assert_eq!(
                mant_ir::inline_plain_text(children),
                format!("{expected}\n\n")
            );
            assert!(!mant_ir::inline_plain_text(children).contains(':'));
        }
    }
}

fn assert_entry_mapping(content: &ResolvedContent, expected: &str) {
    use mant_codec::encode::{
        MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options,
    };
    use mant_ir::EntryOwner;
    use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};
    let document = content.document.as_ref().unwrap();
    let Block::Table { rows, .. } = &document.sections[1].blocks[0] else {
        unreachable!()
    };
    let Block::DefinitionList { items, .. } = &rows[0].cells[0].blocks[0] else {
        unreachable!()
    };
    let item = &items[0];
    let owner = EntryOwner::Definition(item);
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, ["XX"]);
    assert_eq!(facts.name_bindings.len(), 1);
    for binding in &facts.name_bindings {
        for occurrence in &binding.occurrences {
            assert_eq!(
                mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
                "XX"
            );
        }
    }
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    let mapped = artifact.nodes().iter().filter(|mapped| {
        matches!(mapped.node(), MarkdownNode::DocumentEntry { owner: EntryOwner::Definition(mapped_owner), names, .. }
            if std::ptr::eq(*mapped_owner, item) && *names == facts.names.as_slice())
    }).collect::<Vec<_>>();
    assert_eq!(mapped.len(), 1);
    assert!(artifact.text()[mapped[0].range()].contains(expected));
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let search = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: expected.into(),
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
        assert_eq!(search.matches.len(), 1, "{scope:?}");
        for occurrence in &search.matches[0].occurrences {
            let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                ..usize::try_from(occurrence.markdown.end_byte).unwrap();
            assert_eq!(&artifact.text()[range], expected);
        }
    }
}

fn assert_nested_ast_owners(source: &str) {
    use libmandoc_rs::{Node, NodeKind, Parser};
    fn contains(node: &Node, text: &str) -> bool {
        node.text.as_deref() == Some(text)
            || node.children.iter().any(|child| contains(child, text))
    }
    fn find(node: &Node) -> Option<&Node> {
        if node.kind == NodeKind::Block
            && node.macro_token.as_deref() == Some("It")
            && node
                .children
                .iter()
                .any(|child| child.kind == NodeKind::Head && contains(child, "XX"))
        {
            Some(node)
        } else {
            node.children.iter().find_map(find)
        }
    }
    let parsed = Parser::default()
        .parse_bytes("table-definition.1", source.as_bytes())
        .unwrap();
    let item = find(&parsed.document.root).unwrap();
    let head = item
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Head)
        .unwrap();
    let body = item
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Body)
        .unwrap();
    assert!(contains(head, "XX"));
    assert!(!contains(head, "BODY"));
    assert!(contains(body, "BODY"));
}
