//! Complete declaration names refer to accepted text, never font-run guesses.

use mant_codec::encode::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options, render_markdown,
};
use mant_ir::{
    Block, Document, EntryKind, EntryOwner, ResolvedContent,
    visit::{self, Visit},
};
use mant_protocol::{EvidenceBasis, ExplanationOptions, ExplanationQuery, SearchQuery};
use serde::Deserialize;

#[path = "declaration_names/limits.rs"]
mod limits;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    source: String,
    names: Vec<String>,
    excluded: Vec<String>,
    kind: String,
    native_head: Option<String>,
    body_word: String,
    owner_proof: OwnerProof,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnerProof {
    kind: String,
    head_macro: Option<String>,
}

fn cases() -> Vec<Case> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("declaration_names/cases.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 88);
    serde_json::from_value(fixture["cases"].clone()).unwrap()
}

fn owners(document: &Document) -> Vec<EntryOwner<'_>> {
    struct Collect<'a>(Vec<EntryOwner<'a>>);
    impl<'a> Visit<'a> for Collect<'a> {
        fn visit_block(&mut self, block: &'a Block) {
            match block {
                Block::DefinitionList { items, .. } => self.0.extend(
                    items
                        .iter()
                        .filter(|item| item.entry.is_some())
                        .map(EntryOwner::Definition),
                ),
                Block::List { items, .. } => self.0.extend(
                    items
                        .iter()
                        .filter(|item| item.entry.is_some())
                        .map(EntryOwner::List),
                ),
                _ => {}
            }
            visit::walk_block(self, block);
        }
    }
    let mut collect = Collect(Vec::new());
    collect.visit_document(document);
    collect.0
}

fn text_blocks(document: &Document) -> Vec<String> {
    struct Collect(Vec<String>);
    impl<'a> Visit<'a> for Collect {
        fn visit_block(&mut self, block: &'a Block) {
            match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    self.0.push(mant_ir::inline_plain_text(children));
                }
                Block::DefinitionList { items, .. } => {
                    for item in items {
                        self.0.extend(
                            item.terms
                                .iter()
                                .map(|term| mant_ir::inline_plain_text(term)),
                        );
                        for block in &item.description {
                            self.visit_block(block);
                        }
                    }
                    return;
                }
                _ => {}
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
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
    assert!(!json.contains("mant-native-definition-owner"));
    assert!(!json.contains("mant-field-word"));
    let wire: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let result: ResolvedContent = wire.into();
    assert_eq!(result, original, "{}: actual JSON", case.id);
    assert_eq!(
        mant_ir::validate_document(result.document.as_ref().unwrap()).len(),
        0
    );
    result
}

fn assert_owner(case: &Case, owner: EntryOwner<'_>) {
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, case.names, "{}: complete names", case.id);
    let kind = match case.kind.as_str() {
        "option" => EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        },
        "command" => EntryKind::Command,
        "term" => EntryKind::Term,
        _ => panic!("author-specified entry kind"),
    };
    assert_eq!(facts.kind, kind, "{}: kind", case.id);
    assert_eq!(
        facts.alias_groups.len(),
        0,
        "{}: spellings are not aliases",
        case.id
    );
    assert_eq!(facts.name_bindings.len(), case.names.len());
    for binding in &facts.name_bindings {
        assert_ne!(binding.occurrences.len(), 0);
        for occurrence in &binding.occurrences {
            assert_eq!(
                mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
                case.names[binding.name],
                "{}: original leaf byte binding",
                case.id,
            );
        }
    }
    if let Some(expected) = &case.native_head {
        let forms = owner.forms().unwrap();
        let text = forms
            .iter()
            .map(mant_ir::inline_plain_text)
            .collect::<Vec<_>>();
        assert_eq!(
            text.as_slice(),
            std::slice::from_ref(expected),
            "{}: complete native form",
            case.id
        );
    }
}

