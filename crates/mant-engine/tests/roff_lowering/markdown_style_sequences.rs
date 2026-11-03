//! Native alternating fonts survive complete Markdown phrasing readback.

use mant_codec::encode::{MarkdownNode, render_addressable_markdown, render_markdown};
use mant_ir::{Block, EntryOwner, Inline, ResolvedContent};

const HEADER: &str = ".TH PROBE 1 \"November 3, 2026\"\n.SH NAME\nprobe \\- audit\n.SH OPTIONS\n";

fn roundtrip(source: &str) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let decoded = ResolvedContent::from(decoded);
    assert_eq!(decoded, original);
    decoded
}

fn styles(nodes: &[Inline], mask: u8) -> Vec<(char, u8)> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            Inline::Text { value } => {
                output.extend(value.chars().map(|character| (character, mask)));
            }
            Inline::Strong { children } => output.extend(styles(children, mask | 1)),
            Inline::Emphasis { children } => output.extend(styles(children, mask | 2)),
            Inline::Link { children, .. } => output.extend(styles(children, mask)),
            Inline::Anchor { .. } => {}
            _ => panic!("original alternating font phrasing: {node:?}"),
        }
    }
    output
}

fn assert_visible_search(content: &ResolvedContent, text: &str) {
    let response = mant_query::search_query(
        content,
        &mant_protocol::SearchQuery {
            pattern: text.into(),
            syntax: mant_protocol::SearchSyntax::Literal,
            case: mant_protocol::SearchCase::Sensitive,
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    let occurrences = response
        .matches
        .iter()
        .flat_map(|hit| &hit.occurrences)
        .collect::<Vec<_>>();
    assert_eq!(occurrences.len(), 1, "{text}: {response:#?}");
    assert_eq!(occurrences[0].matched_text, text);
}

fn options(content: &ResolvedContent) -> &[Block] {
    &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "OPTIONS")
        .unwrap()
        .blocks
}

#[test]
fn native_bi_and_ib_keep_exact_visible_scalars_and_alternating_fonts_after_markdown() {
    // All twelve exact inputs ran pristine ASCII/UTF-8(width78)/HTML before
    // these assertions. man_term.c::pre_alternate selects BOLD/UNDER for BI,
    // UNDER/BOLD for IB and NOSPACE between operands. No marker or separator
    // belongs to that executed content, including intraword underscores.
    for (macro_name, left, right) in [
        ("BI", "-fstack-reuse=", "reuse_level"),
        ("IB", "reuse_level", "=reuse"),
        ("BI", "$((", "arithmetic_expression"),
        ("IB", "arithmetic_expression", "))"),
        ("BI", "--arg=", "word_value"),
        ("IB", "word_value", ",tail"),
        ("BI", "prefix_", "suffix"),
        ("IB", "prefix", "_suffix"),
        ("BI", "é中=", "参数_值"),
        ("IB", "参数_值", "=é中"),
        ("BI", "a*b", "c_d"),
        ("IB", "a_b", "c*d"),
    ] {
        let source = format!("{HEADER}.{macro_name} \"{left}\" \"{right}\"\n");
        let content = roundtrip(&source);
        let [Block::Paragraph { children, .. }] = options(&content) else {
            panic!("one original paragraph owner: {source}")
        };
        let (first, second) = if macro_name == "BI" { (1, 2) } else { (2, 1) };
        let expected = left
            .chars()
            .map(|character| (character, first))
            .chain(right.chars().map(|character| (character, second)))
            .collect::<Vec<_>>();
        assert_eq!(styles(children, 0), expected, "{source}");
        let markdown = render_markdown(&content);
        let parsed = mant_codec::parse_markdown(&markdown, None).unwrap();
        let decoded = ResolvedContent {
            address: None,
            label: "readback".into(),
            document: Some(parsed.document),
            tldr: parsed.tldr,
        };
        let [Block::Paragraph { children, .. }] = options(&decoded) else {
            panic!("same readback paragraph owner: {markdown}")
        };
        assert_eq!(styles(children, 0), expected, "{source}: {markdown}");
        for query in [&content, &decoded] {
            assert_visible_search(query, &format!("{left}{right}"));
        }
    }
}

#[test]
fn alternating_option_styles_keep_original_name_binding_and_borrowed_markdown_owner() {
    // Exact TP+BI input ran pristine ASCII/UTF-8/HTML/tree before assertions.
    // pre_alternate's font/NOSPACE rule applies within the unchanged TP HEAD;
    // the option name remains the original --arg subrange, not its argument.
    let source = format!("{HEADER}.TP\n.BI \"--arg=\" \"reuse_level\"\nBodyWord.\n");
    let content = roundtrip(&source);
    let [Block::DefinitionList { items, .. }] = options(&content) else {
        panic!("one original definition list")
    };
    let [item] = items.as_slice() else {
        panic!("one original option owner")
    };
    assert_eq!(
        item.source
            .as_ref()
            .map(|source| (source.line, source.column)),
        Some((5, 2))
    );
    assert_eq!(item.terms.len(), 1);
    assert_eq!(
        mant_ir::inline_plain_text(&item.terms[0]),
        "--arg=reuse_level"
    );
    let owner = EntryOwner::Definition(item);
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, ["--arg"]);
    assert_eq!(facts.name_bindings.len(), 1);
    let binding = &facts.name_bindings[0];
    assert_eq!(binding.name, 0);
    assert_eq!(binding.occurrences.len(), 1);
    let name = owner.form(&binding.occurrences[0]).unwrap();
    assert_eq!(mant_ir::inline_plain_text(&name), "--arg");
    let original = content.clone();
    let artifact = render_addressable_markdown(&content);
    let entries = artifact
        .nodes()
        .iter()
        .filter_map(|mapped| match mapped.node() {
            MarkdownNode::DocumentEntry {
                owner: EntryOwner::Definition(candidate),
                names,
                ..
            } if std::ptr::eq(*candidate, item) => Some((mapped, *names)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert!(std::ptr::eq(entries[0].1.as_ptr(), facts.names.as_ptr()));
    let rendered = &artifact.text()[entries[0].0.range()];
    assert!(rendered.contains("BodyWord."));
    assert!(rendered.contains("**--arg=**_reuse_level_"));
    assert_eq!(
        content, original,
        "presentation cannot rewrite owner/name roots"
    );
}