#[test]
fn ordered_native_heads_keep_complete_names_fonts_and_actual_content_bindings() {
    // All 88 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // man_term.c::pre_alternate calls term_word once per actual TEXT child;
    // term.c::term_word changes fonts inside that operand without a new word.
    // Mandoc proves the visible HEAD and fonts; semantic names/kinds below
    // are the independently authored, bounded ManT declaration contract.
    for case in cases() {
        let content = roundtrip(&case);
        let entries = owners(content.document.as_ref().unwrap());
        assert_eq!(
            entries.len(),
            usize::from(case.kind != "none"),
            "{}: owner count",
            case.id
        );
        for owner in entries {
            assert_owner(&case, owner);
            let native = libmandoc_rs::Parser::default()
                .parse_bytes("decl.1", case.source.as_bytes())
                .unwrap();
            let source = owner.source().unwrap();
            let head_macro = case.owner_proof.head_macro.as_deref().unwrap();
            let reached = match case.owner_proof.kind.as_str() {
                "native-definition" => has_native_head(&native.document.root, source, head_macro),
                "literal-relative-body" => has_literal_relative_pair(
                    &native.document.root,
                    source,
                    head_macro,
                    &case.body_word,
                ),
                _ => panic!("a no-owner source cannot produce an entry"),
            };
            assert!(reached, "{}: actual source ownership", case.id);
        }
        let blocks = text_blocks(content.document.as_ref().unwrap());
        assert_eq!(
            blocks
                .iter()
                .filter(|block| block.as_str() == case.body_word)
                .count(),
            1,
            "{}: body retained once",
            case.id
        );
    }
}

fn has_native_head(
    node: &libmandoc_rs::Node,
    source: mant_ir::SourceSpan,
    macro_name: &str,
) -> bool {
    node.kind == libmandoc_rs::NodeKind::Block
        && node.macro_name.as_deref() == Some(macro_name)
        && (node.line, node.column) == (source.line, source.column)
        && node
            .children
            .iter()
            .any(|child| child.kind == libmandoc_rs::NodeKind::Head && !child.children.is_empty())
        || node
            .children
            .iter()
            .any(|child| has_native_head(child, source, macro_name))
}

fn native_contains_source(node: &libmandoc_rs::Node, source: mant_ir::SourceSpan) -> bool {
    (node.line, node.column) == (source.line, source.column)
        || node
            .children
            .iter()
            .any(|child| native_contains_source(child, source))
}

fn native_contains_text(node: &libmandoc_rs::Node, value: &str) -> bool {
    node.kind == libmandoc_rs::NodeKind::Text && node.text.as_deref() == Some(value)
        || node
            .children
            .iter()
            .any(|child| native_contains_text(child, value))
}

fn has_literal_relative_pair(
    node: &libmandoc_rs::Node,
    source: mant_ir::SourceSpan,
    macro_name: &str,
    body: &str,
) -> bool {
    // man_term.c::pre_PP supplies paragraph spacing, pre_B/pre_MR text,
    // pre_RS/post_RS a distinct indented BODY. man validation removes the
    // initial empty PP after SH in these exact inputs: pristine tree proves
    // B/MR and RS are actual adjacent siblings, not native TP/IP HEADs.
    // Owner source must remain within that exact literal subtree; description
    // text must belong to its actual following RS BODY, independently of kind.
    node.children.windows(2).any(|siblings| {
        let head = &siblings[0];
        let tail = &siblings[1];
        head.kind == libmandoc_rs::NodeKind::Element
            && head.macro_name.as_deref() == Some(macro_name)
            && native_contains_source(head, source)
            && tail.kind == libmandoc_rs::NodeKind::Block
            && tail.macro_name.as_deref() == Some("RS")
            && tail.children.iter().any(|child| {
                child.kind == libmandoc_rs::NodeKind::Body && native_contains_text(child, body)
            })
    }) || node
        .children
        .iter()
        .any(|child| has_literal_relative_pair(child, source, macro_name, body))
}

fn names_from_explain(content: &ResolvedContent, name: &str) -> Vec<String> {
    mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap()
    .evidence
    .into_iter()
    .filter(|evidence| {
        evidence
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
    })
    .map(|evidence| evidence.outline.node.id().to_owned())
    .collect()
}

fn search(content: &ResolvedContent, name: &str) -> mant_protocol::QuerySearch {
    mant_query::search_query(
        content,
        &SearchQuery {
            pattern: name.into(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Sensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 100,
            offset: 0,
        },
    )
    .unwrap()
}

#[test]
fn native_name_queries_and_markdown_ranges_select_only_the_authoritative_owner() {
    for case in cases() {
        let content = roundtrip(&case);
        let entries = owners(content.document.as_ref().unwrap());
        let artifact =
            render_addressable_markdown_with_options(&content, MarkdownOptions::ADDRESSABLE);
        for owner in &entries {
            let facts = owner.facts().unwrap();
            let mapped = artifact.nodes().iter().filter(|mapped| matches!(mapped.node(),
                MarkdownNode::DocumentEntry { owner: candidate, .. } if candidate.facts().is_some_and(|entry| entry.id == facts.id)))
                .collect::<Vec<_>>();
            assert_eq!(mapped.len(), 1, "{}: one mapped owner", case.id);
            assert!(artifact.text()[mapped[0].range()].contains(&case.body_word));
            for name in &case.names {
                assert_eq!(
                    names_from_explain(&content, name),
                    [facts.id.to_string()],
                    "{}: exact name evidence",
                    case.id
                );
                let excerpt =
                    crate::semantic_test_read::semantic_excerpt(&content, &[name]).unwrap();
                assert!(mant_render::render_excerpt_text(&excerpt).contains(&case.body_word));
                let found = search(&content, name);
                let hits = found
                    .matches
                    .iter()
                    .filter(|hit| hit.outline.node.id() == facts.id.as_str())
                    .collect::<Vec<_>>();
                assert_ne!(hits.len(), 0, "{}: actual owner search", case.id);
                let occurrences = hits
                    .iter()
                    .flat_map(|hit| &hit.occurrences)
                    .collect::<Vec<_>>();
                assert_ne!(occurrences.len(), 0);
                for occurrence in occurrences {
                    assert_eq!(&occurrence.matched_text, name);
                    let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
                    let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
                    assert_eq!(visible_range(artifact.text(), start..end), *name);
                    let start_position = position(artifact.text(), start);
                    let end_position = position(artifact.text(), end);
                    assert_eq!(
                        start_position,
                        (
                            occurrence.markdown.start_line,
                            occurrence.markdown.start_column
                        )
                    );
                    assert_eq!(
                        end_position,
                        (occurrence.markdown.end_line, occurrence.markdown.end_column)
                    );
                }
            }
        }
        for excluded in &case.excluded {
            assert_eq!(
                names_from_explain(&content, excluded).len(),
                0,
                "{}: argument is no selector",
                case.id
            );
        }
    }
}

fn position(markdown: &str, byte: usize) -> (u32, u32) {
    let prefix = &markdown[..byte];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
    (u32::try_from(line).unwrap(), u32::try_from(column).unwrap())
}

fn visible_range(markdown: &str, wanted: std::ops::Range<usize>) -> String {
    // SearchMarkdownRange is a raw byte envelope. When a name crosses a
    // font-run seam it includes intervening delimiters, e.g. --a*ll. Parse
    // the complete artifact independently; reparsing that envelope alone
    // would invent unmatched CommonMark delimiters and a different label.
    let mut visible = String::new();
    let mut first = None;
    let mut last = None;
    for (event, range) in pulldown_cmark::Parser::new(markdown).into_offset_iter() {
        if range.end <= wanted.start || range.start >= wanted.end {
            continue;
        }
        let (pulldown_cmark::Event::Text(value) | pulldown_cmark::Event::Code(value)) = event
        else {
            continue;
        };
        let mut source = &markdown[range.clone()];
        let mut offset = range.start;
        if source.starts_with('`') {
            let ticks = source.bytes().take_while(|byte| *byte == b'`').count();
            source = &source[ticks..source.len() - ticks];
            offset += ticks;
            if source != value.as_ref() && source.starts_with(' ') && source.ends_with(' ') {
                source = &source[1..source.len() - 1];
                offset += 1;
            }
        }
        for character in value.chars() {
            let escaped = source.starts_with('\\') && character.is_ascii_punctuation();
            let width = character.len_utf8() + usize::from(escaped);
            assert_eq!(
                source.get(usize::from(escaped)..width),
                Some(character.to_string().as_str()),
                "this fixture uses exact text/code or one punctuation escape"
            );
            let character_range = offset..offset + width;
            if character_range.start >= wanted.start && character_range.end <= wanted.end {
                first.get_or_insert(character_range.start);
                last = Some(character_range.end);
                visible.push(character);
            }
            source = &source[width..];
            offset += width;
        }
        assert_eq!(source, "");
    }
    assert_eq!(first, Some(wanted.start));
    assert_eq!(last, Some(wanted.end));
    visible
}

fn flow_words(blocks: &[String]) -> String {
    blocks
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn portable_markdown_reader_keeps_source_order_and_parameter_glyphs() {
    // This assertion checks portable text, not native device columns or hard
    // row geometry (covered by the execution and consumer row matrices).
    for case in cases() {
        let content = roundtrip(&case);
        let markdown = render_markdown(&content);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_eq!(
            flow_words(&text_blocks(read.document.as_ref().unwrap())),
            flow_words(&text_blocks(content.document.as_ref().unwrap())),
            "{}: actual Markdown readback",
            case.id,
        );
    }
}
